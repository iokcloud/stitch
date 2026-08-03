//! Plan mode — structured task planning before execution.
//!
//! When plan mode is enabled, the agent first asks the LLM to produce
//! a structured execution plan. The plan is presented to the user for
//! review and approval. Once approved, the agent executes each step
//! and reports progress.
//!
//! ## Flow
//!
//! 1. User provides a task → LLM generates numbered steps
//! 2. Agent emits `AgentEvent::PlanProposed` → UI shows plan for review
//! 3. User approves/modifies/rejects → agent receives confirmation
//! 4. Agent iterates through steps, emitting `PlanStepStart`/`PlanStepDone`
//! 5. On completion, emits `AgentEvent::Done`

use serde::{Deserialize, Serialize};

/// A single step in an execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// Human-readable description of what to do.
    pub description: String,
    /// Current status.
    pub status: PlanStepStatus,
}

/// Status of a plan step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanStepStatus {
    Pending,
    InProgress,
    Done,
    Skipped,
}

/// A complete execution plan with an optional title.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Optional title for the plan.
    pub title: Option<String>,
    /// Ordered list of steps.
    pub steps: Vec<PlanStep>,
}

impl Plan {
    /// Create a new empty plan.
    pub fn new() -> Self {
        Self {
            title: None,
            steps: Vec::new(),
        }
    }

    /// Create a plan from parsed LLM output.
    ///
    /// Expects the LLM to output steps in a numbered format like:
    /// ```text
    /// PLAN:
    /// ## Title (optional)
    /// 1. First step description
    /// 2. Second step description
    /// ```
    pub fn from_llm_output(output: &str) -> Self {
        let mut plan = Self::new();
        let mut in_plan = false;

        for line in output.lines() {
            let trimmed = line.trim();

            // Detect plan start
            if trimmed.eq_ignore_ascii_case("PLAN:")
                || trimmed.eq_ignore_ascii_case("## Plan")
                || trimmed.eq_ignore_ascii_case("### Plan")
            {
                in_plan = true;
                continue;
            }

            if !in_plan {
                continue;
            }

            // Detect title (## Title)
            if let Some(title) = trimmed.strip_prefix("## ") {
                if !title.is_empty()
                    && !title.eq_ignore_ascii_case("Plan")
                    && !title.eq_ignore_ascii_case("Steps")
                {
                    plan.title = Some(title.to_string());
                }
                continue;
            }

            // Detect numbered steps: "1. Description" or "1) Description" or "- [ ] 1. Description"
            let step_text = if let Some(rest) = trimmed
                .strip_prefix("- [ ] ")
                .or_else(|| trimmed.strip_prefix("- "))
            {
                rest.trim().to_string()
            } else {
                trimmed.to_string()
            };

            if let Some(desc) = parse_numbered_step(&step_text) {
                plan.steps.push(PlanStep {
                    description: desc,
                    status: PlanStepStatus::Pending,
                });
            }
        }

        // If no explicit plan markers, try to parse numbered lines directly
        if plan.steps.is_empty() {
            for line in output.lines() {
                let trimmed = line.trim();
                if let Some(desc) = parse_numbered_step(trimmed) {
                    plan.steps.push(PlanStep {
                        description: desc,
                        status: PlanStepStatus::Pending,
                    });
                }
            }
        }

        plan
    }

    /// Check if the plan has any executable steps.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Mark a step as in progress.
    pub fn start_step(&mut self, index: usize) -> Option<&str> {
        self.steps.get_mut(index).map(|step| {
            step.status = PlanStepStatus::InProgress;
            step.description.as_str()
        })
    }

    /// Mark a step as done.
    pub fn complete_step(&mut self, index: usize) {
        if let Some(step) = self.steps.get_mut(index) {
            step.status = PlanStepStatus::Done;
        }
    }

    /// Count remaining steps.
    pub fn remaining(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.status == PlanStepStatus::Pending)
            .count()
    }

    /// Format the plan for display.
    pub fn format(&self) -> String {
        let mut s = String::new();
        if let Some(ref title) = self.title {
            s.push_str(&format!("## {title}\n\n"));
        }
        for (i, step) in self.steps.iter().enumerate() {
            let status_icon = match step.status {
                PlanStepStatus::Done => "[x]",
                PlanStepStatus::InProgress => "[>]",
                PlanStepStatus::Pending => "[ ]",
                PlanStepStatus::Skipped => "[~]",
            };
            s.push_str(&format!("{status_icon} {}. {}\n", i + 1, step.description));
        }
        s
    }
}

impl Default for Plan {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a numbered step like "1. Do something" or "1) Do something".
fn parse_numbered_step(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    // Consume digits
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }

