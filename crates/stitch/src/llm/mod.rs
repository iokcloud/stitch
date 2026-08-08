//! LLM provider abstraction.
//!
//! Supports multiple LLM backends with a unified streaming interface.
//! - OpenAI Chat Completions API（默认）
//! - OpenAI Responses API（`responses` 子模块，官方 OpenAI + DeepSeek V4 Flash 自动路由）

pub mod responses;
pub mod stream;
pub mod vision;

pub use responses::{LlmProtocol, resolve_protocol};

use crate::session::Message;
use std::sync::Mutex;

/// A token emitted during streaming generation.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A text token from the model.
    Token(String),
    /// 思考过程 token（/think on 时 DeepSeek V4 reasoning_content 流）。
    Thinking(String),
    /// A complete tool call request (accumulated from streaming deltas).
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// 服务端真实 usage（流末尾 usage chunk / Responses completed 事件）。
    /// 字段缺失时用 0——调用方应回退到本地估算。
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        /// 缓存命中的输入 token（DeepSeek prompt_cache_hit_tokens / OpenAI cached_tokens）。
        cache_hit_tokens: u64,
        cache_miss_tokens: u64,
    },
    /// The model has finished its response.
    Done,
    /// An error occurred during streaming.
    Error(String),
}

/// 解析流末尾 usage chunk（OpenAI / DeepSeek chat 流式：`choices` 空 + `usage`）。
/// 返回 `None` 表示该 chunk 无 usage（普通增量或格式不符）。
fn parse_stream_usage(chunk: &ChatStreamChunk) -> Option<StreamEvent> {
    let usage = chunk.usage.as_ref()?;
    let input = usage.prompt_tokens.unwrap_or(0);
    let output = usage.completion_tokens.unwrap_or(0);
    let cache_hit = usage.prompt_cache_hit_tokens.unwrap_or(0);
    let cache_miss = usage.prompt_cache_miss_tokens.unwrap_or(0);
    Some(StreamEvent::Usage {
        input_tokens: input,
        output_tokens: output,
        cache_hit_tokens: cache_hit,
        cache_miss_tokens: cache_miss,
    })
}

/// API 错误 → 用户可理解的中文说明（替代原始 status+body）。
/// 分类依据：4xx 客户端错误（密钥/模型/限流）与 5xx/网络（可重试）。
pub fn classify_api_error(status: u16, body: &str) -> String {
    let body_hint = body.trim().chars().take(60).collect::<String>();
    match status {
        401 | 403 => format!(
            "模型密钥无效或已过期（{status}）。请在设置 → 模型中检查密钥是否正确。{body_hint}"
        ),
        404 => format!(
            "模型不存在或服务不支持该模型名（{status}）。请在设置 → 模型中更新模型名称。{body_hint}"
        ),
        429 => format!(
            "模型服务限流（{status}，请求过多），已自动重试仍失败。请稍等片刻再试。{body_hint}"
        ),
        400 => format!("模型服务拒绝了请求（{status}）。{body_hint}"),
        500..=599 => {
            format!("模型服务暂时不可用（{status}），已自动重试仍失败。请稍后再试。{body_hint}")
        }
        _ => format!("模型服务返回异常（{status}）。{body_hint}"),
    }
}

/// 网络层错误 → 用户可理解的中文说明。
pub fn classify_network_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "连接模型服务超时，请检查网络或服务地址。".to_string()
    } else if e.is_connect() {
        "无法连接模型服务，请检查网络或服务地址。".to_string()
    } else if e.is_request() {
        format!("发送请求失败：{e}")
    } else {
        format!("请求异常：{e}")
    }
}

/// 是否应重试（429 限流 + 5xx + 网络类；4xx 其余不重试）。
pub(crate) fn is_retryable_status(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// Provider name.
/// Configuration for an LLM request.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub api_base: String,
    pub model: String,
    pub api_key: String,
    pub messages: Vec<Message>,
    pub max_tokens: usize,
    /// Tool definitions for function calling (OpenAI format).
    pub tools: Option<Vec<serde_json::Value>>,
    /// 会话级采样参数覆盖（--model-config）：发送处合并，None = 用模型默认。
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

/// 合并会话级采样覆盖（--model-config）与请求默认：返回
/// (temperature, top_p, max_tokens)——覆盖存在时优先，否则请求默认。
fn merge_sampling(request: &LlmRequest) -> (Option<f32>, Option<f32>, usize) {
    match crate::session_settings::model_overrides() {
        Some(o) => (
            o.temperature,
            o.top_p,
            o.max_tokens.unwrap_or(request.max_tokens),
        ),
        None => (None, None, request.max_tokens),
    }
}

// ── OpenAI API types ──────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a [serde_json::Value]>,
    /// DeepSeek V4: thinking is a request flag (not a separate model name).
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
}

