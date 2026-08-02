//! Context window management — smart conversation summarization.
//!
//! When a conversation grows too large for the model's context window,
//! we automatically compact older messages into a short summary while
//! preserving the system prompt and the most recent exchanges.
//!
//! Default path uses an LLM condenser (OpenHands-style) with a heuristic
//! snippet draft as input / fallback so tool pairing stays intact.

use crate::agent::tokens;
use crate::llm::{self, LlmRequest};
use crate::session::{Message, Role, Session};
use std::collections::HashSet;

/// Configuration for automatic context compaction.
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Maximum estimated tokens before compaction triggers.
    pub max_tokens: usize,
    /// Number of most recent messages to always preserve (after system prompt).
    pub keep_recent: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            // Generous default — most models handle 128k+, but 64k keeps us safe
            max_tokens: 64_000,
            keep_recent: 20,
        }
    }
}

/// Soft (~70%) candidate produced in parallel with ReAct (ADR-036).
/// Does **not** mutate the live session; hard compact may reuse the summary.
#[derive(Debug, Clone)]
pub struct SoftCompactCandidate {
    pub parent_epoch: u32,
    pub summary: String,
}

/// Background soft-compact job handle (invalidate on cancel / hard compact).
#[derive(Debug, Default)]
pub struct SoftCompactState {
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    in_flight: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ready: std::sync::Arc<std::sync::Mutex<Option<SoftCompactCandidate>>>,
}

impl SoftCompactState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn invalidate(&self) {
        use std::sync::atomic::Ordering;
        self.cancel.store(true, Ordering::SeqCst);
        self.in_flight.store(false, Ordering::SeqCst);
        if let Ok(mut g) = self.ready.lock() {
            *g = None;
        }
    }

    pub fn take_ready(&self, live_epoch: u32) -> Option<SoftCompactCandidate> {
        let Ok(mut g) = self.ready.lock() else {
            return None;
        };
        let cand = g.take()?;
        if cand.parent_epoch != live_epoch {
            return None;
        }
        Some(cand)
    }

    pub fn in_flight(&self) -> bool {
        self.in_flight.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Spawn LLM soft condensation on a **clone** of messages; never touches live Session.
    pub fn try_spawn(
        &self,
        messages: Vec<Message>,
        parent_epoch: u32,
        keep_recent: usize,
        soft_max_tokens: usize,
        api_base: String,
        model: String,
        api_key: String,
    ) {
        use std::sync::atomic::Ordering;
        if self.in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        self.cancel.store(false, Ordering::SeqCst);
        let cancel = self.cancel.clone();
        let in_flight = self.in_flight.clone();
        let ready = self.ready.clone();
        tokio::spawn(async move {
            let cfg = ContextConfig {
                max_tokens: soft_max_tokens,
                keep_recent,
            };
            let outcome = async {
                if cancel.load(Ordering::SeqCst) {
                    return None;
                }
                let Some((_keep, draft)) = plan_compact_messages(&messages, &cfg) else {
                    return None;
                };
                if cancel.load(Ordering::SeqCst) {
                    return None;
                }
                let creds = CompactLlm {
                    api_base: api_base.as_str(),
                    model: model.as_str(),
                    api_key: api_key.as_str(),
                };
                let summary = match llm_rewrite_summary(&draft, creds).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(%e, "soft compact LLM failed; using draft");
                        draft
                    }
                };
                if cancel.load(Ordering::SeqCst) {
                    return None;
                }
                Some(SoftCompactCandidate {
                    parent_epoch,
                    summary,
                })
            }
            .await;
            if let Some(cand) = outcome
                && let Ok(mut g) = ready.lock()
            {
                *g = Some(cand);
                tracing::info!(parent_epoch, "soft compact candidate ready");
            }
            in_flight.store(false, Ordering::SeqCst);
        });
    }
}

