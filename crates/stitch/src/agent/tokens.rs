//! Token estimation and context-window helpers.
//!
//! Stitch estimates token counts from text (streaming APIs often omit usage).
//! Heuristic: CJK ≈ 1 token/char; Latin ≈ 4 chars/token. Conservative for
//! compaction triggers.

use crate::session::Message;

/// Accumulated token usage for a generation / session turn.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TokenUsage {
    /// Estimated input tokens sent to the model (sum across iterations).
    pub input_tokens: usize,
    /// Estimated output tokens received from the model.
    pub output_tokens: usize,
    /// 服务端真实缓存命中输入 token（API usage；无则 0，展示时回退估算）。
    pub cache_hit_tokens: u64,
    /// 服务端真实缓存未命中输入 token（API usage）。
    pub cache_miss_tokens: u64,
}

impl TokenUsage {
    pub fn total(&self) -> usize {
        self.input_tokens + self.output_tokens
    }

    /// 缓存命中率（0-100；无真实 usage 时返回 None）。
    pub fn cache_hit_pct(&self) -> Option<u8> {
        let total = self.cache_hit_tokens + self.cache_miss_tokens;
        if total == 0 {
            return None;
        }
        Some(((self.cache_hit_tokens.saturating_mul(100)) / total).min(100) as u8)
    }
}

/// 每百万 token 价格（元/百万）：(输入命中, 输入未命中, 输出)。
/// 近似表——官方价格变化后此处维护；未知模型用通用默认。
fn price_per_million(model: &str) -> (f64, f64, f64) {
    let m = model.to_ascii_lowercase();
    if m.contains("deepseek") {
        // DeepSeek V4 系：缓存命中约 0.1 折（官方 2.5 折优惠期后近似）
        (0.05, 0.5, 2.0)
    } else if m.contains("gpt-5") || m.contains("o3") || m.contains("o4") {
        // OpenAI 前沿系：输入 $1.25/M ≈ ¥9，缓存 $0.125/M ≈ ¥0.9，输出 $10/M ≈ ¥72
        (0.9, 9.0, 72.0)
    } else if m.contains("gpt-4") {
        (0.72, 7.2, 21.6)
    } else {
        // 通用默认（近似 ¥2/M 输入 · ¥12/M 输出 · 命中 0.1 折）
        (0.2, 2.0, 12.0)
    }
}

/// 回合成本估算（人民币，元）。基于真实 usage（缓存命中/未命中分开计价）；
/// 无真实 usage 时按估算 token 全量未命中计价。
pub fn estimate_cost(usage: &TokenUsage, model: &str) -> f64 {
    let (hit_price, miss_price, out_price) = price_per_million(model);
    let (hit, miss) = if usage.cache_hit_tokens + usage.cache_miss_tokens > 0 {
        (
            usage.cache_hit_tokens as f64,
            usage.cache_miss_tokens as f64,
        )
    } else {
        (0.0, usage.input_tokens as f64)
    };
    (hit * hit_price + miss * miss_price + usage.output_tokens as f64 * out_price) / 1_000_000.0
}

/// Snapshot for UI: turn totals + live context fill.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UsageSnapshot {
    pub input_tokens: usize,
    pub output_tokens: usize,
    /// Estimated tokens currently in the session message list (context window fill).
    pub context_tokens: usize,
    pub context_limit: usize,
    pub iteration: usize,
    pub compacted: bool,
    /// Per-tier breakdown when layering is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<super::layers::LayerStats>,
}

impl UsageSnapshot {
    pub fn context_pct(&self) -> u8 {
        if self.context_limit == 0 {
            return 0;
        }
        let pct = (self.context_tokens.saturating_mul(100)) / self.context_limit;
        pct.min(100) as u8
    }
}

/// Rough token estimation for a character count (ASCII-biased). Prefer [`estimate_text`].
pub fn estimate(chars: usize) -> usize {
    if chars == 0 {
        return 0;
    }
    chars.saturating_div(4).max(1)
}

pub fn is_cjk(ch: char) -> bool {
    matches!(
        ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified
        | '\u{3400}'..='\u{4DBF}' // Extension A
        | '\u{F900}'..='\u{FAFF}' // Compatibility
        | '\u{3000}'..='\u{303F}' // CJK punctuation
        | '\u{FF00}'..='\u{FFEF}' // Fullwidth
    )
}

/// Estimate tokens for a string (CJK-aware).
pub fn estimate_text(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in s.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if is_cjk(ch) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    let latin = if other == 0 {
        0
    } else {
        other.div_ceil(4).max(1)
    };
    (cjk + latin).max(1)
}

/// Estimated tokens for one image data URL (bounded, monotonic in size).
/// `85` matches the OpenAI low-detail floor; the base64 length is a rough
/// proxy for decoded pixels.
pub fn estimate_image_url(url: &str) -> usize {
    if url.is_empty() {
        return 0;
    }
    let data = url.split_once(',').map(|(_, d)| d).unwrap_or(url);
    (IMAGE_TOKEN_BASE + data.len() / 256).min(IMAGE_TOKEN_BASE + IMAGE_TOKEN_CAP)
}

/// Estimate token count for a slice of messages (content + tool call args).
pub fn estimate_messages(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| {
            let mut n = match &m.content {
                crate::session::Content::Text(s) => estimate_text(s),
                crate::session::Content::Parts(parts) => parts
                    .iter()
                    .map(|p| match p {
                        crate::session::ContentPart::Text { text } => estimate_text(text),
                        crate::session::ContentPart::ImageUrl { image_url } => {
                            estimate_image_url(&image_url.url)
                        }
                    })
                    .sum(),
            };
            if let Some(ref calls) = m.tool_calls {
                for tc in calls {
                    n += estimate_text(&tc.function.name);
                    n += estimate_text(&tc.function.arguments);
                }
            }
            n
        })
        .sum()
}