#[derive(Serialize, Clone, Copy)]
struct ThinkingMode {
    #[serde(rename = "type")]
    kind: &'static str,
}

/// 思考过程开关（/think on|off，进程级单例；默认关——agent 工具循环保持可预测）。
static THINKING_ENABLED: std::sync::LazyLock<Mutex<bool>> =
    std::sync::LazyLock::new(|| Mutex::new(false));

/// 设置思考开关（CLI /think 会话内切换；测试隔离用 clear 配合串行锁）。
pub fn set_thinking(enabled: bool) {
    if let Ok(mut g) = THINKING_ENABLED.lock() {
        *g = enabled;
    }
}

/// 思考开关只读快照。
pub fn thinking_enabled() -> bool {
    THINKING_ENABLED.lock().map(|g| *g).unwrap_or(false)
}

fn deepseek_thinking_for(api_base: &str, model: &str) -> Option<ThinkingMode> {
    let base = api_base.to_ascii_lowercase();
    if !base.contains("deepseek.com") && !model.starts_with("deepseek-") {
        return None;
    }
    // V4 defaults thinking on; agent/tool loops stay predictable with it off
    // (until the user explicitly turns it on via /think).
    Some(ThinkingMode {
        kind: if thinking_enabled() {
            "enabled"
        } else {
            "disabled"
        },
    })
}

#[derive(Deserialize)]
struct ChatStreamChunk {
    #[serde(default)]
    choices: Vec<ChoiceDelta>,
    /// 流末尾 usage（DeepSeek: prompt_cache_hit_tokens / prompt_cache_miss_tokens）。
    #[serde(default)]
    usage: Option<StreamUsage>,
}

#[derive(Deserialize)]
struct StreamUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    /// DeepSeek 缓存字段（OpenAI 是 prompt_tokens_details.cached_tokens，见下）。
    #[serde(default)]
    prompt_cache_hit_tokens: Option<u64>,
    #[serde(default)]
    prompt_cache_miss_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct ChoiceDelta {
    delta: DeltaContent,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct DeltaContent {
    #[serde(default)]
    content: Option<String>,
    /// DeepSeek V4 thinking 模式：reasoning_content 单独成流（与 content 互斥）。
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Deserialize)]
struct ToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
pub struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

use serde::{Deserialize, Serialize};

/// Send a chat completion request and stream the response.
///
/// This is the main entry point. It handles the HTTP request,
/// SSE parsing, and tool call accumulation, emitting `StreamEvent`s
/// through the provided channel. Retries transient network errors
/// up to 2 times.
/// 共享 HTTP client（连接池/TLS 会话复用——ReAct 每迭代至少一次
/// LLM 调用，逐次 Client::new() 会丢全部连接复用）。
pub(crate) fn shared_http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("http client build")
    })
}

/// 发送 JSON POST 并重试（Chat / Responses 两协议共用）。
/// 429 尊重 Retry-After 头，5xx/网络按 1s/2s 退避，最多 2 次；
/// 其余 4xx 立即以用户可理解的中文报错（有 tx 时发 `StreamEvent::Error`）。
pub(crate) async fn send_with_retry(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: &serde_json::Value,
    tx: Option<&tokio::sync::mpsc::UnboundedSender<StreamEvent>>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<reqwest::Response> {
    let max_retries: u32 = 2;
    let mut last_error: Option<anyhow::Error> = None;
    for attempt in 0..=max_retries {
        let mut builder = client
            .post(url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(body);
        if let Some(secs) = timeout_secs {
            builder = builder.timeout(std::time::Duration::from_secs(secs));
        }
        match builder.send().await {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    let retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.trim().parse::<u64>().ok());
                    let body_text = response.text().await.unwrap_or_default();
                    if is_retryable_status(status.as_u16()) {
                        last_error = Some(anyhow::anyhow!(
                            "{}",
                            classify_api_error(status.as_u16(), &body_text)
                        ));
                        if attempt < max_retries {
                            let wait = retry_after
                                .map(std::time::Duration::from_secs)
                                .unwrap_or_else(|| {
                                    std::time::Duration::from_millis(1000 * (attempt as u64 + 1))
                                });
                            tracing::warn!(attempt, wait_ms = wait.as_millis(), "retrying");
                            tokio::time::sleep(wait).await;
                        }
                        continue;
                    }
                    let friendly = classify_api_error(status.as_u16(), &body_text);
                    if let Some(tx) = tx {
                        let _ = tx.send(StreamEvent::Error(friendly.clone()));
                    }
                    anyhow::bail!("{friendly}");
                }
                return Ok(response);
            }
            Err(e) => {
                if e.is_timeout() || e.is_connect() || e.is_request() {
                    last_error = Some(anyhow::anyhow!("{}", classify_network_error(&e)));
                    if attempt < max_retries {
                        tracing::warn!(attempt, "retrying after network error");
                        tokio::time::sleep(std::time::Duration::from_millis(
                            1000 * (attempt as u64 + 1),
                        ))
                        .await;
                    }
                    continue;
                }
                let friendly = format!("请求异常：{e}");
                if let Some(tx) = tx {
                    let _ = tx.send(StreamEvent::Error(friendly.clone()));
                }
                anyhow::bail!("{friendly}");
            }
        }
    }

    let err = last_error.unwrap_or_else(|| anyhow::anyhow!("未知错误"));
    if let Some(tx) = tx {
        let _ = tx.send(StreamEvent::Error(err.to_string()));
    }
    anyhow::bail!("{err:#}")
}