/// Credentials for optional LLM condensation (same chat model as the agent).
#[derive(Debug, Clone, Copy)]
pub struct CompactLlm<'a> {
    pub api_base: &'a str,
    pub model: &'a str,
    pub api_key: &'a str,
}

/// Repair history before provider calls (DeepSeek-strict role rules).
///
/// 1. Merge consecutive `assistant` messages (content + tool_calls).
/// 2. Ensure each `tool_calls` block has matching `tool` results (stub gaps;
///    drop orphan tools).
pub fn repair_message_sequence(messages: &mut Vec<Message>) {
    merge_consecutive_assistants(messages);
    sanitize_tool_pairing(messages);
}

/// DeepSeek rejects consecutive assistant turns; merge into one.
fn merge_consecutive_assistants(messages: &mut Vec<Message>) {
    if messages.len() < 2 {
        return;
    }
    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    for msg in messages.drain(..) {
        if msg.role == Role::Assistant
            && let Some(prev) = out.last_mut()
            && prev.role == Role::Assistant
        {
            if !msg.content.text_is_empty() {
                if !prev.content.text_is_empty() {
                    prev.content.text_mut().push('\n');
                }
                prev.content.text_mut().push_str(msg.content.text());
            }
            match (&mut prev.tool_calls, msg.tool_calls) {
                (Some(dst), Some(src)) => dst.extend(src),
                (None, Some(src)) => prev.tool_calls = Some(src),
                _ => {}
            }
            continue;
        }
        out.push(msg);
    }
    *messages = out;
}

/// Drop orphan `tool` messages; stub missing results for assistant `tool_calls`.
///
/// Providers (OpenAI / DeepSeek) reject histories where `role=tool` is not
/// immediately preceded by an assistant message that requested those ids.
pub fn sanitize_tool_pairing(messages: &mut Vec<Message>) {
    if messages.is_empty() {
        return;
    }
    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    let mut i = 0;
    while i < messages.len() {
        match messages[i].role {
            Role::Tool => {
                // Orphan tool result (no open assistant tool_calls group) — skip.
                i += 1;
            }
            Role::Assistant
                if messages[i]
                    .tool_calls
                    .as_ref()
                    .is_some_and(|c| !c.is_empty()) =>
            {
                let expected_order: Vec<String> = messages[i]
                    .tool_calls
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|c| c.id.clone())
                    .collect();
                let expected: HashSet<String> = expected_order.iter().cloned().collect();
                let mut by_id: std::collections::HashMap<String, Message> =
                    std::collections::HashMap::new();
                let mut j = i + 1;
                while j < messages.len() && messages[j].role == Role::Tool {
                    if let Some(id) = messages[j].tool_call_id.clone() {
                        by_id.insert(id, messages[j].clone());
                    }
                    j += 1;
                }
                let got: HashSet<String> = by_id.keys().cloned().collect();
                if expected.is_empty() {
                    i = j;
                    continue;
                }
                if got != expected {
                    tracing::warn!(
                        expected = expected.len(),
                        got = got.len(),
                        missing = expected.difference(&got).count(),
                        "repairing incomplete tool_calls block with stubs"
                    );
                }
                out.push(messages[i].clone());
                for id in expected_order {
                    if let Some(m) = by_id.remove(&id) {
                        out.push(m);
                    } else {
                        out.push(Message {
                            role: Role::Tool,
                            content: "[tool result unavailable]".into(),
                            tool_calls: None,
                            tool_call_id: Some(id),
                        });
                    }
                }
                i = j;
            }
            _ => {
                out.push(messages[i].clone());
                i += 1;
            }
        }
    }
    *messages = out;
}

/// Align a keep-window start so it never lands inside a tool-call group.
fn align_keep_start(messages: &[Message], mut keep_start: usize) -> usize {
    if keep_start == 0 {
        return 1;
    }
    if keep_start >= messages.len() {
        return messages.len().max(1);
    }
    // Walk back over tool results to the assistant that issued them.
    while keep_start > 1 && messages[keep_start].role == Role::Tool {
        keep_start -= 1;
    }
    keep_start.max(1)
}

