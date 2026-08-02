//! Chrome DevTools Protocol client for reliable browser automation.
//!
//! Connects to a Chrome/Chromium instance running with
//! `--remote-debugging-port=9222`. Provides DOM-level operations
//! that work on React SPAs where coordinate-based clicks fail.

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

/// CDP message id counter.
type CdpId = u64;

/// A connected CDP session to a specific page (target).
pub struct CdpClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: CdpId,
    base_url: String,
}

/// Summary of a debuggable page returned by GET /json.
#[derive(Debug, serde::Deserialize)]
pub struct TargetInfo {
    #[serde(rename = "webSocketDebuggerUrl")]
    pub ws_url: String,
    pub title: String,
    pub url: String,
    #[serde(rename = "type")]
    pub target_type: String,
}

/// Ensure Chrome is running with --remote-debugging-port. Returns the port.
pub async fn ensure_chrome_debug(port: u16) -> anyhow::Result<u16> {
    // Check if already available
    if tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_ok()
    {
        return Ok(port);
    }

    // Try to launch Chrome
    let chrome_paths = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];

    let chrome = chrome_paths
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Chrome not found. Install Chrome or set --remote-debugging-port={port} manually."
            )
        })?;

    let mut cmd = tokio::process::Command::new(chrome);
    cmd.arg(format!("--remote-debugging-port={port}"));
    let user_data = std::env::temp_dir().join("stitch-chrome-cdp");
    cmd.arg(format!("--user-data-dir={}", user_data.display()));
    cmd.arg("--no-first-run");
    cmd.arg("--no-default-browser-check");
    cmd.kill_on_drop(false);
    cmd.spawn()?;

    // Wait for port to open (up to 15 seconds)
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            tracing::info!("Chrome DevTools ready on port {port}");
            return Ok(port);
        }
    }

    anyhow::bail!("Chrome started but port {port} never opened")
}

impl CdpClient {
    /// Discover debuggable pages and connect to the best candidate.
    ///
    /// `debug_port` is typically 9222.
    /// `prefer_url` — if provided, prefer the page whose URL contains this.
    pub async fn connect(debug_port: u16, prefer_url: Option<&str>) -> anyhow::Result<Self> {
        // Auto-launch Chrome if needed
        ensure_chrome_debug(debug_port).await?;

        let base = format!("http://localhost:{debug_port}");
        let list_url = format!("{base}/json");

        let client = reqwest::Client::new();
        let resp = client.get(&list_url).send().await.with_context(|| {
            format!(
                "Cannot reach Chrome DevTools at {list_url}. \
                 Make sure Chrome is running with --remote-debugging-port={debug_port}"
            )
        })?;

        let targets: Vec<TargetInfo> = resp
            .json()
            .await
            .context("Failed to parse /json response")?;

        // Pick the best page target
        let target = pick_target(&targets, prefer_url).ok_or_else(|| {
            anyhow::anyhow!(
                "No debuggable page found. Open a page in Chrome first. Targets: {targets:?}"
            )
        })?;

        tracing::info!(
            title = %target.title,
            url = %target.url,
            "CDP connecting to target"
        );

        let (ws, _resp) = connect_async(&target.ws_url)
            .await
            .context("CDP WebSocket handshake failed")?;

        // Enable Runtime domain so we can evaluate JS
        let mut client = CdpClient {
            ws,
            next_id: 1,
            base_url: base,
        };
        let _ = client.send("Runtime.enable", serde_json::json!({})).await;
        let _ = client.send("Page.enable", serde_json::json!({})).await;
        let _ = client.send("DOM.enable", serde_json::json!({})).await;

        Ok(client)
    }

    /// Navigate to a URL. Returns the page title after load.
    pub async fn navigate(&mut self, url: &str) -> anyhow::Result<String> {
        let result = self
            .send("Page.navigate", serde_json::json!({"url": url}))
            .await?;

        // Wait for page load
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let loader_id = result
            .pointer("/result/loaderId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if let Some(lid) = loader_id {
            // Wait for load event
            let _ =
                tokio::time::timeout(std::time::Duration::from_secs(10), self.wait_for_load(&lid))
                    .await;
        }

        let title = self.get_title().await.unwrap_or_default();
        Ok(format!("Navigated to {url} — title: \"{title}\""))
    }