pub async fn stream_chat(
    request: LlmRequest,
    tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
) -> anyhow::Result<()> {
    let resolved_model =
        crate::config::migrate_llm_model(&request.model).unwrap_or(request.model.as_str());
    // 协议路由：官方 OpenAI + DeepSeek V4 Flash 走 Responses API
    if resolve_protocol(&request.api_base, resolved_model) == LlmProtocol::Responses {
        return responses::stream_responses(request, tx).await;
    }

    let url = format!(
        "{}/chat/completions",
        request.api_base.trim_end_matches('/')
    );
    let thinking = deepseek_thinking_for(&request.api_base, &request.model);
    let (temperature, top_p, max_tokens) = merge_sampling(&request);
    let chat_req = ChatRequest {
        model: resolved_model,
        messages: &request.messages,
        stream: true,
        max_tokens: Some(max_tokens),
        tools: request.tools.as_deref(),
        thinking,
        temperature,
        top_p,
    };

    tracing::debug!(%url, model = %request.model, msg_count = request.messages.len(), "sending chat request");

    let body = serde_json::to_value(&chat_req)?;
    let client = shared_http_client();
    let response = send_with_retry(client, &url, &request.api_key, &body, Some(&tx), None).await?;
    process_sse_stream(response, tx).await
}

/// Non-streaming chat completion — used for context condensation (no tools).
pub async fn complete_chat(request: LlmRequest) -> anyhow::Result<String> {
    let resolved_model =
        crate::config::migrate_llm_model(&request.model).unwrap_or(request.model.as_str());
    // 协议路由：官方 OpenAI + DeepSeek V4 Flash 走 Responses API
    if resolve_protocol(&request.api_base, resolved_model) == LlmProtocol::Responses {
        return responses::complete_responses(request).await;
    }

    let url = format!(
        "{}/chat/completions",
        request.api_base.trim_end_matches('/')
    );
    let thinking = deepseek_thinking_for(&request.api_base, &request.model);
    let (temperature, top_p, max_tokens) = merge_sampling(&request);
    let chat_req = ChatRequest {
        model: resolved_model,
        messages: &request.messages,
        stream: false,
        max_tokens: Some(max_tokens),
        tools: None,
        thinking,
        temperature,
        top_p,
    };

    let body = serde_json::to_value(&chat_req)?;
    let client = shared_http_client();
    let response = send_with_retry(client, &url, &request.api_key, &body, None, Some(45)).await?;
    let body = response.text().await.unwrap_or_default();

    let parsed: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("invalid completion JSON: {e}; body={body}"))?;
    let content = parsed
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if content.is_empty() {
        anyhow::bail!("empty completion content");
    }
    Ok(content)
}

