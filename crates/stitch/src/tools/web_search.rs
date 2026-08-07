//! Web search tool.
//!
//! Searches the web via DuckDuckGo HTML endpoint first, falling back to
//! Bing (cn.bing.com — reachable from mainland China) when DDG is
//! unreachable or returns nothing. No API key needed. Returns top results
//! with title, URL and snippet — so the agent can answer questions that
//! need current, up-to-date information.

use super::{ToolDef, ToolResult};
use regex::Regex;
use std::sync::OnceLock;

/// Max results to return.
const MAX_RESULTS: usize = 8;

/// Request timeout in seconds.
const TIMEOUT_SECS: u64 = 15;

#[derive(Clone)]
pub struct WebSearch;

impl Default for WebSearch {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSearch {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "web_search".into(),
            description: "Search the web for current information (news, docs, releases, \
                 anything beyond your training data). Returns top results with title, URL \
                 and snippet. Pair with web_fetch to read a full page."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum results to return (default 5, max 8)"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = arguments["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' argument"))?;
        let max = arguments["max_results"]
            .as_i64()
            .unwrap_or(5)
            .clamp(1, MAX_RESULTS as i64) as usize;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
            // 搜索端点接受普通浏览器 UA
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36");
        let client = with_env_proxy(client).build()?;

        // 先 DDG，失败/无结果再退 Bing（国内可达）
        let mut last_err = String::new();
        for (url, parser) in [
            (
                format!(
                    "https://html.duckduckgo.com/html/?q={}",
                    percent_encode(query)
                ),
                parse_results as fn(&str, usize) -> Vec<String>,
            ),
            (
                format!("https://cn.bing.com/search?q={}", percent_encode(query)),
                parse_bing_results,
            ),
        ] {
            let body = match fetch_body(&client, &url).await {
                Ok(b) => b,
                Err(e) => {
                    last_err = e;
                    continue;
                }
            };
            let results = parser(&body, max);
            if !results.is_empty() {
                return Ok(ToolResult::ok(format!(
                    "Search results for \"{query}\":\n\n{}",
                    results.join("\n\n")
                )));
            }
        }
        if last_err.is_empty() {
            Ok(ToolResult::fail(format!("No results found for: {query}")))
        } else {
            Ok(ToolResult::fail(format!("Search failed: {last_err}")))
        }
    }
}

/// 应用环境变量代理（HTTPS_PROXY/HTTP_PROXY，大写优先、小写兜底，
/// 空值忽略、解析失败忽略）——reqwest 默认不读系统代理，
/// 国内用户开代理后联网工具即通。
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

/// GET + 状态检查 + 取文本；失败返回错误描述。
async fn fetch_body(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.text().await.map_err(|e| e.to_string())
}

/// 解析 DDG HTML 结果页：标题 + 真实 URL + 摘要。
fn parse_results(body: &str, max: usize) -> Vec<String> {
    let flat = body.replace('\n', " ");
    let link_re = link_regex();
    let snip_re = snippet_regex();
    let snips: Vec<String> = snip_re
        .captures_iter(&flat)
        .map(|c| strip_tags(&c[1]).trim().to_string())
        .collect();

    let mut out = Vec::new();
    for (i, cap) in link_re.captures_iter(&flat).enumerate() {
        if out.len() >= max {
            break;
        }
        let mut href = percent_decode(&cap[1]);
        if href.starts_with("//") {
            href = format!("https:{href}");
        }
        // DDG 跳转链接：//duckduckgo.com/l/?uddg=<urlencoded>&rut=...
        if let Some(pos) = href.find("uddg=") {
            let rest = &href[pos + 5..];
            let enc = rest.split('&').next().unwrap_or(rest);
            href = percent_decode(enc);
        }
        let title = strip_tags(&cap[2]).trim().to_string();
        let snippet = snips.get(i).cloned().unwrap_or_default();
        out.push(format!(
            "{}. {title}\n   {href}\n   {snippet}",
            out.len() + 1
        ));
    }
    out
}

