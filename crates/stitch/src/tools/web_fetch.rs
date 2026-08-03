//! Web fetch tool.
//!
//! Fetches content from a URL and returns text/html. Uses reqwest
//! with timeouts and size limits for safety.

use super::{ToolDef, ToolResult};

/// Maximum response body size in bytes.
const MAX_RESPONSE_BYTES: usize = 500_000;

/// Request timeout in seconds.
const TIMEOUT_SECS: u64 = 15;

#[derive(Clone)]
pub struct WebFetch;

impl Default for WebFetch {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetch {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "web_fetch".into(),
            description: "Fetch content from a URL. Returns the response body as text. \
                 Use for reading documentation, API responses, or web pages. \
                 Respects timeouts and size limits."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch (must start with http:// or https://)"
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional HTTP headers to include in the request",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["url"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let url = arguments["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' argument"))?;

        // Only allow http/https
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolResult::fail(
                "Only http:// and https:// URLs are supported.",
            ));
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
            .user_agent("Stitch/0.1 (PromptStdio Agent)")
            .build()?;

        let mut req = client.get(url);

        // Add custom headers if provided
        if let Some(headers) = arguments["headers"].as_object() {
            for (key, value) in headers {
                if let Some(v) = value.as_str() {
                    req = req.header(key.as_str(), v);
                }
            }
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult::fail(format!("Request failed: {e}")));
            }
        };

        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();

        // Read body up to limit
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult::fail(format!(
                    "Failed to read response body: {e}"
                )));
            }
        };

        let truncated = bytes.len() > MAX_RESPONSE_BYTES;

        if truncated {
            let body = String::from_utf8_lossy(&bytes[..MAX_RESPONSE_BYTES]);
            Ok(ToolResult::ok(format!(
                "Status: {status}\nContent-Type: {content_type}\n\n{body}\n\n[... truncated at {MAX_RESPONSE_BYTES} bytes, total {} bytes]",
                bytes.len()
            )))
        } else {
            let body = String::from_utf8_lossy(&bytes);
            Ok(ToolResult::ok(format!(
                "Status: {status}\nContent-Type: {content_type}\n\n{body}"
            )))
        }
    }
}