/// Whether a model id accepts image input (heuristic, mirrors the frontend's
/// `modelSupportsVision` in types.ts — keep both in sync).
pub fn model_supports_vision(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    if m.contains("deepseek") {
        return false;
    }
    m.contains("gpt-4o")
        || m.contains("gpt-4")
        || m.contains("claude")
        || m.contains("kimi")
        || m.contains("moonshot")
        || m.contains("qwen")
        || m.contains("glm-4v")
        || m.contains("gemini")
        || m.contains("vision")
}

/// OpenAI low-detail image floor; per-image token cap (bounded estimates).
const IMAGE_TOKEN_BASE: usize = 85;
const IMAGE_TOKEN_CAP: usize = 1024;

/// Default context window size for a model id (conservative).
pub fn context_limit_for_model(model: &str) -> usize {
    let m = model.to_ascii_lowercase();
    if m.contains("deepseek") {
        // deepseek-v4 / chat family — use a safe working budget under advertised max
        return 64_000;
    }
    if m.contains("gpt-4o") || m.contains("o1") || m.contains("o3") {
        return 128_000;
    }
    if m.contains("gpt-4") || m.contains("claude") {
        return 100_000;
    }
    if m.contains("32k") {
        return 32_000;
    }
    64_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_rounds_correctly() {
        assert_eq!(estimate(0), 0);
        assert_eq!(estimate(1), 1);
        assert_eq!(estimate(4), 1);
        assert_eq!(estimate(8), 2);
        assert_eq!(estimate(40), 10);
    }

    #[test]
    fn estimate_text_cjk_heavier() {
        let en = estimate_text("abcd"); // 1
        let zh = estimate_text("你好世界"); // 4
        assert!(zh > en);
        assert_eq!(zh, 4);
    }

    #[test]
    fn estimate_messages_sums() {
        let msgs = vec![
            Message {
                role: crate::session::Role::User,
                content: "Hello world!".into(),
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: crate::session::Role::Assistant,
                content: "Hi there!".into(),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        assert!(estimate_messages(&msgs) >= 2);
    }

    #[test]
    fn usage_total() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        };
        assert_eq!(usage.total(), 1500);
    }

    #[test]
    fn cache_hit_pct_and_cost() {
        let mut usage = TokenUsage {
            input_tokens: 10_000,
            output_tokens: 500,
            ..Default::default()
        };
        // 无真实 usage → None，成本按全量未命中估算
        assert_eq!(usage.cache_hit_pct(), None);
        let no_cache = estimate_cost(&usage, "deepseek-v4-flash");
        assert!(no_cache > 0.0);

        // 有真实 usage：8 成命中
        usage.cache_hit_tokens = 8_000;
        usage.cache_miss_tokens = 2_000;
        assert_eq!(usage.cache_hit_pct(), Some(80));
        let cached = estimate_cost(&usage, "deepseek-v4-flash");
        assert!(
            cached < no_cache,
            "缓存命中应显著降低成本（{cached} vs {no_cache}）"
        );
    }

    #[test]
    fn context_limit_deepseek() {
        assert_eq!(context_limit_for_model("deepseek-v4-flash"), 64_000);
    }

    #[test]
    fn context_pct() {
        let s = UsageSnapshot {
            input_tokens: 100,
            output_tokens: 50,
            context_tokens: 32_000,
            context_limit: 64_000,
            iteration: 1,
            compacted: false,
            layers: None,
        };
        assert_eq!(s.context_pct(), 50);
    }

    #[test]
    fn estimate_image_url_bounded_and_monotonic() {
        assert_eq!(estimate_image_url(""), 0);
        let small = estimate_image_url("data:image/png;base64,AAAA");
        assert_eq!(small, 85 + 4 / 256); // floor + tiny payload
        // Large payloads cap at base + cap.
        let big = "x".repeat(1_000_000);
        let url = format!("data:image/png;base64,{big}");
        assert_eq!(estimate_image_url(&url), 85 + 1024);
        // Monotonic in payload length (by 256-char steps).
        let longer = format!("data:image/png;base64,{}", "A".repeat(300));
        assert!(estimate_image_url(&longer) > small);
    }

    #[test]
    fn estimate_messages_counts_image_parts() {
        use crate::session::{Content, ContentPart, ImageUrl, Message, Role};
        let text_only = Message {
            role: Role::User,
            content: "hello world".into(),
            tool_calls: None,
            tool_call_id: None,
        };
        let with_image = Message {
            role: Role::User,
            content: Content::Parts(vec![
                ContentPart::Text {
                    text: "看这张图".into(),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: "data:image/png;base64,AAAA".into(),
                    },
                },
            ]),
            tool_calls: None,
            tool_call_id: None,
        };
        let n = estimate_messages(&[with_image.clone()]);
        assert!(n > estimate_messages(&[text_only]));
        assert!(n >= 85); // image floor included
    }

    #[test]
    fn model_supports_vision_heuristic() {
        assert!(!model_supports_vision("deepseek-v4-flash"));
        assert!(!model_supports_vision("deepseek-v4-pro"));
        assert!(model_supports_vision("gpt-4o"));
        assert!(model_supports_vision("gpt-4-turbo"));
        assert!(model_supports_vision("claude-sonnet-4"));
        assert!(model_supports_vision("kimi-k2.5"));
        assert!(model_supports_vision("qwen-vl-max"));
        assert!(model_supports_vision("gemini-2.5-pro"));
        assert!(!model_supports_vision("llama-3.2"));
        assert!(!model_supports_vision(""));
    }
}
