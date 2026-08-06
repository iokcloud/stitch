//! Agent execution engine.
//!
//! Implements a ReAct (Reasoning + Acting) loop:
//!   think → select tool → execute → observe → repeat
//!
//! `serde_json::json!` macro uses `unwrap()` internally — allowed project-wide.
#![allow(clippy::disallowed_methods)]

pub mod context;
pub mod guard;
pub mod layers;
pub mod persist;
pub mod plan;
pub mod project;
pub mod prompt;
pub mod rules;
pub mod tokens;

use crate::llm::{self, LlmRequest, StreamEvent};
use crate::session::{self, Session, ToolCall};
use crate::tools::{self, ToolDef, ToolRegistry};
use std::collections::HashMap;
use std::sync::Arc;

/// Result of one complete agent interaction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentResult {
    pub response: String,
    pub iterations: usize,
    pub tokens_used: usize,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub context_tokens: usize,
    pub context_limit: usize,
    /// 服务端真实缓存命中输入 token（无则 0）。
    pub cache_hit_tokens: u64,
    /// 服务端真实缓存未命中输入 token。
    pub cache_miss_tokens: u64,
}

/// Events emitted during agent execution for streaming UIs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// A text token from the model (for streaming display).
    Token { text: String },
    /// A tool call has started executing.
    ToolStart {
        name: String,
        #[serde(default)]
        call_id: String,
    },
    /// Incremental output from a running tool, forwarded live to the UI
    /// (ADR-037). `text` is one or more raw lines, newline-terminated.
    ToolOutput {
        name: String,
        #[serde(default)]
        call_id: String,
        text: String,
    },
    /// A tool call has completed.
    ToolDone {
        name: String,
        #[serde(default)]
        call_id: String,
        success: bool,
        summary: String,
        /// Per-tool benchmark metrics (duration_ms, …), mirror of ToolResult.metrics.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metrics: Option<HashMap<String, f64>>,
    },
    /// The agent is requesting confirmation before executing a tool.
    ConfirmRequest {
        id: String,
        tool: String,
        message: String,
    },
    /// Plan mode: a proposed execution plan is ready for review.
    PlanProposed { id: String, plan: plan::Plan },
    /// Plan mode: plan was approved by the user.
    PlanApproved,
    /// Plan mode: plan was rejected by the user.
    PlanRejected,
    /// Plan mode: starting a step in the plan.
    PlanStepStart { index: usize, description: String },
    /// Plan mode: a step has been completed.
    PlanStepDone { index: usize, description: String },
    /// Token / context usage update (each ReAct iteration + final).
    Usage {
        iteration: usize,
        input_tokens: usize,
        output_tokens: usize,
        context_tokens: usize,
        context_limit: usize,
        compacted: bool,
        /// 服务端真实缓存命中输入 token（无则 0）。
        #[serde(default)]
        cache_hit_tokens: u64,
        /// 服务端真实缓存未命中输入 token。
        #[serde(default)]
        cache_miss_tokens: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layers: Option<layers::LayerStats>,
    },
    /// The agent has finished the task.
    Done {
        response: String,
        iterations: usize,
        #[serde(default)]
        input_tokens: usize,
        #[serde(default)]
        output_tokens: usize,
        #[serde(default)]
        context_tokens: usize,
        #[serde(default)]
        context_limit: usize,
        /// 服务端真实缓存命中输入 token（无则 0）。
        #[serde(default)]
        cache_hit_tokens: u64,
        /// 服务端真实缓存未命中输入 token。
        #[serde(default)]
        cache_miss_tokens: u64,
        /// 回合成本估算（人民币，元；Rust 价格表计算）。
        #[serde(default)]
        cost: f64,
        /// Turn ended because the iteration budget was exhausted; UI may offer
        /// a one-tap "continue" to resume the same session.
        #[serde(default)]
        hit_iteration_cap: bool,
    },
    /// An error occurred.
    Error { message: String },
    /// Non-blocking informational notice (e.g. degraded image description).
    Notice { message: String },
}

/// 渲染/确认/取消/落盘差异抽象——CLI 与桌面的 ReAct 循环共用一份核心
/// （审查发现：run_react 与 run_react_streaming 曾是 ~450 行逐行复制，
/// 差异仅在此 trait 的五个方法）。
/// 内部 trait（仅本 crate 消费）——async fn 无需公共 auto-trait 保证。
#[allow(async_fn_in_trait)]
pub trait ReactRenderer {
    /// 工具执行前的中间文本（CLI 终端渲染；桌面文本走 on_event 的 token）。
    fn on_interim_text(&mut self, text: &str);
    /// 事件出口（桌面 event_tx；CLI 忽略）。
    fn on_event(&mut self, ev: AgentEvent);
    /// 确认门：返回是否放行工具执行。
    async fn confirm_tool(&mut self, tool: &str, call_id: &str, message: &str) -> bool;
    /// 取消检查（CLI 恒 false；桌面读 cancel_flag）。
    fn is_cancelled(&self) -> bool;
    /// 回合中落盘（桌面 TurnFlusher；CLI 无操作）。
    fn flush_turn(&mut self, session: &Session);
    /// 工具是否需要确认（CLI 用 Option 规则；桌面从 Arc<Mutex> lock 读）。
    fn needs_confirmation(
        &self,
        tool: &crate::tools::Tool,
        args: &serde_json::Value,
        work_dir: Option<&str>,
    ) -> bool;
    /// 事件发送通道 clone（工具直播输出转发用，ADR-037；CLI 无）。
    fn event_tx_clone(&self) -> Option<tokio::sync::mpsc::UnboundedSender<AgentEvent>> {
        None
    }
    /// 工具级取消标志（run_command 长命令中断；CLI 无）。
    fn tool_cancel_flag(&self) -> Option<&std::sync::atomic::AtomicBool> {
        None
    }
}