/// Process the SSE stream from the LLM response.
///
/// Bytes are framed into lines before UTF-8 decode so multi-byte characters
/// split across TCP chunks are never lossy-replaced (U+FFFD).
async fn process_sse_stream(
    response: reqwest::Response,
    tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
) -> anyhow::Result<()> {
    let mut byte_stream = response.bytes_stream();
    let mut line_buf = stream::SseLineBuffer::new();
    let mut tool_calls_acc: Vec<stream::PendingToolCall> = Vec::new();

    use futures_util::StreamExt;
    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk?;
        if let Err(e) = line_buf.push(&chunk) {
            let _ = tx.send(StreamEvent::Error(e.clone()));
            anyhow::bail!("{e}");
        }

        for line in line_buf.drain_lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };

            if data == "[DONE]" {
                // Some providers end the stream without finish_reason — flush tools.
                for pending in tool_calls_acc.drain(..) {
                    if let Some(tc) = pending.finalize() {
                        let _ = tx.send(StreamEvent::ToolCall {
                            id: tc.id,
                            name: tc.name,
                            arguments: tc.arguments,
                        });
                    }
                }
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }

            match serde_json::from_str::<ChatStreamChunk>(data) {
                Ok(chunk) => {
                    // 流末尾 usage chunk（choices 通常为空）——成本/缓存统计用
                    if let Some(usage_ev) = parse_stream_usage(&chunk) {
                        let _ = tx.send(usage_ev);
                    }
                    for choice in chunk.choices {
                        let delta = choice.delta;

                        // Handle text content
                        if let Some(content) = delta.content
                            && !content.is_empty()
                        {
                            let _ = tx.send(StreamEvent::Token(content));
                        }

                        // Handle thinking content (/think on; reasoning_content 流)
                        if let Some(reasoning) = delta.reasoning_content
                            && !reasoning.is_empty()
                        {
                            let _ = tx.send(StreamEvent::Thinking(reasoning));
                        }

                        // Handle tool call deltas
                        if let Some(tc_deltas) = delta.tool_calls {
                            for tc in tc_deltas {
                                stream::accumulate_tool_call(
                                    &mut tool_calls_acc,
                                    tc.index,
                                    tc.id,
                                    tc.function,
                                );
                            }
                        }

                        // Check for finish
                        if let Some(ref reason) = choice.finish_reason
                            && (reason == "tool_calls" || reason == "stop")
                        {
                            // Flush any accumulated tool calls
                            for pending in tool_calls_acc.drain(..) {
                                if let Some(tc) = pending.finalize() {
                                    let _ = tx.send(StreamEvent::ToolCall {
                                        id: tc.id,
                                        name: tc.name,
                                        arguments: tc.arguments,
                                    });
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        %data, error = %e,
                        "failed to parse SSE chunk"
                    );
                }
            }
        }
    }

    // Trailing bytes without newline are unusual; still try to parse as a line.
    if let Some(line) = line_buf.flush_remainder() {
        let line = line.trim();
        if let Some(data) = line.strip_prefix("data: ")
            && data != "[DONE]"
            && let Ok(chunk) = serde_json::from_str::<ChatStreamChunk>(data)
        {
            if let Some(usage_ev) = parse_stream_usage(&chunk) {
                let _ = tx.send(usage_ev);
            }
            for choice in chunk.choices {
                if let Some(content) = choice.delta.content
                    && !content.is_empty()
                {
                    let _ = tx.send(StreamEvent::Token(content));
                }
                if let Some(reasoning) = choice.delta.reasoning_content
                    && !reasoning.is_empty()
                {
                    let _ = tx.send(StreamEvent::Thinking(reasoning));
                }
            }
        }
    }

    for pending in tool_calls_acc.drain(..) {
        if let Some(tc) = pending.finalize() {
            let _ = tx.send(StreamEvent::ToolCall {
                id: tc.id,
                name: tc.name,
                arguments: tc.arguments,
            });
        }
    }
    let _ = tx.send(StreamEvent::Done);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ChatStreamChunk, Mutex, classify_api_error, deepseek_thinking_for, is_retryable_status,
        set_thinking, thinking_enabled,
    };

    /// 思考开关是进程级单例：测试串行（permission.rs 同模式）。
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn thinking_switch_defaults_off_and_toggles() {
        let _g = lock();
        set_thinking(false);
        assert!(!thinking_enabled());
        // deepseek 请求标志跟随开关
        assert_eq!(
            deepseek_thinking_for("https://api.deepseek.com", "deepseek-v4-flash")
                .expect("deepseek 应注入 thinking 标志")
                .kind,
            "disabled"
        );
        set_thinking(true);
        assert!(thinking_enabled());
        assert_eq!(
            deepseek_thinking_for("https://api.deepseek.com", "deepseek-v4-flash")
                .expect("deepseek 应注入 thinking 标志")
                .kind,
            "enabled"
        );
        // 非 deepseek 不注入 thinking 标志
        assert!(deepseek_thinking_for("https://example.com", "gpt-4o").is_none());
        set_thinking(false);
    }

    #[test]
    fn chat_delta_parses_reasoning_content() {
        // DeepSeek V4 thinking 模式：reasoning_content 独立字段
        let chunk: ChatStreamChunk =
            serde_json::from_str(r#"{"choices":[{"delta":{"reasoning_content":"先看代码"}}]}"#)
                .expect("parse");
        let delta = &chunk.choices[0].delta;
        assert_eq!(delta.reasoning_content.as_deref(), Some("先看代码"));
        assert_eq!(delta.content.as_deref(), None);
    }

    #[test]
    fn auth_error_friendly_and_not_retryable() {
        assert!(classify_api_error(401, "invalid key").contains("密钥无效"));
        assert!(!is_retryable_status(401));
        assert!(!is_retryable_status(404));
    }

    #[test]
    fn rate_limit_is_retryable() {
        assert!(is_retryable_status(429));
        assert!(classify_api_error(429, "").contains("限流"));
    }

    #[test]
    fn server_error_retryable_and_friendly() {
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(503));
        assert!(classify_api_error(503, "").contains("暂时不可用"));
    }

    #[test]
    fn model_not_found_hints_config() {
        assert!(classify_api_error(404, "").contains("更新模型"));
    }
}
