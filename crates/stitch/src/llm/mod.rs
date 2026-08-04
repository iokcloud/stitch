//! LLM provider abstraction.
//!
//! Supports multiple LLM backends with a unified streaming interface.
//! Currently implements OpenAI Chat Completions API with streaming.

pub mod stream;
pub mod vision;

use crate::session::Message;

/// A token emitted during streaming generation.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A text token from the model.
    Token(String),
    /// A complete tool call request (accumulated from streaming deltas).
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// The model has finished its response.
    Done,
    /// An error occurred during streaming.
    Error(String),
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
fn is_retryable_status(status: u16) -> bool {
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
}

#[derive(Serialize, Clone, Copy)]
struct ThinkingMode {
    #[serde(rename = "type")]
    kind: &'static str,
}

fn deepseek_thinking_for(api_base: &str, model: &str) -> Option<ThinkingMode> {
    let base = api_base.to_ascii_lowercase();
    if !base.contains("deepseek.com") && !model.starts_with("deepseek-") {
        return None;
    }
    // V4 defaults thinking on; agent/tool loops stay predictable with it off.
    let _ = model;
    Some(ThinkingMode { kind: "disabled" })
}

#[derive(Deserialize)]
struct ChatStreamChunk {
    choices: Vec<ChoiceDelta>,
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
fn shared_http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("http client build")
    })
}

pub async fn stream_chat(
    request: LlmRequest,
    tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
) -> anyhow::Result<()> {
    let url = format!(
        "{}/chat/completions",
        request.api_base.trim_end_matches('/')
    );

    let resolved_model =
        crate::config::migrate_llm_model(&request.model).unwrap_or(request.model.as_str());
    let thinking = deepseek_thinking_for(&request.api_base, &request.model);
    let chat_req = ChatRequest {
        model: resolved_model,
        messages: &request.messages,
        stream: true,
        max_tokens: Some(request.max_tokens),
        tools: request.tools.as_deref(),
        thinking,
    };

    tracing::debug!(%url, model = %request.model, msg_count = request.messages.len(), "sending chat request");

    // 重试策略：429 限流 + 5xx + 网络类错误最多重试 2 次；
    // 429 尊重 Retry-After 头，其余按 1s/2s 退避。4xx 其余（密钥/模型等）
    // 不重试——立即以用户可理解的中文报错。
    let max_retries: u32 = 2;
    let mut last_error = None;
    let client = shared_http_client();
    for attempt in 0..=max_retries {
        match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", request.api_key))
            .header("Content-Type", "application/json")
            .json(&chat_req)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    // 429 尊重 Retry-After 头（秒），其余按 1s/2s 退避
                    let retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.trim().parse::<u64>().ok());
                    let body = response.text().await.unwrap_or_default();
                    if is_retryable_status(status.as_u16()) {
                        last_error = Some(anyhow::anyhow!(
                            "{}",
                            classify_api_error(status.as_u16(), &body)
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
                    let friendly = classify_api_error(status.as_u16(), &body);
                    let _ = tx.send(StreamEvent::Error(friendly.clone()));
                    anyhow::bail!("{friendly}");
                }
                // Success — proceed to stream processing
                return process_sse_stream(response, tx).await;
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
                let _ = tx.send(StreamEvent::Error(friendly.clone()));
                anyhow::bail!("{friendly}");
            }
        }
    }

    let err = last_error.unwrap_or_else(|| anyhow::anyhow!("未知错误"));
    let _ = tx.send(StreamEvent::Error(err.to_string()));
    anyhow::bail!("{err:#}");
}

/// Non-streaming chat completion — used for context condensation (no tools).
pub async fn complete_chat(request: LlmRequest) -> anyhow::Result<String> {
    let url = format!(
        "{}/chat/completions",
        request.api_base.trim_end_matches('/')
    );
    let resolved_model =
        crate::config::migrate_llm_model(&request.model).unwrap_or(request.model.as_str());
    let thinking = deepseek_thinking_for(&request.api_base, &request.model);
    let chat_req = ChatRequest {
        model: resolved_model,
        messages: &request.messages,
        stream: false,
        max_tokens: Some(request.max_tokens),
        tools: None,
        thinking,
    };

    let client = shared_http_client();
    // 与 stream_chat 同策略：429/5xx/网络重试 2 次，其余立即友好报错。
    let max_retries: u32 = 2;
    let mut last_error = None;
    let mut response = None;
    for attempt in 0..=max_retries {
        if attempt > 0 {
            tracing::warn!(attempt, "complete_chat retrying after transient error");
            tokio::time::sleep(std::time::Duration::from_millis(1000 * attempt as u64)).await;
        }
        match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", request.api_key))
            .header("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(45))
            .json(&chat_req)
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    if is_retryable_status(status.as_u16()) {
                        last_error = Some(anyhow::anyhow!(
                            "{}",
                            classify_api_error(status.as_u16(), &body)
                        ));
                        continue;
                    }
                    anyhow::bail!("{}", classify_api_error(status.as_u16(), &body));
                }
                response = Some(resp);
                break;
            }
            Err(e) => {
                if e.is_timeout() || e.is_connect() || e.is_request() {
                    last_error = Some(anyhow::anyhow!("{}", classify_network_error(&e)));
                    continue;
                }
                anyhow::bail!("请求异常：{e}");
            }
        }
    }
    let response =
        response.ok_or_else(|| last_error.unwrap_or_else(|| anyhow::anyhow!("未知错误")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("{}", classify_api_error(status.as_u16(), &body));
    }

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
                    for choice in chunk.choices {
                        let delta = choice.delta;

                        // Handle text content
                        if let Some(content) = delta.content
                            && !content.is_empty()
                        {
                            let _ = tx.send(StreamEvent::Token(content));
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
            for choice in chunk.choices {
                if let Some(content) = choice.delta.content
                    && !content.is_empty()
                {
                    let _ = tx.send(StreamEvent::Token(content));
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
    use super::{classify_api_error, is_retryable_status};

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