/// CLI 渲染器：终端渲染 + dialoguer 确认门 + 无取消/无 flusher。
struct CliRenderer {
    skip_confirm: bool,
    work_dir: Option<String>,
    allow_rules: Option<crate::allow::AllowRules>,
}

impl CliRenderer {
    fn new(
        skip_confirm: bool,
        work_dir: Option<&str>,
        allow_rules: Option<&crate::allow::AllowRules>,
    ) -> Self {
        Self {
            skip_confirm,
            work_dir: work_dir.map(|s| s.to_string()),
            allow_rules: allow_rules.cloned(),
        }
    }
}

impl ReactRenderer for CliRenderer {
    fn on_interim_text(&mut self, text: &str) {
        crate::render::render_message(text);
    }
    fn on_event(&mut self, _ev: AgentEvent) {}
    async fn confirm_tool(&mut self, _tool: &str, _call_id: &str, message: &str) -> bool {
        crate::render::dialog::confirm(message)
    }
    fn is_cancelled(&self) -> bool {
        false
    }
    fn flush_turn(&mut self, _session: &Session) {}
    fn needs_confirmation(
        &self,
        tool: &crate::tools::Tool,
        args: &serde_json::Value,
        work_dir: Option<&str>,
    ) -> bool {
        if self.skip_confirm {
            return false;
        }
        tool.needs_confirmation(args, work_dir, self.allow_rules.as_ref())
    }
}

/// 桌面渲染器：事件发送 + oneshot 确认门 + cancel_flag + TurnFlusher。
struct DesktopRenderer<'a> {
    confirm_pending: &'a Arc<
        std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>,
    >,
    allow_rules: &'a Arc<std::sync::Mutex<crate::allow::AllowRules>>,
    event_tx: &'a tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    cancel_flag: &'a std::sync::atomic::AtomicBool,
    flusher: Option<&'a std::sync::Arc<std::sync::Mutex<persist::TurnFlusher>>>,
}

impl<'a> ReactRenderer for DesktopRenderer<'a> {
    fn on_interim_text(&mut self, _text: &str) {
        // 桌面文本经 on_event 的 Token 流式呈现，无需终端渲染。
    }
    fn on_event(&mut self, ev: AgentEvent) {
        let _ = self.event_tx.send(ev);
    }
    async fn confirm_tool(&mut self, tool: &str, call_id: &str, message: &str) -> bool {
        let confirm_id = format!("confirm-{call_id}");
        let _ = self.event_tx.send(AgentEvent::ConfirmRequest {
            id: confirm_id.clone(),
            tool: tool.to_string(),
            message: message.to_string(),
        });
        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        {
            let mut guard = self.confirm_pending.lock().expect("confirm mutex poisoned");
            guard.insert(confirm_id.clone(), tx);
        }
        match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
            Ok(Ok(true)) => true,
            Ok(Ok(false)) | Ok(Err(_)) => false,
            Err(_elapsed) => {
                tracing::warn!(%confirm_id, "confirmation dialog timed out");
                {
                    let mut guard = self.confirm_pending.lock().expect("confirm mutex poisoned");
                    guard.remove(&confirm_id);
                }
                false
            }
        }
    }
    fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn flush_turn(&mut self, session: &Session) {
        if let Some(f) = self.flusher {
            flush_turn(Some(f), session);
        }
    }
    fn needs_confirmation(
        &self,
        tool: &crate::tools::Tool,
        args: &serde_json::Value,
        work_dir: Option<&str>,
    ) -> bool {
        let rules = self.allow_rules.lock().expect("allow rules mutex poisoned");
        tool.needs_confirmation(args, work_dir, Some(&rules))
    }
    fn event_tx_clone(&self) -> Option<tokio::sync::mpsc::UnboundedSender<AgentEvent>> {
        Some(self.event_tx.clone())
    }
    fn tool_cancel_flag(&self) -> Option<&std::sync::atomic::AtomicBool> {
        Some(self.cancel_flag)
    }
}