/// Check if the session's message history exceeds the configured token budget.
/// If so, compact older messages into a summary (heuristic only — for tests).
///
/// Prefer [`maybe_compact_llm`] in the live agent loop.
pub fn maybe_compact(session: &mut Session, config: &ContextConfig) -> bool {
    let Some((keep_start, draft)) = plan_compact(session, config) else {
        repair_message_sequence(&mut session.messages);
        return false;
    };
    apply_compact(session, keep_start, &draft);
    true
}

/// Compact with optional LLM rewrite of the heuristic draft.
pub async fn maybe_compact_llm(
    session: &mut Session,
    config: &ContextConfig,
    llm: Option<CompactLlm<'_>>,
) -> bool {
    let Some((keep_start, draft)) = plan_compact(session, config) else {
        repair_message_sequence(&mut session.messages);
        return false;
    };

    let summary = if let Some(creds) = llm {
        match llm_rewrite_summary(&draft, creds).await {
            Ok(s) => {
                tracing::info!(chars = s.len(), "LLM context condensation ok");
                s
            }
            Err(e) => {
                tracing::warn!(%e, "LLM condensation failed; using heuristic draft");
                draft
            }
        }
    } else {
        draft
    };

    apply_compact(session, keep_start, &summary);
    true
}

fn plan_compact(session: &Session, config: &ContextConfig) -> Option<(usize, String)> {
    plan_compact_messages(&session.messages, config)
}

fn plan_compact_messages(messages: &[Message], config: &ContextConfig) -> Option<(usize, String)> {
    if messages.len() <= config.keep_recent + 2 {
        return None;
    }
    let estimated_tokens = tokens::estimate_messages(messages);
    if estimated_tokens <= config.max_tokens {
        return None;
    }
    let mut keep_start = messages.len().saturating_sub(config.keep_recent);
    keep_start = align_keep_start(messages, keep_start);
    let to_summarize = &messages[1..keep_start];
    if to_summarize.is_empty() {
        return None;
    }
    tracing::info!(
        estimated_tokens,
        msg_count = messages.len(),
        compact_count = to_summarize.len(),
        "compacting conversation context"
    );
    Some((keep_start, build_summary(to_summarize)))
}

/// Apply a precomputed summary (e.g. soft-compact candidate) as a hard compact.
pub fn apply_compact_with_summary(
    session: &mut Session,
    summary: &str,
    keep_recent: usize,
) -> bool {
    if session.messages.len() <= keep_recent + 2 {
        return false;
    }
    let mut keep_start = session.messages.len().saturating_sub(keep_recent);
    keep_start = align_keep_start(&session.messages, keep_start);
    if keep_start <= 1 {
        return false;
    }
    apply_compact(session, keep_start, summary);
    true
}

fn apply_compact(session: &mut Session, keep_start: usize, summary: &str) {
    let parent_epoch = session.epoch;
    // Layering archive: the removed range (index 0 is the system prompt).
    let removed: Vec<Message> = session.messages[1..keep_start].to_vec();
    let mut compacted: Vec<Message> =
        Vec::with_capacity(2 + session.messages.len().saturating_sub(keep_start));
    compacted.push(session.messages[0].clone());
    compacted.push(Message {
        role: Role::User,
        content: format!(
            "[Earlier conversation — condensed]\n\
             The following is a summary of earlier messages that have been \
             compacted to save context space. Use this for continuity.\n\n{summary}"
        )
        .into(),
        tool_calls: None,
        tool_call_id: None,
    });
    compacted.extend_from_slice(&session.messages[keep_start..]);
    repair_message_sequence(&mut compacted);
    session.messages = compacted;
    // ADR-036: bump epoch on hard compact so callers can commit a Checkpoint.
    session.epoch = parent_epoch.saturating_add(1);
    // ADR-036 ext: archive the removed turns into the warm/cold layers.
    if let Some(lm) = session.layers.as_mut() {
        lm.push_removed_range(&removed);
    }
}

