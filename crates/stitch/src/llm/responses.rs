//! OpenAI Responses API 后端（协议路由第二分支 · docs/LONG-HORIZON-CONTEXT-PLAN.md）。
//!
//! - 请求：`POST {api_base}/responses`，`input` items + 顶层 `instructions`
//! - 流式：语义事件（`response.output_text.delta` / `function_call_arguments` /
//!   `output_item.done` / `completed`），统一折叠回 [`super::StreamEvent`]，
//!   agent 层零改动
//! - 路由：官方 OpenAI 端点 + DeepSeek V4 Flash 自动走本后端；其余走 Chat Completions

#![allow(clippy::disallowed_methods)] // json! 宏展开内含 unwrap（与 web routes 同惯例）

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::session::{Content, ContentPart, Message, Role};

use super::stream::SseLineBuffer;
use super::{StreamEvent, send_with_retry, shared_http_client};

/// LLM 请求协议（路由判定在 llm 层内部，调用点零改动）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProtocol {
    Chat,
    Responses,
}

/// 自动路由：官方 OpenAI（api.openai.com）与 DeepSeek V4 Flash（原生支持
/// Responses，2026-07-31 起）走 Responses；其余（Ollama/本地/第三方代理/
/// DeepSeek 其他模型）走 Chat Completions。
/// `model` 应为 `migrate_llm_model` 解析后的模型名。
pub fn resolve_protocol(api_base: &str, model: &str) -> LlmProtocol {
    let base = api_base.to_ascii_lowercase();
    if base.contains("openai.com") {
        return LlmProtocol::Responses;
    }
    if base.contains("deepseek.com") && model.to_ascii_lowercase().contains("v4-flash") {
        return LlmProtocol::Responses;
    }
    LlmProtocol::Chat
}

// ── 请求体 ──────────────────────────────────────────────

#[derive(Serialize)]
struct ResponsesRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<serde_json::Value>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    /// DeepSeek：V4 默认 thinking 开（流式先出 reasoning item 费 token），
    /// agent 工具循环保持一致用 `effort: none` 关掉（实测接受）。
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
}

#[derive(Serialize, Clone, Copy)]
struct ReasoningConfig {
    effort: &'static str,
}

/// chat 消息历史 → Responses `input` items（`system` 首条抽为 `instructions`）。
/// 转换是单向的：会话历史保持 chat 形状，每次请求时转换（不改写 session）。
pub fn messages_to_input(messages: &[Message]) -> (Option<String>, Vec<serde_json::Value>) {
    let mut instructions: Option<String> = None;
    let mut input = Vec::with_capacity(messages.len());

    for msg in messages {
        if msg.role == Role::System {
            let text = msg.content.text();
            if text.is_empty() {
                continue;
            }
            match instructions.as_mut() {
                Some(ins) => {
                    ins.push_str("\n\n");
                    ins.push_str(text);
                }
                None => instructions = Some(text.to_string()),
            }
            continue;
        }

        if let Some(tool_calls) = &msg.tool_calls {
            for tc in tool_calls {
                input.push(json!({
                    "type": "function_call",
                    "call_id": tc.id,
                    "name": tc.function.name,
                    "arguments": tc.function.arguments,
                }));
            }
            continue;
        }

        if msg.role == Role::Tool {
            input.push(json!({
                "type": "function_call_output",
                "call_id": msg.tool_call_id.clone().unwrap_or_default(),
                "output": msg.content.text(),
            }));
            continue;
        }

        let role = match msg.role {
            Role::User => "user",
            _ => "assistant",
        };
        let content: Vec<serde_json::Value> = match &msg.content {
            Content::Text(t) => vec![json!({"type": "input_text", "text": t})],
            Content::Parts(parts) => parts
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => {
                        json!({"type": "input_text", "text": text})
                    }
                    ContentPart::ImageUrl { image_url } => {
                        json!({"type": "input_image", "image_url": image_url.url})
                    }
                })
                .collect(),
        };
        input.push(json!({"type": "message", "role": role, "content": content}));
    }

    (instructions, input)
}