/// 单一核心 ReAct 循环（CLI 与桌面共用——差异经 ReactRenderer 注入）。
#[allow(clippy::too_many_arguments)]
async fn run_react_core<R: ReactRenderer>(
    session: &mut Session,
    api_base: &str,
    model: &str,
    api_key: &str,
    tools: &ToolRegistry,
    max_iterations: usize,
    work_dir: Option<&str>,
    renderer: &mut R,
) -> anyhow::Result<AgentResult> {
    let mut usage = tokens::TokenUsage::default();
    let tool_defs = build_openai_tools(tools);
    let native_tool_defs = tools.definitions();
    let mut tool_guard = guard::ToolCallGuard::new();
    let ctx_limit = tokens::context_limit_for_model(model);
    let soft_lim = persist::soft_token_limit(ctx_limit);
    let hard_lim = persist::hard_token_limit(ctx_limit);
    let keep_recent = context::ContextConfig::default().keep_recent;
    let soft_state = context::SoftCompactState::new();

    // Pull archived context referenced by the latest user message back into hot.
    layers::promote_referenced_context(session);

    // 长任务治理：旧工具结果 → 占位符（Anthropic clear_tool_uses 语义）。
    // 先于压缩触发——清理可重取的大 payload 后 est 回落，可避免后续压缩；
    // 只保留最近 KEEP_TOOL_RESULTS 条结果，assistant 的 tool_calls 记录保留。
    let est_turn_start = tokens::estimate_messages(&session.messages);
    if est_turn_start > soft_lim
        && context::clear_old_tool_results(session, context::KEEP_TOOL_RESULTS)
    {
        // 清理改写历史——checkpoint 落盘，防崩溃只留内存清理态
        renderer.flush_turn(session);
    }

    for iteration in 0..max_iterations {
        // Check cancellation at start of each iteration
        if renderer.is_cancelled() {
            soft_state.invalidate();
            renderer.on_event(AgentEvent::Error {
                message: "cancelled".into(),
            });
            anyhow::bail!("cancelled");
        }

        session.iteration = iteration;
        tracing::info!(iteration, msg_count = session.messages.len(), "agent loop");

        let est = tokens::estimate_messages(&session.messages);
        let mut compacted = false;

        // Hard compact (~85%): sync; prefer ready soft candidate as draft.
        if est > hard_lim {
            if let Some(cand) = soft_state.take_ready(session.epoch) {
                compacted =
                    context::apply_compact_with_summary(session, &cand.summary, keep_recent);
            }
            soft_state.invalidate();
            if !compacted {
                compacted = context::maybe_compact_llm(
                    session,
                    &context::ContextConfig {
                        max_tokens: hard_lim,
                        keep_recent,
                    },
                    Some(context::CompactLlm {
                        api_base,
                        model,
                        api_key,
                    }),
                )
                .await;
            }
            // Epoch may have bumped — rewrite + checkpoint on disk now so a
            // crash does not strand the compacted state only in memory.
            renderer.flush_turn(session);
        }

        // Defensive: DeepSeek-strict pairing / no consecutive assistants.
        context::repair_message_sequence(&mut session.messages);

        // Estimate input tokens for this iteration (hard compact 后算一次，
        // usage 累加 / 软压判断 / emit 传参共用——避免同值重复全量扫描)。
        let est_after = tokens::estimate_messages(&session.messages);
        usage.input_tokens += est_after;
        emit_usage(
            renderer,
            iteration + 1,
            &usage,
            session,
            model,
            compacted,
            session.layers.as_ref(),
            est_after,
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let request = LlmRequest {
            api_base: api_base.to_string(),
            model: model.to_string(),
            api_key: api_key.to_string(),
            messages: session.messages.clone(),
            max_tokens: 4096,
            tools: Some(tool_defs.clone()),
        };

        let llm_handle = tokio::spawn(async move {
            if let Err(e) = llm::stream_chat(request, tx).await {
                tracing::error!(%e, "LLM streaming failed");
            }
        });

        // Soft compact (~70%): run in parallel with the main LLM stream (ADR-036).
        if est_after > soft_lim && est_after <= hard_lim && !soft_state.in_flight() {
            soft_state.try_spawn(
                session.messages.clone(),
                session.epoch,
                keep_recent,
                soft_lim,
                api_base.to_string(),
                model.to_string(),
                api_key.to_string(),
            );
        }

        let response = collect_stream_events(&mut rx, renderer, &NEVER_CANCEL, &mut usage).await;
        llm_handle.await?;

        match classify_response(response, &native_tool_defs) {
            ResponseType::ApiError(msg) => {
                anyhow::bail!("{msg}");
            }
            ResponseType::TextOnly(text) => {
                usage.output_tokens += tokens::estimate_text(&text);
                if !text.is_empty() {
                    session.add_assistant_message(&text);
                }
                let result = make_result(text.clone(), iteration + 1, &usage, session, model);
                emit_done(renderer, &result, false, model);
                return Ok(result);
            }
            ResponseType::ToolCalls { text, tool_calls } => {
                usage.output_tokens += tokens::estimate_text(&text);
                let openai_tool_calls: Vec<ToolCall> = tool_calls
                    .iter()
                    .map(|tc| ToolCall {
                        id: tc.id.clone(),
                        call_type: "function".into(),
                        function: session::FunctionCall {
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        },
                    })
                    .collect();

                session.add_assistant_tool_calls(text, openai_tool_calls);

                let mut force_final = false;
                for tc in &tool_calls {
                    renderer.on_event(AgentEvent::ToolStart {
                        name: tc.name.clone(),
                        call_id: tc.id.clone(),
                    });

                    let result = if tool_guard.should_block(&tc.name, &tc.arguments) {
                        tracing::warn!(tool = %tc.name, "blocked duplicate tool call");
                        force_final = tool_guard.should_force_final();
                        guard::ToolCallGuard::blocked_result(&tc.name)
                    } else {
                        let result =
                            execute_tool_with_renderer(tools, tc, work_dir, renderer).await;
                        enrich_tool_observation(&tc.name, result)
                    };
                    let result_str = serde_json::to_string(&result).unwrap_or_default();
                    let summary = truncate_output(&result_str);
                    // Benchmark metrics ride along structured (not just inside
                    // the truncated summary JSON) so the UI/tests can read them.
                    let metrics = result
                        .get("metrics")
                        .and_then(|m| {
                            serde_json::from_value::<HashMap<String, f64>>(m.clone()).ok()
                        })
                        .filter(|m| !m.is_empty());
                    renderer.on_event(AgentEvent::ToolDone {
                        name: tc.name.clone(),
                        call_id: tc.id.clone(),
                        success: result_str.contains("\"success\":true")
                            || result_str.contains("\"cancelled\":true"),
                        summary,
                        metrics,
                    });

                    session.add_tool_result(tc.id.clone(), serde_json::to_string(&result)?);
                }
                if force_final {
                    session.add_user_message(
                        "You are repeating the same tool call. Stop calling tools and write your final answer now.",
                    );
                }
                // Crash-safe: tool calls + results of this batch hit disk now,
                // not only at turn end.
                renderer.flush_turn(session);
                // Refresh context fill after tool results land.
                let est_tool = tokens::estimate_messages(&session.messages);
                emit_usage(
                    renderer,
                    iteration + 1,
                    &usage,
                    session,
                    model,
                    false,
                    session.layers.as_ref(),
                    est_tool,
                );
            }
            ResponseType::Empty => {
                let result = make_result(String::new(), iteration + 1, &usage, session, model);
                emit_done(renderer, &result, false, model);
                return Ok(result);
            }
        }
    }

    // Max iterations — force final response
    session.add_user_message(
        "You have reached the maximum number of iterations. \
         Please provide a final summary.",
    );

    let est_final = tokens::estimate_messages(&session.messages);
    usage.input_tokens += est_final;
    emit_usage(
        renderer,
        max_iterations,
        &usage,
        session,
        model,
        false,
        session.layers.as_ref(),
        est_final,
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let request = LlmRequest {
        api_base: api_base.to_string(),
        model: model.to_string(),
        api_key: api_key.to_string(),
        messages: session.messages.clone(),
        max_tokens: 2048,
        tools: None,
    };

    tokio::spawn(async move {
        let _ = llm::stream_chat(request, tx).await;
    });

    let response = collect_stream_events(&mut rx, renderer, &NEVER_CANCEL, &mut usage).await;
    let result = match classify_response(response, &native_tool_defs) {
        ResponseType::ApiError(msg) => anyhow::bail!("{msg}"),
        ResponseType::TextOnly(text) | ResponseType::ToolCalls { text, .. } => {
            usage.output_tokens += tokens::estimate_text(&text);
            make_result(text, max_iterations, &usage, session, model)
        }
        ResponseType::Empty => make_result(
            "Max iterations reached.".into(),
            max_iterations,
            &usage,
            session,
            model,
        ),
    };
    emit_done(renderer, &result, true, model);
    Ok(result)
}

/// collect_stream_events 的取消占位——collect 阶段的中断由
/// renderer.is_cancelled() 驱动（桌面 renderer 内部转发此标志）。
static NEVER_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Run the ReAct agent loop for a single task.
///
/// `work_dir` / `allow_rules` feed the confirm gate (outside-workspace reads
/// and persisted allow rules). Ignored when `skip_confirm` is set.
#[allow(clippy::too_many_arguments)]
pub async fn run_react(
    session: &mut Session,
    api_base: &str,
    model: &str,
    api_key: &str,
    tools: &ToolRegistry,
    max_iterations: usize,
    skip_confirm: bool,
    work_dir: Option<&str>,
    allow_rules: Option<&crate::allow::AllowRules>,
) -> anyhow::Result<AgentResult> {
    let mut renderer = CliRenderer::new(skip_confirm, work_dir, allow_rules);
    run_react_core(
        session,
        api_base,
        model,
        api_key,
        tools,
        max_iterations,
        work_dir,
        &mut renderer,
    )
    .await
}

// -- Internal helpers --

struct RawResponse {
    text: String,
    tool_calls: Vec<llm::stream::CompletedToolCall>,
    /// Provider / transport error (e.g. HTTP 400). Already emitted to UI.
    error: Option<String>,
}

enum ResponseType {
    TextOnly(String),
    ToolCalls {
        text: String,
        tool_calls: Vec<llm::stream::CompletedToolCall>,
    },
    Empty,
    ApiError(String),
}

/// Strip reasoning/thinking blocks from model output.
///
/// Modern reasoning models (Qwen3, DeepSeek-R1, Claude) wrap chain-of-thought
/// in markers like `<think>...</think>` or `<|begin_of_thought|>...<|end_of_thought|>`.
/// These are useful for the model's reasoning but pollute the agent's
/// accumulated context and burn the token budget.
///
/// We strip them after the stream completes (before classification) so the
/// UI can still show reasoning in real time via streaming tokens.
/// 每迭代必跑的三个正则——LazyLock 静态化（不再每次 Regex::new 重编译）。
static RE_THINK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)<think>.*?</think>|<\|begin_of_thought\|>.*?<\|end_of_thought\|>")
        .unwrap()
});
static RE_TEXT_TOOL_CALL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r#"(\w+)\s*\(\s*"((?:[^"\\]|\\.)*)"(?:\s*,\s*"((?:[^"\\]|\\.)*)")?\s*\)"#)
        .unwrap()
});
static RE_MULTI_BLANK: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"\n{3,}").unwrap());