/// Summary text embedded in the condensed user message (if present).
pub fn condensed_summary_text(session: &Session) -> Option<&str> {
    session.messages.iter().find_map(|m| {
        let text = m.content.text();
        if m.role != Role::User || !text.starts_with("[Earlier conversation — condensed]") {
            return None;
        }
        // Prefer text after the boilerplate blank line.
        if let Some(idx) = text.find("\n\n") {
            let rest = text[idx + 2..].trim();
            if !rest.is_empty() {
                return Some(rest);
            }
        }
        Some(text)
    })
}

async fn llm_rewrite_summary(draft: &str, creds: CompactLlm<'_>) -> anyhow::Result<String> {
    let system = "You compress coding-agent chat history for continuity. \
         Keep: user goals, decisions, files touched, important tool outcomes, open questions. \
         Drop raw file dumps and repetitive tool noise. \
         Write a tight bullet summary (max ~600 words). \
         Match the language of the draft (Chinese if the draft is mostly Chinese). \
         Output only the summary — no preamble.";
    let messages = vec![
        Message {
            role: Role::System,
            content: system.into(),
            tool_calls: None,
            tool_call_id: None,
        },
        Message {
            role: Role::User,
            content: format!("Compress this draft history:\n\n{draft}").into(),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let req = LlmRequest {
        api_base: creds.api_base.to_string(),
        model: creds.model.to_string(),
        api_key: creds.api_key.to_string(),
        messages,
        max_tokens: 1024,
        tools: None,
    };
    let out = tokio::time::timeout(std::time::Duration::from_secs(40), llm::complete_chat(req))
        .await
        .map_err(|_| anyhow::anyhow!("condensation timed out"))??;
    Ok(out)
}

/// Build a terse summary from condensed messages.
/// Each message is truncated to a short snippet with a role label.
fn build_summary(messages: &[Message]) -> String {
    let mut s = String::with_capacity(messages.len() * 256);
    s.push_str("## Condensed Messages\n\n");

    for msg in messages {
        let role_label = match msg.role {
            Role::System => "System",
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::Tool => "Tool",
        };

        // Trim tool results aggressively — they're often verbose
        let max_chars = if msg.role == Role::Tool { 80 } else { 200 };

        let snippet: String = msg.content.text().chars().take(max_chars).collect();
        let dotdot = if msg.content.text().chars().count() > max_chars {
            "…"
        } else {
            ""
        };

        s.push_str(&format!("[{role_label}] {snippet}{dotdot}\n\n"));
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{FunctionCall, ToolCall};

    fn make_msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.to_string().into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn tool_call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: name.into(),
                arguments: "{}".into(),
            },
        }
    }

    #[test]
    fn no_compact_when_under_limit() {
        let mut session = Session::new("You are a helpful assistant.");
        session.add_user_message("Hello");
        session.add_assistant_message("Hi there!");

        let config = ContextConfig::default();
        assert!(!maybe_compact(&mut session, &config));
        assert_eq!(session.messages.len(), 3); // system + user + assistant
    }

    #[test]
    fn compact_when_over_limit() {
        let mut session = Session::new("You are helpful.");
        // Add enough messages to trigger compaction with a small config
        let config = ContextConfig {
            max_tokens: 1, // force compaction
            keep_recent: 2,
        };
        for i in 0..10 {
            session.add_user_message(&format!("Message {i}"));
            session.add_assistant_message(&format!("Response {i}"));
        }

        let original_len = session.messages.len(); // 21: system + 20
        assert!(maybe_compact(&mut session, &config));
        // After compaction: system + summary + keep_recent messages
        assert!(session.messages.len() < original_len);
        // System prompt should be first
        assert_eq!(session.messages[0].role, Role::System);
        assert_eq!(session.epoch, 1);
        assert!(condensed_summary_text(&session).is_some());
    }

    #[test]
    fn keep_recent_preserved() {
        let mut session = Session::new("You are a helpful coding assistant.");
        let pad = "x".repeat(100);
        for i in 0..20 {
            session.add_user_message(&format!("Question {i}: {pad}"));
            session.add_assistant_message(&format!("Answer {i}: {pad}"));
        }
        let last_content = session.messages.last().unwrap().content.clone();
        assert!(last_content.contains("Answer 19"));

        let config = ContextConfig {
            max_tokens: 10,
            keep_recent: 6,
        };
        assert!(maybe_compact(&mut session, &config));
        assert_eq!(session.messages.last().unwrap().content, last_content);
    }

    #[test]
    fn estimate_tokens_roughly_correct() {
        let msgs = vec![make_msg(
            Role::User,
            "This is a test message with forty chars.",
        )];
        let n = tokens::estimate_messages(&msgs);
        assert!(n >= 5 && n <= 20);
    }

    #[test]
    fn sanitize_drops_orphan_tool_messages() {
        let mut msgs = vec![
            make_msg(Role::System, "sys"),
            make_msg(Role::User, "hi"),
            Message {
                role: Role::Tool,
                content: "orphan".into(),
                tool_calls: None,
                tool_call_id: Some("call_x".into()),
            },
            make_msg(Role::Assistant, "ok"),
        ];
        sanitize_tool_pairing(&mut msgs);
        assert_eq!(msgs.len(), 3);
        assert!(msgs.iter().all(|m| m.role != Role::Tool));
    }

    #[test]
    fn sanitize_keeps_complete_tool_block() {
        let mut msgs = vec![
            make_msg(Role::System, "sys"),
            make_msg(Role::User, "list"),
            Message {
                role: Role::Assistant,
                content: String::new().into(),
                tool_calls: Some(vec![tool_call("c1", "list_directory")]),
                tool_call_id: None,
            },
            Message {
                role: Role::Tool,
                content: "ok".into(),
                tool_calls: None,
                tool_call_id: Some("c1".into()),
            },
        ];
        sanitize_tool_pairing(&mut msgs);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[2].role, Role::Assistant);
        assert_eq!(msgs[3].role, Role::Tool);
    }

    #[test]
    fn sanitize_stubs_missing_tool_results() {
        let mut msgs = vec![
            make_msg(Role::System, "sys"),
            Message {
                role: Role::Assistant,
                content: String::new().into(),
                tool_calls: Some(vec![tool_call("c1", "a"), tool_call("c2", "b")]),
                tool_call_id: None,
            },
            Message {
                role: Role::Tool,
                content: "only one".into(),
                tool_calls: None,
                tool_call_id: Some("c1".into()),
            },
            make_msg(Role::User, "next"),
        ];
        sanitize_tool_pairing(&mut msgs);
        assert_eq!(msgs.len(), 5);
        assert_eq!(msgs[1].role, Role::Assistant);
        assert_eq!(msgs[2].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(msgs[3].tool_call_id.as_deref(), Some("c2"));
        assert!(msgs[3].content.contains("unavailable"));
        assert_eq!(msgs[4].role, Role::User);
    }

    #[test]
    fn merge_consecutive_assistants_before_tools() {
        let mut msgs = vec![
            make_msg(Role::System, "sys"),
            make_msg(Role::User, "go"),
            make_msg(Role::Assistant, "thinking"),
            Message {
                role: Role::Assistant,
                content: "calling".into(),
                tool_calls: Some(vec![tool_call("c1", "list_directory")]),
                tool_call_id: None,
            },
            Message {
                role: Role::Tool,
                content: "ok".into(),
                tool_calls: None,
                tool_call_id: Some("c1".into()),
            },
        ];
        repair_message_sequence(&mut msgs);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[2].role, Role::Assistant);
        assert!(msgs[2].content.contains("thinking"));
        assert!(msgs[2].content.contains("calling"));
        assert_eq!(msgs[2].tool_calls.as_ref().unwrap().len(), 1);
        assert_eq!(msgs[3].role, Role::Tool);
    }

    #[test]
    fn compact_does_not_orphan_tool_results() {
        let mut session = Session::new("sys");
        let pad = "x".repeat(200);
        for i in 0..8 {
            session.add_user_message(&format!("Q{i} {pad}"));
            session.add_assistant_message(&format!("A{i} {pad}"));
        }
        session.add_assistant_tool_calls(
            String::new(),
            vec![
                tool_call("t1", "list_directory"),
                tool_call("t2", "read_file"),
            ],
        );
        session.add_tool_result("t1".into(), format!("dir {pad}"));
        session.add_tool_result("t2".into(), format!("file {pad}"));

        let config = ContextConfig {
            max_tokens: 10,
            keep_recent: 3, // would naively cut into the tool block
        };
        let _ = maybe_compact(&mut session, &config);
        // Must not leave a bare tool message without its assistant tool_calls.
        for (i, m) in session.messages.iter().enumerate() {
            if m.role == Role::Tool {
                assert!(
                    i > 0 && session.messages[i - 1].role == Role::Assistant
                        || session.messages[..i]
                            .iter()
                            .rev()
                            .find(|x| x.role != Role::Tool)
                            .is_some_and(|x| {
                                x.role == Role::Assistant
                                    && x.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
                            }),
                    "orphan tool at index {i}"
                );
            }
        }
        sanitize_tool_pairing(&mut session.messages);
        assert!(
            session
                .messages
                .iter()
                .filter(|m| m.role == Role::Tool)
                .count()
                == 0
                || session.messages.iter().any(|m| {
                    m.role == Role::Assistant
                        && m.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
                })
        );
    }

    #[test]
    fn apply_compact_with_summary_bumps_epoch() {
        let mut session = Session::new("system");
        for i in 0..30 {
            session.add_user_message(&format!("u{i}"));
            session.add_assistant_message(&format!("a{i}"));
        }
        assert_eq!(session.epoch, 0);
        assert!(apply_compact_with_summary(
            &mut session,
            "- goal: keep going",
            20
        ));
        assert_eq!(session.epoch, 1);
        assert!(
            session
                .messages
                .iter()
                .any(|m| { m.role == Role::User && m.content.contains("condensed") })
        );
    }

    #[test]
    fn soft_compact_state_invalidate_clears_ready() {
        let state = SoftCompactState::new();
        *state.ready.lock().unwrap() = Some(SoftCompactCandidate {
            parent_epoch: 0,
            summary: "x".into(),
        });
        state.invalidate();
        assert!(state.take_ready(0).is_none());
    }

    #[test]
    fn apply_compact_archives_removed_turns_into_layers() {
        let mut session = Session::new("system");
        for i in 0..10 {
            session.add_user_message(&format!("u{i}"));
            session.add_assistant_message(&format!("- a{i}"));
        }
        assert_eq!(session.epoch, 0);
        assert!(apply_compact_with_summary(
            &mut session,
            "- goal: keep going",
            2
        ));
        assert_eq!(session.epoch, 1);
        let lm = session.layers.as_ref().unwrap();
        assert!(!lm.warm.is_empty());
        // The condensed summary itself must not be archived.
        assert!(lm.warm.iter().all(|w| !w.user_goal.contains("condensed")));
        // Archiving keeps the oldest turn first.
        assert_eq!(lm.warm[0].user_goal, "u0");
    }

    #[test]
    fn apply_compact_under_guard_leaves_layers_untouched() {
        let mut session = Session::new("system");
        session.add_user_message("u");
        assert!(!apply_compact_with_summary(&mut session, "- goal: x", 20));
        assert_eq!(session.epoch, 0);
        assert!(session.layers.as_ref().unwrap().warm.is_empty());
    }
}