/// 解析 Bing 结果页：`<li class="b_algo">` 块内的标题链接 + 摘要。
fn parse_bing_results(body: &str, max: usize) -> Vec<String> {
    static LI: OnceLock<Regex> = OnceLock::new();
    static LINK: OnceLock<Regex> = OnceLock::new();
    static SNIP: OnceLock<Regex> = OnceLock::new();
    let li_re =
        LI.get_or_init(|| Regex::new(r#"(?is)<li class="b_algo".*?</li>"#).expect("bing li regex"));
    let link_re = LINK.get_or_init(|| {
        Regex::new(r#"(?is)<a[^>]*href="(https?://[^"]+)"[^>]*>(.*?)</a>"#)
            .expect("bing link regex")
    });
    let snip_re = SNIP.get_or_init(|| {
        Regex::new(r#"(?is)<div class="b_caption".*?<p[^>]*>(.*?)</p>"#)
            .expect("bing snippet regex")
    });

    let mut out = Vec::new();
    for li in li_re.find_iter(body).take(max).map(|m| m.as_str()) {
        let (href, title) = link_re
            .captures(li)
            .map(|c| {
                (
                    strip_tags(&c[1]).to_string(),
                    strip_tags(&c[2]).trim().to_string(),
                )
            })
            .unwrap_or_default();
        if href.is_empty() || title.is_empty() {
            continue;
        }
        let snippet = snip_re
            .captures(li)
            .map(|c| strip_tags(&c[1]).trim().to_string())
            .unwrap_or_default();
        out.push(format!(
            "{}. {title}\n   {href}\n   {snippet}",
            out.len() + 1
        ));
    }
    out
}

fn link_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
            .expect("link regex")
    })
}

fn snippet_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"<a[^>]*class="[^"]*result__snippet[^"]*"[^>]*>(.*?)</a>"#)
            .expect("snippet regex")
    })
}

/// 去 HTML 标签 + 解码常见实体 + 折叠连续空白
/// （标题内嵌 span/strong 时去标签会留双空格，折叠后模型读到干净文本）。
fn strip_tags(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"<[^>]*>").expect("tag regex"));
    let no_tags = re.replace_all(s, " ").to_string();
    let decoded = no_tags
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// RFC 3986 保留字符之外的字符 percent-encode（查询参数用）。
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 百分号解码（`+` 视为空格）。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_roundtrip() {
        let enc = percent_encode("rust 网络 教程 abc-_.~");
        assert!(enc.contains("rust"));
        assert!(enc.contains("%20") || enc.contains("%E7")); // 空格或中文编码
        let dec = percent_decode(&enc);
        assert!(dec.contains("rust"));
        assert!(dec.contains("教程"));
        // 空格编码为 %20（percent_encode 不产生 +）
        assert_eq!(percent_decode("%E7%BD%91"), "网");
        assert_eq!(percent_decode("a+b"), "a b");
    }

    #[test]
    fn parses_ddg_results() {
        let html = r#"
<div class="result results_links results_links_deep web-result">
  <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&amp;rut=x">Example Page Title</a>
  <a class="result__snippet" href="//duckduckgo.com/l/?uddg=...">Some snippet &amp; text here</a>
</div>
"#;
        let results = parse_results(html, 5);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("Example Page Title"));
        assert!(results[0].contains("https://example.com/page"));
        assert!(results[0].contains("Some snippet & text here"));
    }

    #[test]
    fn parses_bing_results() {
        let html = r#"
<ol id="b_results">
  <li class="b_algo">
    <h2><a href="https://example.com/rust" h="ID=SERP,1">Rust <strong>Lang</strong> 官网</a></h2>
    <div class="b_caption"><p>A systems programming language — 摘要文本。</p></div>
  </li>
  <li class="b_algo">
    <h2><a href="https://example.com/rust-book" h="ID=SERP,2">Rust 书</a></h2>
    <div class="b_caption"><p>学习资源汇总。</p></div>
  </li>
</ol>"#;
        let results = parse_bing_results(html, 5);
        assert_eq!(results.len(), 2);
        assert!(results[0].contains("Rust Lang 官网"));
        assert!(results[0].contains("https://example.com/rust"));
        assert!(results[0].contains("摘要文本"));
        // max 生效
        assert_eq!(parse_bing_results(html, 1).len(), 1);
        // 无高亮标签残留
        assert!(!results[0].contains("<strong>"));
    }

    #[test]
    fn parse_empty_page_yields_nothing() {
        assert!(parse_results("<html><body>no results</body></html>", 5).is_empty());
    }

    #[test]
    fn parse_respects_max() {
        let mut html = String::new();
        for i in 0..10 {
            html.push_str(&format!(
                r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2F{i}">R{i}</a>"#
            ));
        }
        assert_eq!(parse_results(&html, 3).len(), 3);
        assert_eq!(parse_results(&html, MAX_RESULTS).len(), 8);
    }
}