/// chat 格式 tools（`{"type":"function","function":{...}}`）→ Responses 新格式
/// （顶层 `name`/`description`/`parameters`）。DeepSeek /v1/responses 实测
/// 只接受后者（嵌套 function 形式 400）。
fn convert_tools(tools: &[serde_json::Value]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| match t.get("function") {
            Some(f) => json!({
                "type": "function",
                "name": f.get("name"),
                "description": f.get("description"),
                "parameters": f.get("parameters"),
            }),
            None => t.clone(),
        })
        .collect()
}

fn build_request<'a>(
    request: &'a super::LlmRequest,
    resolved_model: &'a str,
    stream: bool,
) -> ResponsesRequest<'a> {
    let (instructions, input) = messages_to_input(&request.messages);
    let reasoning = if request
        .api_base
        .to_ascii_lowercase()
        .contains("deepseek.com")
    {
        Some(ReasoningConfig { effort: "none" })
    } else {
        None
    };
    ResponsesRequest {
        model: resolved_model,
        instructions,
        input,
        stream,
        max_output_tokens: Some(request.max_tokens),
        tools: request.tools.as_deref().map(convert_tools),
        reasoning,
    }
}

// ── 流式（语义事件 SSE）──────────────────────────────────

/// Responses SSE 事件类型常量。
const EV_OUTPUT_TEXT_DELTA: &str = "response.output_text.delta";
const EV_FN_ARGS_DELTA: &str = "response.function_call_arguments.delta";
const EV_FN_ARGS_DONE: &str = "response.function_call_arguments.done";
const EV_OUTPUT_ITEM_DONE: &str = "response.output_item.done";
const EV_COMPLETED: &str = "response.completed";
const EV_FAILED: &str = "response.failed";

#[derive(Deserialize)]
struct ResponsesEvent {
    #[serde(rename = "type")]
    type_: String,
    #[serde(default)]
    item_id: Option<String>,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
    /// OpenAI 标准字段；DeepSeek 兼容实现用 `item`（双字段兼容）。
    #[serde(default)]
    output: Option<ResponsesOutputItem>,
    #[serde(default)]
    item: Option<ResponsesOutputItem>,
}

#[derive(Deserialize)]
struct ResponsesOutputItem {
    #[serde(rename = "type")]
    type_: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// 按 `item_id` 聚合的工具调用（Responses 无 index，用 item id 定位）。
#[derive(Debug)]
struct PendingResponsesToolCall {
    item_id: String,
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
}

/// 定位或创建（`arguments.delta` 是首个携带 item_id 的事件——找不到即新建）。
fn upsert_pending<'a>(
    pending: &'a mut Vec<PendingResponsesToolCall>,
    item_id: &str,
) -> &'a mut PendingResponsesToolCall {
    if let Some(idx) = pending.iter().position(|p| p.item_id == item_id) {
        return &mut pending[idx];
    }
    pending.push(PendingResponsesToolCall {
        item_id: item_id.to_string(),
        call_id: None,
        name: None,
        arguments: String::new(),
    });
    pending.last_mut().expect("just pushed")
}