fn strip_thinking(text: &str) -> String {
    // Simple regex-based approach: remove content between think tags.
    // Handles Qwen3: <think>...</think>
    // Handles DeepSeek-R1: <|begin_of_thought|>...<|end_of_thought|>
    RE_THINK.replace_all(text, "").trim().to_string()
}

/// Parse text-format tool calls from model output when the model doesn't support
/// native OpenAI function calling (e.g. Qwen3, DeepSeek-R1, Claude fallback).
///
/// Handles patterns like:
/// - `tool_name("arg1")`
/// - `tool_name("arg1", "arg2")`
/// - `tool_name("arg1", "arg2 with spaces and (parens) okay")`
///
/// Returns `(remaining_text, Vec<CompletedToolCall>)`.
/// Remaining text is the original text with matched tool call lines removed.
fn parse_text_tool_calls(
    text: &str,
    tool_defs: &[ToolDef],
) -> (String, Vec<llm::stream::CompletedToolCall>) {
    // Match: name("arg1"[, "arg2"[, ...]]) - positional quoted-arg calls
    let re = &*RE_TEXT_TOOL_CALL;

    let mut call_id_counter = 0u32;
    let mut tool_calls: Vec<llm::stream::CompletedToolCall> = Vec::new();

    // Build tool param lookup: name -> ordered param names
    let param_order: std::collections::HashMap<&str, Vec<&str>> = tool_defs
        .iter()
        .map(|def| {
            let required: Vec<&str> = def
                .parameters
                .get("required")
                .and_then(|r| r.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let props = def.parameters.get("properties").and_then(|p| p.as_object());

            let mut ordered: Vec<&str> = Vec::new();
            // Required params first
            for name in &required {
                ordered.push(*name);
            }
            // Then optional params (in schema order)
            if let Some(props) = props {
                for key in props.keys() {
                    if !required.contains(&key.as_str()) {
                        ordered.push(key.as_str());
                    }
                }
            }
            (def.name.as_str(), ordered)
        })
        .collect();

    let mut remaining = text.to_string();

    for caps in re.captures_iter(text) {
        let tool_name = caps.get(1).unwrap().as_str();
        let arg1 = unescape_quoted(caps.get(2).unwrap().as_str());
        let arg2 = caps.get(3).map(|m| unescape_quoted(m.as_str()));

        // Map positional args to param names
        let param_names = match param_order.get(tool_name) {
            Some(names) => names,
            None => continue, // Unknown tool — skip
        };

        let mut args = serde_json::Map::new();
        args.insert(param_names[0].to_string(), serde_json::Value::String(arg1));
        if let (Some(a2), Some(&p2)) = (arg2, param_names.get(1)) {
            args.insert(p2.to_string(), serde_json::Value::String(a2));
        }

        let arguments = serde_json::Value::Object(args).to_string();
        let call_id = format!("call_text_{:04}", call_id_counter);
        call_id_counter += 1;

        tool_calls.push(llm::stream::CompletedToolCall {
            id: call_id,
            name: tool_name.to_string(),
            arguments,
        });

        // Remove the matched call from remaining text
        let matched = caps.get(0).unwrap().as_str();
        remaining = remaining.replace(matched, "");
    }

    // Clean up: collapse multiple blank lines, trim
    let cleaned = RE_MULTI_BLANK
        .replace_all(&remaining, "\n\n")
        .trim()
        .to_string();

    (cleaned, tool_calls)
}

/// Undo basic JSON-string escapes that the regex captured literally.
fn unescape_quoted(s: &str) -> String {
    s.replace("\\\"", "\"")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\r", "\r")
        .replace("\\\\", "\\")
}

fn classify_response(raw: RawResponse, tool_defs: &[ToolDef]) -> ResponseType {
    if let Some(err) = raw.error {
        return ResponseType::ApiError(err);
    }
    let text = strip_thinking(&raw.text);
    if !raw.tool_calls.is_empty() {
        return ResponseType::ToolCalls {
            text,
            tool_calls: raw.tool_calls,
        };
    }
    // No native tool calls — try text-based parsing for models like Qwen3
    if !text.is_empty() && !tool_defs.is_empty() {
        let (clean_text, parsed_calls) = parse_text_tool_calls(&text, tool_defs);
        if !parsed_calls.is_empty() {
            return ResponseType::ToolCalls {
                text: clean_text,
                tool_calls: parsed_calls,
            };
        }
    }
    if text.is_empty() {
        ResponseType::Empty
    } else {
        ResponseType::TextOnly(text)
    }
}

fn build_openai_tools(registry: &tools::ToolRegistry) -> Vec<serde_json::Value> {
    registry
        .definitions()
        .into_iter()
        .map(|def| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": def.name,
                    "description": def.description,
                    "parameters": def.parameters,
                }
            })
        })
        .collect()
}

