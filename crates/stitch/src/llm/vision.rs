//! Local vision describe layer.
//!
//! Text-only models (DeepSeek et al.) cannot receive image parts. When the
//! user attaches images, the desktop shell asks a **local** vision model
//! (Ollama qwen3-vl by default) to describe each image and merges the
//! descriptions into the user message as text — the remote model keeps doing
//! the reasoning, the local model acts as its eyes. Images never leave the
//! machine (the endpoint is user-configured, defaults to 127.0.0.1).

use crate::session::user_content_with_images;
use std::time::Duration;

/// Why describing one image failed — the caller decides the fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescribeFailure {
    /// Connection refused / request failed (service not running).
    Unreachable,
    Timeout,
    Http {
        status: u16,
        body: String,
    },
    Parse,
    /// `choices[0].message.content` missing or empty.
    Empty,
}

/// Prompt given to the local vision model. Short, factual, model-facing.
const DESCRIBE_PROMPT: &str =
    "用中文简要描述这张图片，供无视觉的对话模型理解。只说事实，不超过 80 字。";

/// Describe one image (data URL) via an OpenAI-compatible local endpoint.
pub async fn describe_image(
    api_base: &str,
    model: &str,
    data_url: &str,
    timeout: Duration,
) -> Result<String, DescribeFailure> {
    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));
    let content = user_content_with_images(DESCRIBE_PROMPT, &[data_url.to_string()]);
    let body = serde_json::json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": content,
        }],
        "stream": false,
        "max_tokens": 300,
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .timeout(timeout)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                DescribeFailure::Timeout
            } else {
                DescribeFailure::Unreachable
            }
        })?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(DescribeFailure::Http {
            status: status.as_u16(),
            body: text.chars().take(200).collect(),
        });
    }
    parse_description_response(&text)
}

/// Pure parse of a non-streaming chat completion response.
pub fn parse_description_response(body: &str) -> Result<String, DescribeFailure> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|_| DescribeFailure::Parse)?;
    let content = parsed
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .ok_or(DescribeFailure::Parse)?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(DescribeFailure::Empty);
    }
    Ok(trimmed.to_string())
}

/// Merge per-image descriptions into the user message as a leading block.
/// `descriptions[i]` is `None` when that image failed to describe — it gets
/// the `[图片]` placeholder instead.
pub fn compose_description_text(text: &str, descriptions: &[Option<String>]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (i, desc) in descriptions.iter().enumerate() {
        let label = if descriptions.len() > 1 {
            format!("[图片描述 {}：", i + 1)
        } else {
            "[图片描述：".to_string()
        };
        match desc {
            Some(d) => parts.push(format!("{label}{d}]")),
            None => parts.push(format!("{label}[图片]]")),
        }
    }
    let block = parts.join("\n");
    if text.trim().is_empty() {
        block
    } else {
        format!("{block}\n\n{text}")
    }
}

/// Strip `[图片描述…]` lines from a stored message so text comparisons
/// (`content_matches_user`, rewind) match against the user's original text.
pub fn strip_image_descriptions(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with("[图片描述"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn parse_happy_path() {
        let body = r#"{"choices":[{"message":{"content":"一张纯色图片"}}]}"#;
        assert_eq!(parse_description_response(body).unwrap(), "一张纯色图片");
    }

    #[test]
    fn parse_empty_content_is_empty_failure() {
        let body = r#"{"choices":[{"message":{"content":"   "}}]}"#;
        assert_eq!(
            parse_description_response(body),
            Err(DescribeFailure::Empty)
        );
    }

    #[test]
    fn parse_missing_choices_is_parse_failure() {
        assert_eq!(
            parse_description_response("{}"),
            Err(DescribeFailure::Parse)
        );
        assert_eq!(
            parse_description_response(r#"{"choices":[{"message":{}}]}"#),
            Err(DescribeFailure::Parse)
        );
        assert_eq!(
            parse_description_response("not json"),
            Err(DescribeFailure::Parse)
        );
    }

    #[test]
    fn compose_merges_descriptions_and_placeholders() {
        let out = compose_description_text("原文本", &[Some("红色背景".into()), None]);
        assert!(out.contains("[图片描述 1：红色背景]"));
        assert!(out.contains("[图片描述 2：[图片]]"));
        assert!(out.ends_with("原文本"));

        let single = compose_description_text("", &[Some("纯色".into())]);
        assert_eq!(single, "[图片描述：纯色]");
    }

    #[test]
    fn strip_removes_description_lines_only() {
        let stored = "[图片描述：红色背景]\n\n原文本\n[图片描述 2：[图片]]";
        let stripped = strip_image_descriptions(stored);
        assert_eq!(stripped, "原文本");
    }

    #[test]
    fn unreachable_connection_reports_unreachable() {
        // Bind a port then drop the listener → nothing listening.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(describe_image(
                &format!("http://127.0.0.1:{port}/v1"),
                "qwen3-vl:8b",
                "data:image/png;base64,AA",
                Duration::from_secs(3),
            ))
            .unwrap_err();
        assert_eq!(err, DescribeFailure::Unreachable);
    }

    #[test]
    fn timeout_reports_timeout() {
        // Accept and stall the connection past the client timeout.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = thread::spawn(move || {
            if let Ok((_s, _)) = listener.accept() {
                thread::sleep(Duration::from_millis(5000));
            }
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(describe_image(
                &format!("http://127.0.0.1:{port}/v1"),
                "qwen3-vl:8b",
                "data:image/png;base64,AA",
                Duration::from_millis(300),
            ))
            .unwrap_err();
        assert_eq!(err, DescribeFailure::Timeout);
        h.join().ok();
    }

    #[test]
    fn http_error_reports_status() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = thread::spawn(move || {
            use std::io::{Read, Write};
            if let Ok((mut s, _)) = listener.accept() {
                // Drain the request head so reqwest finishes sending its body.
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let _ = s.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
                );
            }
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(describe_image(
                &format!("http://127.0.0.1:{port}/v1"),
                "qwen3-vl:8b",
                "data:image/png;base64,AA",
                Duration::from_secs(3),
            ))
            .unwrap_err();
        assert!(matches!(err, DescribeFailure::Http { status: 404, .. }));
        h.join().ok();
    }
}