/// 处理单个 Responses 事件（纯函数，便于单测；SSE 循环逐行调用）。
fn handle_responses_event(
    ev: ResponsesEvent,
    pending: &mut Vec<PendingResponsesToolCall>,
    tx: &tokio::sync::mpsc::UnboundedSender<StreamEvent>,
) -> Option<()> {
    match ev.type_.as_str() {
        EV_OUTPUT_TEXT_DELTA => {
            if let Some(delta) = ev.delta
                && !delta.is_empty()
            {
                let _ = tx.send(StreamEvent::Token(delta));
            }
            None
        }
        EV_FN_ARGS_DELTA => {
            if let (Some(item_id), Some(delta)) = (ev.item_id, ev.delta)
                && !delta.is_empty()
            {
                let p = upsert_pending(pending, &item_id);
                p.arguments.push_str(&delta);
            }
            None
        }
        EV_FN_ARGS_DONE => {
            // 部分实现只在 done 事件里给完整 arguments（覆盖增量）
            if let (Some(item_id), Some(args)) = (ev.item_id, ev.arguments)
                && let Some(p) = pending.iter_mut().find(|p| p.item_id == item_id)
            {
                p.arguments = args;
            }
            None
        }
        EV_OUTPUT_ITEM_DONE => {
            let out = ev.output.or(ev.item)?;
            if out.type_ != "function_call" {
                return None;
            }
            // 完整参数以 output_item.done 为准；call_id 优先（回填 tool_call_id 用）。
            // pending 无对应条目（参数未走 delta 直接 done）时 out 自带完整信息。
            let idx = pending
                .iter()
                .position(|p| out.id.as_deref().is_some_and(|id| id == p.item_id));
            let (id, name, arguments) = match idx {
                Some(idx) => {
                    let p = pending.remove(idx);
                    (
                        out.call_id.or(out.id).or(p.call_id).unwrap_or_default(),
                        out.name.or(p.name).unwrap_or_default(),
                        out.arguments.unwrap_or(p.arguments),
                    )
                }
                None => (
                    out.call_id.or(out.id).unwrap_or_default(),
                    out.name.unwrap_or_default(),
                    out.arguments.unwrap_or_default(),
                ),
            };
            let _ = tx.send(StreamEvent::ToolCall {
                id,
                name,
                arguments,
            });
            None
        }
        EV_COMPLETED => Some(()),
        EV_FAILED => {
            let _ = tx.send(StreamEvent::Error(
                "模型服务流式响应失败（response.failed）。".to_string(),
            ));
            None
        }
        _ => None,
    }
}

fn flush_pending(
    pending: &mut Vec<PendingResponsesToolCall>,
    tx: &tokio::sync::mpsc::UnboundedSender<StreamEvent>,
) {
    for p in pending.drain(..) {
        if let (Some(id), Some(name)) = (p.call_id.or(Some(p.item_id)), p.name) {
            let _ = tx.send(StreamEvent::ToolCall {
                id,
                name,
                arguments: p.arguments,
            });
        }
    }
}

/// 流式调用：POST {api_base}/responses → 语义事件 SSE → StreamEvent。
pub async fn stream_responses(
    request: super::LlmRequest,
    tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
) -> anyhow::Result<()> {
    let url = format!("{}/responses", request.api_base.trim_end_matches('/'));
    let resolved_model =
        crate::config::migrate_llm_model(&request.model).unwrap_or(request.model.as_str());
    let req = build_request(&request, resolved_model, true);
    let body = serde_json::to_value(&req)?;
    tracing::debug!(%url, model = %request.model, msg_count = request.messages.len(), "sending responses request");

    let client = shared_http_client();
    let response = send_with_retry(client, &url, &request.api_key, &body, Some(&tx), None).await?;
    process_responses_sse(response, tx).await
}

async fn process_responses_sse(
    response: reqwest::Response,
    tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
) -> anyhow::Result<()> {
    let mut byte_stream = response.bytes_stream();
    let mut line_buf = SseLineBuffer::new();
    let mut pending: Vec<PendingResponsesToolCall> = Vec::new();

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
                flush_pending(&mut pending, &tx);
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }
            let Ok(ev) = serde_json::from_str::<ResponsesEvent>(data) else {
                continue;
            };
            if handle_responses_event(ev, &mut pending, &tx).is_some() {
                // response.completed —— 正常收尾
                flush_pending(&mut pending, &tx);
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }
        }
    }

    // 流意外中断（无 completed）：尽量收尾已聚合的工具调用
    if let Some(line) = line_buf.flush_remainder()
        && let Some(data) = line.trim().strip_prefix("data: ")
        && data != "[DONE]"
        && let Ok(ev) = serde_json::from_str::<ResponsesEvent>(data)
    {
        handle_responses_event(ev, &mut pending, &tx);
    }
    flush_pending(&mut pending, &tx);
    let _ = tx.send(StreamEvent::Done);
    Ok(())
}

