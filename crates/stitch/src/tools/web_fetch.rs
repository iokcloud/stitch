//! Web fetch tool.
//!
//! Fetches content from a URL and returns text. HTML responses are
//! converted to plain text (tags stripped) so the model gets readable
//! content. Uses reqwest with timeouts and size limits for safety.

use super::{ToolDef, ToolResult};
use regex::Regex;
use std::sync::OnceLock;

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
            .user_agent("Stitch/0.1 (PromptStdio Agent)");
        let client = with_env_proxy(client).build()?;

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
        let raw = if truncated {
            String::from_utf8_lossy(&bytes[..MAX_RESPONSE_BYTES]).into_owned()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };

        // HTML 响应转纯文本（去 script/style/tags），模型直接读到正文
        let is_html = content_type.contains("text/html") || raw.trim_start().starts_with("<");
        let body = if is_html { html_to_text(&raw) } else { raw };

        let size_note = if truncated {
            format!(
                "\n\n[... truncated at {MAX_RESPONSE_BYTES} bytes, total {} bytes]",
                bytes.len()
            )
        } else {
            String::new()
        };
        Ok(ToolResult::ok(format!(
            "Status: {status}\nContent-Type: {content_type}\n\n{body}{size_note}"
        )))
    }
}

/// 应用环境变量代理（HTTPS_PROXY/HTTP_PROXY，大写优先、小写兜底，
/// 空值忽略、解析失败忽略）——reqwest 默认不读系统代理，
/// 国内用户开代理后联网工具即通。与 web_search 的 with_env_proxy 同源。
fn with_env_proxy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    let proxy = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .or_else(|_| std::env::var("HTTP_PROXY"))
        .or_else(|_| std::env::var("http_proxy"))
        .ok()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    match proxy {
        Some(p) => match reqwest::Proxy::all(&p) {
            Ok(proxy) => builder.proxy(proxy),
            Err(_) => builder,
        },
        None => builder,
    }
}

/// HTML 转纯文本：去 script/style 与标签、解码实体、折叠空白行。
fn html_to_text(html: &str) -> String {
    static RE_SCRIPT: OnceLock<Regex> = OnceLock::new();
    static RE_STYLE: OnceLock<Regex> = OnceLock::new();
    static RE_TAG: OnceLock<Regex> = OnceLock::new();
    let stripped = RE_SCRIPT
        .get_or_init(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").expect("script regex"))
        .replace_all(html, " ")
        .to_string();
    let stripped = RE_STYLE
        .get_or_init(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").expect("style regex"))
        .replace_all(&stripped, " ")
        .to_string();
    let text = RE_TAG
        .get_or_init(|| Regex::new(r"(?s)<[^>]*>").expect("tag regex"))
        .replace_all(&stripped, "\n")
        .to_string();
    let decoded = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    decoded
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_style_and_tags() {
        let html = r#"<html><head><style>body{color:red}</style></head>
<body><h1>标题</h1><p>正文内容</p>
<script>alert('x')</script></body></html>"#;
        let text = html_to_text(html);
        assert!(text.contains("标题"));
        assert!(text.contains("正文内容"));
        assert!(!text.contains("<h1>"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("color:red"));
    }

    #[test]
    fn decodes_entities() {
        let html = "<p>a &amp; b &lt; c &gt; d &quot;e&quot;</p>";
        let text = html_to_text(html);
        assert_eq!(text, "a & b < c > d \"e\"");
    }

    #[test]
    fn collapses_blank_lines() {
        let html = "<p>one</p>\n\n\n<div>two</div>";
        assert_eq!(html_to_text(html), "one\ntwo");
    }
}