    // Expect a separator: "." or ")" or ":"
    if i < chars.len() && (chars[i] == '.' || chars[i] == ')' || chars[i] == ':') {
        i += 1;
        // Optional space
        if i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        let desc: String = chars[i..].iter().collect();
        let desc = desc.trim().to_string();
        if !desc.is_empty() {
            return Some(desc);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_numbered_list() {
        let output = "1. Install dependencies\n2. Run tests\n3. Deploy to staging";
        let plan = Plan::from_llm_output(output);
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].description, "Install dependencies");
        assert_eq!(plan.steps[1].description, "Run tests");
        assert_eq!(plan.steps[2].description, "Deploy to staging");
    }

    #[test]
    fn parse_with_plan_marker() {
        let output = "PLAN:\n1. Step one\n2. Step two";
        let plan = Plan::from_llm_output(output);
        assert_eq!(plan.steps.len(), 2);
    }

    #[test]
    fn parse_with_paren_separator() {
        let output = "1) First thing\n2) Second thing";
        let plan = Plan::from_llm_output(output);
        assert_eq!(plan.steps.len(), 2);
    }

    #[test]
    fn parse_mixed_content() {
        let output = "Here's my analysis.\n\nPLAN:\n1. Fix the bug in auth.rs\n2. Add tests\n\nThis should work.";
        let plan = Plan::from_llm_output(output);
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].description, "Fix the bug in auth.rs");
    }

    #[test]
    fn empty_output_gives_empty_plan() {
        let plan = Plan::from_llm_output("Just some text with no numbers.");
        assert!(plan.is_empty());
    }

    #[test]
    fn step_lifecycle() {
        let mut plan = Plan::from_llm_output("1. Step A\n2. Step B\n3. Step C");
        assert_eq!(plan.remaining(), 3);

        plan.start_step(0);
        assert_eq!(plan.steps[0].status, PlanStepStatus::InProgress);
        assert_eq!(plan.remaining(), 2);

        plan.complete_step(0);
        assert_eq!(plan.steps[0].status, PlanStepStatus::Done);
        assert_eq!(plan.remaining(), 2);

        plan.start_step(1);
        plan.complete_step(1);
        plan.start_step(2);
        plan.complete_step(2);
        assert_eq!(plan.remaining(), 0);
    }

    #[test]
    fn format_plan() {
        let mut plan = Plan::from_llm_output("1. Do A\n2. Do B");
        plan.title = Some("My Plan".into());
        plan.start_step(0);
        plan.complete_step(0);

        let formatted = plan.format();
        assert!(formatted.contains("## My Plan"));
        assert!(formatted.contains("[x] 1. Do A"));
        assert!(formatted.contains("[ ] 2. Do B"));
    }

    #[test]
    fn sanitize_caps_and_drops_empty() {
        let mut steps = Vec::new();
        for i in 1..=12 {
            steps.push(PlanStep {
                description: format!("Step {i}"),
                status: PlanStepStatus::Pending,
            });
        }
        steps.push(PlanStep {
            description: "   ".into(),
            status: PlanStepStatus::Pending,
        });
        let plan = sanitize_plan(Plan { title: None, steps });
        assert_eq!(plan.steps.len(), MAX_PLAN_STEPS);
        assert!(plan.steps.iter().all(|s| !s.description.trim().is_empty()));
    }

    #[test]
    fn plan_task_hint_detects_review() {
        assert!(plan_task_hint("审查本次改动").contains("review"));
    }
}

// ── Plan Mode Orchestration ────────────────────────────────────────

/// Max steps kept after sanitization (long plans become unstable).
pub const MAX_PLAN_STEPS: usize = 8;

/// Instruction suffix appended when asking the model to draft a plan.
pub const PLAN_INSTRUCTION: &str = "\n\nBefore taking any action, output a numbered execution plan ONLY.\n\
Use this exact format:\n\
PLAN:\n\
## Title\n\
1. First concrete step\n\
2. Second concrete step\n\
...\n\n\
Rules for the plan:\n\
- 3 to 8 steps (prefer fewer).\n\
- Each step is one actionable unit (one file group, one verify, or one review phase).\n\
- For code changes: include verify (build/test) as a late step when relevant.\n\
- For reviewing diffs: steps should be git_status → git_diff (scoped) → spot-read → conclude.\n\
- Do not include vague steps like \"investigate\" without a target.\n\
- Do not call tools yet — plan text only.";

/// Soft hint prepended based on the user task (keeps plan generation on rails).
pub fn plan_task_hint(user_message: &str) -> &'static str {
    let m = user_message.to_ascii_lowercase();
    let zh_review = user_message.contains("审查")
        || user_message.contains("改动")
        || user_message.contains("评审");
    if m.contains("review") || m.contains("diff") || zh_review {
        return "Task type: code review. Prefer git_status / git_diff first, then spot-read, then a structured conclusion.";
    }
    if m.contains("multi")
        || m.contains("several file")
        || m.contains("多个文件")
        || m.contains("复杂")
        || user_message.lines().count() >= 4
    {
        return "Task type: multi-step / multi-file work. One coherent deliverable per step; end with verify if possible.";
    }
    "Task type: general. Keep the plan short and executable."
}