// ── 非流式（压缩/plan 判定等无工具调用）───────────────────

pub async fn complete_responses(request: super::LlmRequest) -> anyhow::Result<String> {
    let url = format!("{}/responses", request.api_base.trim_end_matches('/'));
    let resolved_model =
        crate::config::migrate_llm_model(&request.model).unwrap_or(request.model.as_str());
    let req = build_request(&request, resolved_model, false);
    let body = serde_json::to_value(&req)?;

    let client = shared_http_client();
    let response = send_with_retry(client, &url, &request.api_key, &body, None, Some(45)).await?;
    let body = response.text().await.unwrap_or_default();

    let parsed: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("invalid responses JSON: {e}; body={body}"))?;
    // output_text 便捷字段（OpenAI 拼接）；回退 output[0].content[0].text
    let content = parsed
        .get("output_text")
        .and_then(|c| c.as_str())
        .or_else(|| {
            parsed
                .pointer("/output/0/content/0/text")
                .and_then(|c| c.as_str())
        })
        .unwrap_or("")
        .trim()
        .to_string();
    if content.is_empty() {
        anyhow::bail!("empty completion content");
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{FunctionCall, ToolCall};

    #[test]
    fn protocol_routes_by_base_and_model() {
        // OpenAI 官方端点 → Responses
        assert_eq!(
            resolve_protocol("https://api.openai.com/v1", "gpt-5"),
            LlmProtocol::Responses
        );
        // DeepSeek V4 Flash → Responses（官方 2026-07-31 起原生支持）
        assert_eq!(
            resolve_protocol("https://api.deepseek.com/v1", "deepseek-v4-flash"),
            LlmProtocol::Responses
        );
        // DeepSeek 其他模型 → Chat（V4 Pro 暂不支持 Responses）
        assert_eq!(
            resolve_protocol("https://api.deepseek.com/v1", "deepseek-v4-pro"),
            LlmProtocol::Chat
        );
        // Ollama / 本地 / 第三方 → Chat
        assert_eq!(
            resolve_protocol("http://127.0.0.1:11434/v1", "qwen3-vl:8b"),
            LlmProtocol::Chat
        );
        assert_eq!(
            resolve_protocol("https://api.example-proxy.com/v1", "deepseek-v4-flash"),
            LlmProtocol::Chat
        );
    }

    fn msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: Content::Text(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn system_message_becomes_instructions() {
        let msgs = vec![msg(Role::System, "你是助手"), msg(Role::User, "你好")];
        let (instructions, input) = messages_to_input(&msgs);
        assert_eq!(instructions.as_deref(), Some("你是助手"));
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "你好");
    }

    #[test]
    fn tool_calls_and_results_map_to_items() {
        let msgs = vec![
            Message {
                role: Role::Assistant,
                content: Content::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: "read_file".into(),
                        arguments: "{\"path\":\"a\"}".into(),
                    },
                }]),
                tool_call_id: None,
            },
            Message {
                role: Role::Tool,
                content: Content::Text("内容".into()),
                tool_calls: None,
                tool_call_id: Some("call_1".into()),
            },
        ];
        let (instructions, input) = messages_to_input(&msgs);
        assert!(instructions.is_none());
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[0]["name"], "read_file");
        assert_eq!(input[1]["type"], "function_call_output");
        assert_eq!(input[1]["call_id"], "call_1");
        assert_eq!(input[1]["output"], "内容");
    }

    #[test]
    fn deepseek_item_field_variant_folds_to_tool_call() {
        // DeepSeek 兼容实现用 `item` 字段（OpenAI 标准是 `output`）——实测 2026-08。
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut pending = Vec::new();
        handle_responses_event(
            serde_json::from_str(
                r#"{"type":"response.output_item.done","item":{"type":"function_call","id":"fc_9","call_id":"call_9","name":"list_dir","arguments":"{\"path\":\".\"}"}}"#,
            )
            .unwrap(),
            &mut pending,
            &tx,
        );
        handle_responses_event(
            serde_json::from_str(r#"{"type":"response.completed"}"#).unwrap(),
            &mut pending,
            &tx,
        );
        let got = rx.try_recv().expect("tool call emitted");
        match got {
            StreamEvent::ToolCall {
                id,
                name,
                arguments,
            } => {
                assert_eq!(id, "call_9");
                assert_eq!(name, "list_dir");
                assert_eq!(arguments, r#"{"path":"."}"#);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tools_convert_to_top_level_name_format() {
        // chat 嵌套格式 → Responses 顶层 name 格式（DeepSeek 实测只接受后者）
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "读文件",
                "parameters": {"type": "object", "properties": {}},
            }
        })];
        let converted = convert_tools(&tools);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0]["type"], "function");
        assert_eq!(converted[0]["name"], "read_file");
        assert_eq!(converted[0]["description"], "读文件");
        assert!(
            converted[0].get("function").is_none(),
            "嵌套 function 应被移除"
        );
    }

    #[test]
    fn image_parts_map_to_input_image() {
        let msgs = vec![Message {
            role: Role::User,
            content: Content::Parts(vec![
                ContentPart::Text {
                    text: "看图".into(),
                },
                ContentPart::ImageUrl {
                    image_url: crate::session::ImageUrl {
                        url: "data:image/png;base64,xxx".into(),
                    },
                },
            ]),
            tool_calls: None,
            tool_call_id: None,
        }];
        let (_, input) = messages_to_input(&msgs);
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][1]["type"], "input_image");
        assert_eq!(
            input[0]["content"][1]["image_url"],
            "data:image/png;base64,xxx"
        );
    }

    #[test]
    fn sse_events_fold_to_stream_events() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut pending = Vec::new();

        let ev = |s: &str| serde_json::from_str::<ResponsesEvent>(s).unwrap();
        // 文本增量
        handle_responses_event(
            ev(r#"{"type":"response.output_text.delta","item_id":"msg_1","delta":"你好"}"#),
            &mut pending,
            &tx,
        );
        // 工具参数增量（两段）
        handle_responses_event(
            ev(
                r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":"{\"path\":"}"#,
            ),
            &mut pending,
            &tx,
        );
        handle_responses_event(
            ev(
                r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":"\"a\"}"}"#,
            ),
            &mut pending,
            &tx,
        );
        // 完整参数以 output_item.done 为准
        handle_responses_event(
            ev(
                r#"{"type":"response.output_item.done","output":{"type":"function_call","id":"fc_1","call_id":"call_x","name":"read_file","arguments":"{\"path\":\"a\"}"}}"#,
            ),
            &mut pending,
            &tx,
        );
        // 收尾
        handle_responses_event(ev(r#"{"type":"response.completed"}"#), &mut pending, &tx);

        let mut got = Vec::new();
        while let Ok(e) = rx.try_recv() {
            got.push(e);
        }
        assert_eq!(got.len(), 2, "text + tool call");
        assert!(matches!(&got[0], StreamEvent::Token(t) if t == "你好"));
        match &got[1] {
            StreamEvent::ToolCall {
                id,
                name,
                arguments,
            } => {
                assert_eq!(id, "call_x", "call_id 优先于 item id");
                assert_eq!(name, "read_file");
                assert_eq!(arguments, r#"{"path":"a"}"#);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