    /// Click the first element matching a CSS selector.
    /// Uses DOM.getBoxModel for precise coordinates, then Input.dispatchMouseEvent.
    pub async fn click(&mut self, selector: &str) -> anyhow::Result<String> {
        let t0 = std::time::Instant::now();
        // Get document root node id
        let doc = self
            .send("DOM.getDocument", serde_json::json!({"depth": 0}))
            .await?;
        let root_node_id = doc
            .pointer("/result/root/nodeId")
            .and_then(|v| v.as_i64())
            .context("No root nodeId")?;

        // Query selector
        let query_result = self
            .send(
                "DOM.querySelector",
                serde_json::json!({
                    "nodeId": root_node_id,
                    "selector": selector,
                }),
            )
            .await?;

        let node_id = query_result
            .pointer("/result/nodeId")
            .and_then(|v| v.as_i64())
            .filter(|&id| id != 0)
            .with_context(|| format!("No element matching selector: {selector}"))?;

        // Get tag name and text for reporting
        let tag = self.get_node_info(node_id).await?;

        // Try box-model click first (most precise)
        let box_result = self
            .send("DOM.getBoxModel", serde_json::json!({"nodeId": node_id}))
            .await;

        match box_result {
            Ok(v) => {
                let quad = v
                    .pointer("/result/model/content")
                    .and_then(|q| q.as_array())
                    .context("No box model — element may be invisible")?;

                let mut cx = 0.0f64;
                let mut cy = 0.0f64;
                for i in 0..4 {
                    cx += quad[i * 2].as_f64().unwrap_or(0.0);
                    cy += quad[i * 2 + 1].as_f64().unwrap_or(0.0);
                }
                cx /= 4.0;
                cy /= 4.0;

                self.dispatch_mouse(cx, cy, "mousePressed").await?;
                self.dispatch_mouse(cx, cy, "mouseReleased").await?;

                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                Ok(format!(
                    "Clicked <{tag}> at ({cx:.0}, {cy:.0}) via box-model, selector=\"{selector}\" ({ms:.0}ms)"
                ))
            }
            Err(_) => {
                // Box model failed (e.g. zero-size or hidden element).
                // Fall back to JS click — works for hidden compatibility forms.
                self.evaluate(&format!(
                    "(function(){{ var el=document.querySelector('{sel}'); if(el){{ el.click(); return 'clicked '+el.tagName; }} return 'not found'; }})()",
                    sel = selector.replace('\\', "\\\\").replace('\'', "\\'")
                ))
                .await?;
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                Ok(format!(
                    "Clicked <{tag}> via JS click() (box-model unavailable), selector=\"{selector}\" ({ms:.0}ms)"
                ))
            }
        }
    }