/// Run the ReAct agent loop, streaming events through the provided channel.
///
/// This is the desktop/TUI variant: instead of calling terminal render functions,
/// it emits `AgentEvent`s through `event_tx` so the UI layer can render them.
///
/// `flusher`: optional mid-turn crash-safe persistence (ADR-036); new messages
/// are appended to disk as the turn progresses.
///
/// `work_dir` / `allow_rules` feed the confirm gate (outside-workspace reads
/// and persisted allow rules). `allow_rules` is shared with
/// `respond_confirmation` so a rule remembered mid-turn applies to the next
/// call.
#[allow(clippy::too_many_arguments)]
pub async fn run_react_streaming(
    session: &mut Session,
    api_base: &str,
    model: &str,
    api_key: &str,
    tools: &ToolRegistry,
    max_iterations: usize,
    confirm_pending: Arc<
        std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>,
    >,
    work_dir: Option<&str>,
    allow_rules: std::sync::Arc<std::sync::Mutex<crate::allow::AllowRules>>,
    event_tx: &tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    cancel_flag: &std::sync::atomic::AtomicBool,
    flusher: Option<&std::sync::Arc<std::sync::Mutex<persist::TurnFlusher>>>,
) -> anyhow::Result<AgentResult> {
    let mut renderer = DesktopRenderer {
        confirm_pending: &confirm_pending,
        allow_rules: &allow_rules,
        event_tx,
        cancel_flag,
        flusher,
    };
    run_react_core(
        session,
        api_base,
        model,
        api_key,
        tools,
        max_iterations,
        work_dir,
        &mut renderer,
    )
    .await
}

fn make_result(
    response: String,
    iterations: usize,
    usage: &tokens::TokenUsage,
    session: &Session,
    model: &str,
) -> AgentResult {
    let context_limit = tokens::context_limit_for_model(model);
    let context_tokens = tokens::estimate_messages(&session.messages);
    AgentResult {
        response,
        iterations,
        tokens_used: usage.total(),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        context_tokens,
        context_limit,
        cache_hit_tokens: usage.cache_hit_tokens,
        cache_miss_tokens: usage.cache_miss_tokens,
    }
}

#[allow(clippy::too_many_arguments)] // 内部 helper，context 数据直传
fn emit_usage<R: ReactRenderer>(
    renderer: &mut R,
    iteration: usize,
    usage: &tokens::TokenUsage,
    session: &Session,
    model: &str,
    compacted: bool,
    layer_manager: Option<&layers::LayerManager>,
    context_tokens: usize,
) {
    let context_limit = tokens::context_limit_for_model(model);
    let layers = layer_manager.map(|lm| lm.estimate_stats(&session.messages, context_limit));
    renderer.on_event(AgentEvent::Usage {
        iteration,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        context_tokens,
        context_limit,
        compacted,
        cache_hit_tokens: usage.cache_hit_tokens,
        cache_miss_tokens: usage.cache_miss_tokens,
        layers,
    });
}

/// Best-effort mid-turn disk flush; never blocks the agent loop on errors.
fn flush_turn(
    flusher: Option<&std::sync::Arc<std::sync::Mutex<persist::TurnFlusher>>>,
    session: &Session,
) {
    let Some(f) = flusher else { return };
    match f.lock() {
        Ok(mut g) => g.flush(session),
        Err(_) => tracing::warn!("turn flusher lock poisoned; skip flush"),
    }
}