/// Drop empty steps, trim, and cap length so plan mode stays executable.
pub fn sanitize_plan(mut plan: Plan) -> Plan {
    plan.steps.retain(|s| !s.description.trim().is_empty());
    for step in &mut plan.steps {
        let t = step.description.trim();
        step.description = t.chars().take(240).collect();
    }
    if plan.steps.len() > MAX_PLAN_STEPS {
        plan.steps.truncate(MAX_PLAN_STEPS);
        if plan.title.is_none() {
            plan.title = Some("执行计划".into());
        }
    }
    plan
}

/// User message that scopes the agent to a single approved plan step.
pub fn step_execution_prompt(index: usize, total: usize, description: &str) -> String {
    format!(
        "Execute ONLY step {num}/{total} of the approved plan. Do not start later steps.\n\
         Step {num}: {description}\n\
         When this step is complete, stop and briefly state what you did for this step only.",
        num = index + 1,
        total = total,
        description = description.trim()
    )
}

/// Final wrap-up after all plan steps ran.
pub fn plan_summary_prompt() -> &'static str {
    "All plan steps were executed in order. Give a brief final summary: what changed, \
     what was verified, and any remaining risks. Do not start a new large tool campaign."
}

/// 自动规划判断：轻量 LLM 调用决定任务是否需要先规划。
/// 三态「自动」模式下用——多步骤/有副作用（写文件、跑命令、改多处）
/// → true 走规划；简单问答/单步 → false 直接执行。
pub async fn needs_plan(
    session: &crate::session::Session,
    api_base: &str,
    model: &str,
    api_key: &str,
) -> anyhow::Result<bool> {
    use crate::llm::{self, LlmRequest, StreamEvent};
    use crate::session::{Message, Role};

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let user_task = session
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
        .map(|m| m.content.text())
        .unwrap_or("");

    let mut messages = session.messages.clone();
    messages.push(Message {
        role: Role::User,
        content: crate::session::Content::Text(format!(
            "任务：{user_task}

判断这个任务是否需要先规划再执行。需要规划的情况：涉及多个步骤、需要写文件、运行命令、修改多处代码、有副作用的操作。不需要规划的情况：简单问答、单个明确操作、仅解释说明。
只回答 true 或 false，不要其他内容。"
        )),
        tool_calls: None,
        tool_call_id: None,
    });

    let request = LlmRequest {
        api_base: api_base.to_string(),
        model: model.to_string(),
        api_key: api_key.to_string(),
        messages,
        max_tokens: 16,
        tools: None,
    };

    let llm_handle = tokio::spawn(async move {
        let _ = llm::stream_chat(request, tx).await;
    });
    let mut text = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            StreamEvent::Token(t) => text.push_str(&t),
            StreamEvent::Done => break,
            _ => {}
        }
    }
    let _ = llm_handle.await;
    Ok(text.trim().to_lowercase().contains("true"))
}

/// Generate a plan from the LLM based on the current session state.
///
/// This makes a lightweight, tools-disabled LLM call asking for a plan.
/// Returns the parsed `Plan`.
pub async fn generate_plan(
    session: &crate::session::Session,
    api_base: &str,
    model: &str,
    api_key: &str,
) -> anyhow::Result<Plan> {
    use crate::llm::{self, LlmRequest, StreamEvent};
    use crate::session::{Message, Role};

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let user_task = session
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
        .map(|m| m.content.text())
        .unwrap_or("");

    let mut messages = session.messages.clone();
    messages.push(Message {
        role: Role::User,
        content: format!(
            "{}\n\n{}",
            plan_task_hint(user_task),
            PLAN_INSTRUCTION.trim()
        )
        .into(),
        tool_calls: None,
        tool_call_id: None,
    });

    let request = LlmRequest {
        api_base: api_base.to_string(),
        model: model.to_string(),
        api_key: api_key.to_string(),
        messages,
        max_tokens: 2048,
        tools: None, // No tools for plan generation
    };

    tokio::spawn(async move {
        let _ = llm::stream_chat(request, tx).await;
    });

    let mut text = String::new();
    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::Token(t) => text.push_str(&t),
            StreamEvent::ToolCall { .. } => {}
            StreamEvent::Done => break,
            StreamEvent::Error(e) => {
                tracing::warn!(%e, "plan generation LLM error");
            }
        }
    }

    Ok(sanitize_plan(Plan::from_llm_output(&text)))
}
