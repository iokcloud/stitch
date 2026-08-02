//! Stitch ↔ PromptStdio 账号最短连接：本机 HTTP 回调收 Token。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use stitch::config::{McpProfile, StitchConfig};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::commands::{self, ConfigSnapshot, config_to_snapshot, open_http_url};

static CONNECT_BUSY: AtomicBool = AtomicBool::new(false);

const CONNECT_TIMEOUT: Duration = Duration::from_secs(300);
const READ_TIMEOUT: Duration = Duration::from_secs(15);

/// 打开网站连接页，本机监听一次性回调，写入当前账号 Token。
#[tauri::command]
pub async fn start_account_connect(_app: AppHandle) -> Result<ConfigSnapshot, String> {
    if CONNECT_BUSY.swap(true, Ordering::SeqCst) {
        return Err("正在等待网站连接，请先完成浏览器中的步骤".into());
    }
    let result = run_account_connect().await;
    CONNECT_BUSY.store(false, Ordering::SeqCst);
    result
}

async fn run_account_connect() -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let _ = cfg.ensure_mcp_profiles_seeded();

    let profile_id = cfg
        .active_mcp_id
        .clone()
        .or_else(|| cfg.mcp_profiles.first().map(|p| p.id.clone()))
        .unwrap_or_else(|| "default".into());
    let (api_base, profile_label) = cfg
        .mcp_profiles
        .iter()
        .find(|p| p.id == profile_id)
        .map(|p| (p.api_base.clone(), p.label.clone()))
        .unwrap_or_else(|| (cfg.api_base.clone(), "PromptStdio".into()));
    let api_base = {
        let t = api_base.trim().trim_end_matches('/');
        if t.is_empty() {
            "https://www.promptstdio.com".to_string()
        } else if let Some(next) = stitch::config::normalize_promptstdio_api_base(t) {
            next.to_string()
        } else {
            t.to_string()
        }
    };

    let state = Uuid::new_v4().simple().to_string();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("无法启动本机回调：{e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let callback = format!("http://127.0.0.1:{port}/callback");
    let connect_url = format!(
        "{api_base}/stitch/connect?callback={}&state={}",
        urlencoding::encode(&callback),
        urlencoding::encode(&state)
    );
    open_http_url(&connect_url)?;

    let token = wait_for_callback_token(listener, &state).await?;

    cfg.upsert_mcp_profile(McpProfile {
        id: profile_id,
        label: if profile_label.trim().is_empty() {
            "PromptStdio".into()
        } else {
            profile_label
        },
        api_base: api_base.clone(),
        api_token: Some(token),
    })
    .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    let _ = commands::add_promptstdio_mcp_preset();
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

async fn wait_for_callback_token(
    listener: TcpListener,
    expected_state: &str,
) -> Result<String, String> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("连接超时。请在设置中重试「打开网站连接」。".into());
        }
        let (mut stream, _) = tokio::time::timeout(remaining, listener.accept())
            .await
            .map_err(|_| "连接超时。请在设置中重试「打开网站连接」。".to_string())?
            .map_err(|e| format!("本机回调失败：{e}"))?;

        let mut buf = vec![0u8; 8192];
        let n = match tokio::time::timeout(READ_TIMEOUT, stream.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "account_connect read failed");
                let _ = write_html(&mut stream, 400, "<p>请求无效。</p>").await;
                continue;
            }
            Err(_) => {
                let _ = write_html(&mut stream, 408, "<p>请求超时。</p>").await;
                continue;
            }
        };
        let req = String::from_utf8_lossy(&buf[..n]);
        let first = req.lines().next().unwrap_or("");
        let path_q = first
            .strip_prefix("GET ")
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or("");

        if !path_q.starts_with("/callback") {
            let _ = write_html(&mut stream, 404, "<p>Not found</p>").await;
            continue;
        }

        let query = path_q.split_once('?').map(|(_, q)| q).unwrap_or("");
        let mut token = None;
        let mut got_state = None;
        for pair in query.split('&') {
            let Some((k, v)) = pair.split_once('=') else {
                continue;
            };
            let decoded = urlencoding::decode(v)
                .map(|c| c.into_owned())
                .unwrap_or_else(|_| v.to_string());
            match k {
                "token" => token = Some(decoded),
                "state" => got_state = Some(decoded),
                _ => {}
            }
        }

        if got_state.as_deref() != Some(expected_state) {
            let _ = write_html(
                &mut stream,
                400,
                "<p>连接校验失败，请关闭此页并在 Stitch 中重试。</p>",
            )
            .await;
            continue;
        }
        let Some(token) = token.filter(|t| t.starts_with("ps_") && t.len() > 10) else {
            let _ = write_html(
                &mut stream,
                400,
                "<p>未收到有效凭证，请关闭此页并在 Stitch 中重试。</p>",
            )
            .await;
            continue;
        };

        let _ = write_html(
            &mut stream,
            200,
            "<!DOCTYPE html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><title>已连接</title></head><body style=\"font-family:system-ui,sans-serif;padding:2rem;\"><p>账号已连接。可以关闭此页，返回 Stitch。</p></body></html>",
        )
        .await;
        return Ok(token);
    }
}

async fn write_html(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        408 => "Request Timeout",
        _ => "Error",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(resp.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let _ = stream.shutdown().await;
    Ok(())
}