fn emit_done<R: ReactRenderer>(
    renderer: &mut R,
    result: &AgentResult,
    hit_iteration_cap: bool,
    model: &str,
) {
    let cost = tokens::estimate_cost(
        &tokens::TokenUsage {
            input_tokens: result.input_tokens,
            output_tokens: result.output_tokens,
            cache_hit_tokens: result.cache_hit_tokens,
            cache_miss_tokens: result.cache_miss_tokens,
        },
        model,
    );
    renderer.on_event(AgentEvent::Done {
        response: result.response.clone(),
        iterations: result.iterations,
        input_tokens: result.input_tokens,
        output_tokens: result.output_tokens,
        context_tokens: result.context_tokens,
        context_limit: result.context_limit,
        cache_hit_tokens: result.cache_hit_tokens,
        cache_miss_tokens: result.cache_miss_tokens,
        cost,
        hit_iteration_cap,
    });
}

async fn collect_stream_events<R: ReactRenderer>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<StreamEvent>,
    renderer: &mut R,
    cancel_flag: &std::sync::atomic::AtomicBool,
    usage: &mut tokens::TokenUsage,
) -> RawResponse {
    use std::sync::atomic::Ordering;
    let mut text = String::new();
    let mut tool_calls: Vec<llm::stream::CompletedToolCall> = Vec::new();

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(StreamEvent::Token(t)) => {
                        text.push_str(&t);
                        renderer.on_interim_text(&t);
                        renderer.on_event(AgentEvent::Token { text: t });
                    }
                    Some(StreamEvent::ToolCall {
                        id,
                        name,
                        arguments,
                    }) => {
                        tool_calls.push(llm::stream::CompletedToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                    Some(StreamEvent::Usage {
                        cache_hit_tokens,
                        cache_miss_tokens,
                        ..
                    }) => {
                        // 真实 usage 只更新缓存统计（输入/输出仍用本地估算，
                        // 与 compaction 阈值/历史展示保持一致）
                        usage.cache_hit_tokens += cache_hit_tokens;
                        usage.cache_miss_tokens += cache_miss_tokens;
                    }
                    Some(StreamEvent::Done) => break,
                    Some(StreamEvent::Error(msg)) => {
                        renderer.on_event(AgentEvent::Error {
                            message: msg.clone(),
                        });
                        // Stop collecting — a 4xx leaves the channel empty next;
                        // treat as hard failure for this turn.
                        return RawResponse {
                            text: String::new(),
                            tool_calls: Vec::new(),
                            error: Some(msg),
                        };
                    }
                    None => break,
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                if cancel_flag.load(Ordering::SeqCst) {
                    break;
                }
            }
        }
    }

    RawResponse {
        text,
        tool_calls,
        error: None,
    }
}

/// Enrich tool JSON before it is stored in the session / shown in UI.
/// Failed or empty observations get an explicit next-step hint so the model
/// does not continue as if the call succeeded.
fn enrich_tool_observation(tool_name: &str, mut result: serde_json::Value) -> serde_json::Value {
    let err_text = result
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("")
        .to_string();
    let cancelled = result
        .get("cancelled")
        .and_then(|c| c.as_bool())
        .unwrap_or(false);
    let success = result
        .get("success")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    let output = result
        .get("output")
        .and_then(|o| o.as_str())
        .unwrap_or("")
        .to_string();

    if cancelled {
        return result;
    }

    if !err_text.is_empty() {
        let hint = tool_error_hint(tool_name, &err_text);
        result["error"] = serde_json::Value::String(format!("{err_text}\n\n[Agent hint] {hint}"));
        return result;
    }

    if !success {
        let body = if output.is_empty() {
            "Tool reported failure with empty output.".to_string()
        } else {
            output.clone()
        };
        let hint = tool_error_hint(tool_name, &body);
        if tool_name == "run_command" {
            let extra = "\n\n[Agent hint] The command's stderr was streamed live above. \
                         Read the error output, determine the root cause, and fix the issue \
                         before retrying. For missing tools: install them. For wrong paths: \
                         use find_path or list_directory.";
            result["output"] =
                serde_json::Value::String(format!("{output}{extra}\n\n[Agent hint] {hint}"));
        } else {
            result["output"] = serde_json::Value::String(format!("{body}\n\n[Agent hint] {hint}"));
        }
        return result;
    }

    let emptyish = output.trim().is_empty()
        || output.trim() == "(no output)"
        || output.trim().eq_ignore_ascii_case("no output");
    if emptyish {
        result["output"] = serde_json::Value::String(format!(
            "{}\n\n[Agent hint] Command/tool returned no usable output. \
             Retry with different arguments (for git: prefer git_diff / git_status), \
             or verify the command. Do not assume success based on an empty result.",
            if output.trim().is_empty() {
                "(no output)"
            } else {
                output.trim()
            }
        ));
        // Keep success true for exit-0 empty cmds, but force the model to notice.
    }

    result
}

fn tool_error_hint(tool_name: &str, err_or_body: &str) -> String {
    let lower = err_or_body.to_ascii_lowercase();
    if lower.contains("missing 'path'") || lower.contains("missing \"path\"") {
        return format!(
            "Call `{tool_name}` again with a required `path` relative to the working directory \
             (example: `rust/crates/stitch/src/lib.rs`). Do not continue as if the read succeeded."
        );
    }
    if lower.contains("missing 'command'") || lower.contains("missing \"command\"") {
        return "Call `run_command` again with a non-empty `command` string.".into();
    }
    if lower.contains("exit code")
        || lower.contains("exited with")
        || lower.contains("command failed")
    {
        return "run_command failed. Read the stderr output above, identify the root cause, fix the command, and retry ONCE. Do NOT retry more than once without changing the approach. Common fixes: missing dependency (install it), wrong path (check with list_directory), permission denied (use a different location).".into();
    }
    if lower.contains("error:")
        && (lower.contains("compilation")
            || lower.contains("build failed")
            || lower.contains("cargo")
            || lower.contains("npm err")
            || lower.contains("rustc"))
    {
        return "Build/compilation error detected. Read the error message, fix the source file referenced in the error, and retry the build ONCE. Focus on the FIRST error — later errors are often cascading.".into();
    }
    if lower.contains("no such file")
        || lower.contains("cannot find")
        || lower.contains("not found")
    {
        return "File/path not found. Use find_path or list_directory to locate the correct path, then retry. Do not guess paths.".into();
    }
    if lower.contains("connection refused")
        || lower.contains("timeout")
        || lower.contains("network")
        || lower.contains("dns")
    {
        return "Network error — the service may be temporarily unavailable. Wait 2 seconds and retry ONCE. If it fails again, report the error to the user.".into();
    }
    format!(
        "Fix the `{tool_name}` call (arguments / path / command) and retry once. \
         Do not ignore this failure."
    )
}