    /// Get the full text content of the page via DOM (no OCR noise).
    pub async fn read_page_text(&mut self) -> anyhow::Result<String> {
        let result = self
            .send(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": "document.body ? document.body.innerText : ''",
                    "returnByValue": true,
                }),
            )
            .await?;

        let text = result
            .pointer("/result/result/value")
            .and_then(|v| v.as_str())
            .unwrap_or("(empty)");

        // Truncate for token efficiency
        let limit = 30_000;
        if text.len() > limit {
            Ok(format!(
                "{}... [truncated at {limit} chars, total {}]",
                &text[..limit],
                text.len()
            ))
        } else {
            Ok(text.to_string())
        }
    }

    /// Execute arbitrary JavaScript in the page context.
    pub async fn evaluate(&mut self, js: &str) -> anyhow::Result<String> {
        let result = self
            .send(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": js,
                    "returnByValue": true,
                }),
            )
            .await?;

        // Format the result
        if let Some(err) = result.pointer("/result/exceptionDetails") {
            let text = err
                .pointer("/text")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Ok(format!("JS error: {text}"));
        }

        let value = result.pointer("/result/result/value");
        match value {
            Some(v) => Ok(serde_json::to_string_pretty(v).unwrap_or_else(|_| format!("{v}"))),
            None => Ok("undefined".into()),
        }
    }

    /// Focus an element (by selector) and type text into it.
    pub async fn type_into(&mut self, selector: &str, text: &str) -> anyhow::Result<String> {
        // Focus the element
        let _ = self
            .send(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": format!(
                        "(function(){{ var el=document.querySelector('{sel}'); if(el){{ el.focus(); el.select(); return true; }} return false; }})()",
                        sel = selector.replace('\'', "\\'")
                    ),
                    "returnByValue": true,
                }),
            )
            .await?;

        // Type each character via Input.dispatchKeyEvent
        for ch in text.chars() {
            let key = match ch {
                '\n' | '\r' => "Enter",
                '\t' => "Tab",
                ' ' => " ",
                _ => {
                    // For regular chars, use char type
                    self.dispatch_char(ch).await?;
                    continue;
                }
            };
            self.dispatch_key(key).await?;
        }

        Ok(format!("Typed \"{text}\" into \"{selector}\""))
    }

    // ── internal helpers ──

    async fn send(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let msg = serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        });

        let text = serde_json::to_string(&msg)?;
        self.ws
            .send(tokio_tungstenite::tungstenite::Message::Text(text))
            .await?;

        // Read response — CDP sends matching id
        loop {
            match self.ws.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(t))) => {
                    let v: Value = serde_json::from_str(&t)?;
                    if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                        if let Some(err) = v.get("error") {
                            let msg = err
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("unknown CDP error");
                            return Err(anyhow::anyhow!("CDP {method}: {msg}"));
                        }
                        return Ok(v);
                    }
                    // Event/notification — skip (e.g. Page.loadEventFired)
                }
                Some(Ok(_)) => {} // Binary frames — ignore
                Some(Err(e)) => return Err(anyhow::anyhow!("CDP WebSocket error: {e}")),
                None => return Err(anyhow::anyhow!("CDP WebSocket closed")),
            }
        }
    }

    async fn dispatch_mouse(&mut self, x: f64, y: f64, event_type: &str) -> anyhow::Result<()> {
        let _ = self
            .send(
                "Input.dispatchMouseEvent",
                serde_json::json!({
                    "type": event_type,
                    "x": x,
                    "y": y,
                    "button": "left",
                    "clickCount": 1,
                }),
            )
            .await;
        Ok(())
    }

    async fn dispatch_key(&mut self, key: &str) -> anyhow::Result<()> {
        let _ = self
            .send(
                "Input.dispatchKeyEvent",
                serde_json::json!({
                    "type": "keyDown",
                    "key": key,
                }),
            )
            .await;
        let _ = self
            .send(
                "Input.dispatchKeyEvent",
                serde_json::json!({
                    "type": "keyUp",
                    "key": key,
                }),
            )
            .await;
        Ok(())
    }

    async fn dispatch_char(&mut self, ch: char) -> anyhow::Result<()> {
        let text: String = ch.into();
        let _ = self
            .send(
                "Input.dispatchKeyEvent",
                serde_json::json!({
                    "type": "char",
                    "text": text,
                }),
            )
            .await;
        Ok(())
    }

    async fn get_node_info(&mut self, node_id: i64) -> anyhow::Result<String> {
        let result = self
            .send(
                "DOM.describeNode",
                serde_json::json!({"nodeId": node_id, "depth": 1}),
            )
            .await?;

        let node = result.pointer("/result/node").context("No node info")?;
        let tag = node
            .get("nodeName")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_lowercase();
        let text = node
            .get("children")
            .and_then(|v| v.as_array())
            .and_then(|children| {
                children
                    .iter()
                    .find(|c| c.get("nodeType").and_then(|t| t.as_i64()) == Some(3))
                    .and_then(|c| c.get("nodeValue").and_then(|v| v.as_str()))
            })
            .unwrap_or("");
        let text_trimmed: String = text.chars().take(60).collect();
        Ok(if text_trimmed.is_empty() {
            format!("<{tag}>")
        } else {
            format!("<{tag}> \"{text_trimmed}\"")
        })
    }

    async fn get_title(&mut self) -> anyhow::Result<String> {
        let result = self
            .send(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": "document.title",
                    "returnByValue": true,
                }),
            )
            .await?;
        Ok(result
            .pointer("/result/result/value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
    }

    async fn wait_for_load(&mut self, _loader_id: &str) {
        // Simply wait for Page.loadEventFired notification.
        // We read messages until we see the event or timeout.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, self.ws.next()).await {
                Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(t)))) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t)
                        && v.get("method").and_then(|m| m.as_str()) == Some("Page.loadEventFired")
                    {
                        break;
                    }
                }
                _ => break,
            }
        }
    }
}

/// Pick the best page target from /json.
fn pick_target<'a>(targets: &'a [TargetInfo], prefer_url: Option<&str>) -> Option<&'a TargetInfo> {
    // Prefer a target whose URL matches the hint
    if let Some(hint) = prefer_url
        && let Some(t) = targets.iter().find(|t| t.url.contains(hint))
    {
        return Some(t);
    }

    // Pick the first "page" type target
    targets.iter().find(|t| t.target_type == "page")
}