/// Truncate to at most `max_bytes`, never splitting a UTF-8 codepoint.
fn truncate_str_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn truncate_output(json: &str) -> String {
    // Parse tool result and return the output for the UI (preserve newlines).
    // ADR-037: the tool card is the user's window into command output — keep
    // up to 20KB so valuable tail content (errors, summaries) is not lost.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
        if let Some(output) = v.get("output").and_then(|o| o.as_str()) {
            const MAX_BYTES: usize = 20_000;
            if output.len() > MAX_BYTES {
                // Keep the tail: errors and final results live at the end.
                let mut start = output.len() - MAX_BYTES;
                while start < output.len() && !output.is_char_boundary(start) {
                    start += 1;
                }
                format!("[…前 {start} 字节已省略]\n{}", &output[start..])
            } else {
                output.to_string()
            }
        } else if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            format!("Error: {}", truncate_str_bytes(err, 2000))
        } else {
            "Done".into()
        }
    } else {
        "Done".into()
    }
}

/// 工具执行（CLI/桌面共用——确认门经 renderer 注入）：
/// 解析 → scrub 内部标记 → 越界/确认判定 → scoped 标记注入 → 执行。
async fn execute_tool_with_renderer<R: ReactRenderer>(
    registry: &ToolRegistry,
    tc: &llm::stream::CompletedToolCall,
    work_dir: Option<&str>,
    renderer: &mut R,
) -> serde_json::Value {
    let tool = match registry.get(&tc.name) {
        Some(t) => t,
        None => {
            return serde_json::json!({"error": format!("Unknown tool: {}", tc.name)});
        }
    };

    let raw_args: serde_json::Value = match serde_json::from_str(&tc.arguments) {
        Ok(v) => v,
        Err(e) => {
            return serde_json::json!({"error": format!("Invalid arguments: {e}")});
        }
    };

    // Scrub the internal marker so a model-invented key cannot self-authorize;
    // `__stitch_scoped` is re-injected below only after approval / rule match.
    let args = tools::scrub_scoped_marker(&raw_args);
    let outside = tool.scoped_read_target(&args, work_dir);
    let needs = renderer.needs_confirmation(tool, &args, work_dir);
    let mut exec_args = args;

    if needs {
        let desc = match &outside {
            Some(p) => format!(
                "Read outside workspace: {}
Allow?",
                p.display()
            ),
            None => tool.confirm_message(&exec_args),
        };
        if !renderer.confirm_tool(&tc.name, &tc.id, &desc).await {
            return serde_json::json!({
                "cancelled": true,
                "message": "User denied the operation."
            });
        }
    }
    if outside.is_some() {
        exec_args[crate::allow::SCOPED_MARKER] = serde_json::json!(true);
    }

    // ADR-037 直播输出：run_command 逐行经转发任务推 ToolOutput 事件。
    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    if let Some(fwd_tx) = renderer.event_tx_clone() {
        let name = tc.name.clone();
        let call_id = tc.id.clone();
        tokio::spawn(async move {
            while let Some(line) = prog_rx.recv().await {
                let _ = fwd_tx.send(AgentEvent::ToolOutput {
                    name: name.clone(),
                    call_id: call_id.clone(),
                    text: line,
                });
            }
        });
    } else {
        drop(prog_rx);
    }

    let cancel_flag = renderer.tool_cancel_flag();
    match tool
        .execute_with_progress(exec_args, Some(prog_tx), cancel_flag)
        .await
    {
        // 完整序列化（含 metrics——旧 confirm gate 手写 {success, output}
        // 曾静默丢弃 benchmark 指标，回归测试 tool_result_json_keeps_metrics 兜底）。
        Ok(result) => serde_json::to_value(&result).unwrap_or_default(),
        Err(e) => serde_json::json!({"error": format!("{e:#}")}),
    }
}

#[cfg(test)]
mod tests {
    use super::{enrich_tool_observation, truncate_output};

    fn truncate_output_keeps_output_under_20kb_verbatim() {
        let line = "审查验收证明".repeat(80); // multi-byte, under 20KB
        let json = serde_json::json!({
            "success": true,
            "output": line.clone(),
        })
        .to_string();
        let summary = truncate_output(&json);
        assert_eq!(summary, line);
    }

    #[test]
    fn truncate_output_over_20kb_keeps_tail() {
        let head = "旧输出行\n".repeat(3000); // well over 20KB
        let tail_marker = "最终错误：编译失败";
        let output = format!("{head}{tail_marker}\n");
        let json = serde_json::json!({
            "success": false,
            "output": output,
        })
        .to_string();
        let summary = truncate_output(&json);
        assert!(
            summary.contains(tail_marker),
            "tail must survive: {summary}"
        );
        assert!(summary.starts_with("[…前 "), "marker: {summary}");
        assert!(!summary.contains('\u{FFFD}'));
        assert!(summary.len() <= 20_000 + 64);
    }

    #[test]
    fn tool_result_json_keeps_metrics() {
        // 回归：confirm gate 曾手写 {success, output} 静默丢弃 benchmark
        // metrics——ToolResult 完整序列化必须保留 duration_ms。
        let r = crate::tools::ToolResult {
            metrics: Some(std::collections::HashMap::from([(
                "duration_ms".to_string(),
                123.4,
            )])),
            success: true,
            output: "done".to_string(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["metrics"]["duration_ms"], 123.4);
        assert_eq!(v["success"], true);
        assert_eq!(v["output"], "done");
    }

    #[test]
    fn truncate_output_error_cjk() {
        let err = "密钥验证失败：".repeat(40);
        let json = serde_json::json!({ "error": err }).to_string();
        let summary = truncate_output(&json);
        assert!(summary.starts_with("Error: "));
        assert!(summary.is_char_boundary(summary.len()));
    }

    #[test]
    fn enrich_missing_path_adds_retry_hint() {
        let raw = serde_json::json!({"error": "Missing 'path' argument"});
        let out = enrich_tool_observation("read_file", raw);
        let err = out["error"].as_str().unwrap();
        assert!(err.contains("[Agent hint]"));
        assert!(err.contains("path"));
        assert!(err.contains("Do not continue"));
    }

    #[test]
    fn enrich_empty_output_adds_hint() {
        let raw = serde_json::json!({"success": true, "output": "(no output)"});
        let out = enrich_tool_observation("run_command", raw);
        let text = out["output"].as_str().unwrap();
        assert!(text.contains("[Agent hint]"));
        assert!(text.contains("git_diff") || text.contains("Retry"));
    }
}

#[cfg(test)]
mod strip_thinking_tests {
    use super::strip_thinking;

    #[test]
    fn strip_qwen3_think_blocks() {
        let input = "<think>Let me analyze this carefully.\nI need to use a tool.</think>\n\nI'll use run_command.";
        let result = strip_thinking(input);
        assert!(!result.contains("think"));
        assert!(result.contains("I'll use run_command."));
    }

    #[test]
    fn strip_deepseek_r1_tags() {
        let input = "<|begin_of_thought|>Reasoning here...\nDeep analysis.<|end_of_thought|>\n\nFinal answer.";
        let result = strip_thinking(input);
        assert!(!result.contains("begin_of_thought"));
        assert!(result.contains("Final answer."));
    }

    #[test]
    fn no_block_returns_unchanged() {
        let input = "Just a regular response with no thinking.";
        let result = strip_thinking(input);
        assert_eq!(result, input);
    }

    #[test]
    fn only_thinking_returns_empty() {
        let input = "<think>Just thinking here.</think>";
        let result = strip_thinking(input);
        assert_eq!(result, "");
    }

    #[test]
    fn thinking_before_tool_call() {
        let input = "<think>I need to run a command.</think>\n<tool_call>run_command</tool_call>";
        let result = strip_thinking(input);
        assert!(!result.contains("think"));
        assert!(result.contains("<tool_call>"));
    }

    #[test]
    fn handles_crlf() {
        let input = "<think>reasoning</think>\r\n\r\nResponse text.";
        let result = strip_thinking(input);
        assert!(result.contains("Response text."));
        assert!(!result.contains("think"));
    }
}

#[cfg(test)]
mod parse_text_tool_calls_tests {
    use super::parse_text_tool_calls;
    use crate::tools::ToolDef;

    fn tool_defs() -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "write_file".into(),
                description: "Write a file".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"}
                    },
                    "required": ["path", "content"]
                }),
            },
            ToolDef {
                name: "run_command".into(),
                description: "Run a shell command".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"}
                    },
                    "required": ["command"]
                }),
            },
        ]
    }

    #[test]
    fn parse_single_arg_tool() {
        let defs = tool_defs();
        let input = "run_command(\"python3 fib.py\")";
        let (remaining, calls) = parse_text_tool_calls(input, &defs);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "run_command");
        assert!(calls[0].arguments.contains("python3 fib.py"));
        assert!(remaining.is_empty());
    }

    #[test]
    fn parse_multi_arg_tool() {
        let defs = tool_defs();
        let input = "write_file(\"fib.py\", \"print('hello')\")";
        let (remaining, calls) = parse_text_tool_calls(input, &defs);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
        let args: serde_json::Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(args["path"], "fib.py");
        assert_eq!(args["content"], "print('hello')");
        assert!(remaining.is_empty());
    }

    #[test]
    fn parse_multiple_tools() {
        let defs = tool_defs();
        let input = "write_file(\"a.py\", \"x=1\")\nrun_command(\"python3 a.py\")";
        let (remaining, calls) = parse_text_tool_calls(input, &defs);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[1].name, "run_command");
        assert!(remaining.trim().is_empty());
    }

    #[test]
    fn parse_qwen3_real_output() {
        let defs = tool_defs();
        let input = "write_file(\"fib.py\", \"fib = [0, 1]\nfor i in range(2, 10):\n    fib.append(fib[i-1] + fib[i-2])\nprint(fib)\")\nrun_command(\"python3 fib.py\")";
        let (remaining, calls) = parse_text_tool_calls(input, &defs);
        assert_eq!(calls.len(), 2, "got calls: {calls:?}");
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[1].name, "run_command");
        assert!(remaining.trim().is_empty(), "remaining: {remaining:?}");
    }

    #[test]
    fn no_tool_calls_returns_unchanged() {
        let defs = tool_defs();
        let input = "Just a regular response.";
        let (remaining, calls) = parse_text_tool_calls(input, &defs);
        assert!(calls.is_empty());
        assert_eq!(remaining.trim(), input.trim());
    }

    #[test]
    fn unknown_tool_skipped() {
        let defs = tool_defs();
        let input = "unknown_tool(\"arg\")\nrun_command(\"ls\")";
        let (remaining, calls) = parse_text_tool_calls(input, &defs);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "run_command");
        assert!(remaining.contains("unknown_tool"));
    }
}
