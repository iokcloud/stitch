//! Hooks 系统（Claude Code 语义最小集）。
#![allow(clippy::disallowed_methods)] // json! 宏展开 + 测试 unwrap，项目惯例
//!
//! 六个事件：SessionStart / SessionEnd / UserPromptSubmit / PreToolUse /
//! PostToolUse / Stop。command 型 hook：子进程执行，输入 JSON 经 stdin 传入，
//! 输出经 stdout 读回（上限 1MB，与 Claude Code 对齐）。
//!
//! 约定（与 Claude Code 对齐）：
//! - 退出码 0 = 通过；退出码 2 = block（PreToolUse / UserPromptSubmit /
//!   SessionStart 生效，其余事件仅记录警告）
//! - PreToolUse 的 stdout JSON 可返回 `{"decision": "block", "reason": "…"}`
//!   显式拒绝；`"ask"` 降级为 approve（CLI 无询问 UI）
//! - UserPromptSubmit 的 stdout `hookSpecificOutput` 本版忽略
//!
//! 配置：`hooks.json`（全局 `config_dir/hooks.json` 兜底，工作区
//! `.stitch/hooks.json` 覆盖，同名事件两者都执行，工作区在后）：
//!
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse": [ { "matcher": "edit|write", "command": "node check.js" } ],
//!     "Stop": [ { "command": "echo turn done" } ]
//!   }
//! }
//! ```
//!
//! `matcher` 仅对 PreToolUse / PostToolUse 生效：`*` 通配，`|` 分隔多个模式。

use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

/// 退出码 2 = block（Claude Code 约定）。
const EXIT_BLOCK: i32 = 2;
/// stdout 读取上限（对齐 Claude Code 1MB）。
const MAX_HOOK_OUTPUT: usize = 1 << 20;

/// 单个 hook 定义。
#[derive(Debug, Clone, Deserialize)]
pub struct HookSpec {
    /// 工具名匹配模式（`*` 通配，`|` 分隔）。仅 PreToolUse/PostToolUse。
    #[serde(default)]
    pub matcher: Option<String>,
    /// 要执行的 shell 命令（如 `node scripts/check.js`）。
    pub command: String,
}

/// hooks.json 顶层：事件名 → hook 列表。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HooksFile {
    #[serde(rename = "PreToolUse", default)]
    pub pre_tool_use: Vec<HookSpec>,
    #[serde(rename = "PostToolUse", default)]
    pub post_tool_use: Vec<HookSpec>,
    #[serde(rename = "Stop", default)]
    pub stop: Vec<HookSpec>,
    #[serde(rename = "SessionStart", default)]
    pub session_start: Vec<HookSpec>,
    #[serde(rename = "SessionEnd", default)]
    pub session_end: Vec<HookSpec>,
    #[serde(rename = "UserPromptSubmit", default)]
    pub user_prompt_submit: Vec<HookSpec>,
    /// Notification——提示宿主显示一条消息（如 /cost 结果）。
    #[serde(rename = "Notification", default)]
    pub notification: Vec<HookSpec>,
    /// SubagentStop——子代理委派结束。
    #[serde(rename = "SubagentStop", default)]
    pub subagent_stop: Vec<HookSpec>,
    /// PreCompact——压缩前拦截（block 可拒绝压缩）。
    #[serde(rename = "PreCompact", default)]
    pub pre_compact: Vec<HookSpec>,
    /// PostToolUseFailure——工具调用失败后（通知型；matcher 匹配工具名）。
    #[serde(rename = "PostToolUseFailure", default)]
    pub post_tool_use_failure: Vec<HookSpec>,
    /// PermissionRequest——权限确认请求发出前（通知型；matcher 匹配工具名，
    /// 可接程序化审批/审计日志，不改变批准决策）。
    #[serde(rename = "PermissionRequest", default)]
    pub permission_request: Vec<HookSpec>,
}

/// Hook 事件（十一个，与 Claude Code 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    Stop,
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    Notification,
    SubagentStop,
    PreCompact,
    PostToolUseFailure,
    PermissionRequest,
}

impl HookEvent {
    pub fn name(&self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::Stop => "Stop",
            HookEvent::SessionStart => "SessionStart",
            HookEvent::SessionEnd => "SessionEnd",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::Notification => "Notification",
            HookEvent::SubagentStop => "SubagentStop",
            HookEvent::PreCompact => "PreCompact",
            HookEvent::PostToolUseFailure => "PostToolUseFailure",
            HookEvent::PermissionRequest => "PermissionRequest",
        }
    }

    fn specs<'a>(&self, file: &'a HooksFile) -> &'a [HookSpec] {
        match self {
            HookEvent::PreToolUse => &file.pre_tool_use,
            HookEvent::PostToolUse => &file.post_tool_use,
            HookEvent::Stop => &file.stop,
            HookEvent::SessionStart => &file.session_start,
            HookEvent::SessionEnd => &file.session_end,
            HookEvent::UserPromptSubmit => &file.user_prompt_submit,
            HookEvent::Notification => &file.notification,
            HookEvent::SubagentStop => &file.subagent_stop,
            HookEvent::PreCompact => &file.pre_compact,
            HookEvent::PostToolUseFailure => &file.post_tool_use_failure,
            HookEvent::PermissionRequest => &file.permission_request,
        }
    }

    /// 该事件带 block 语义（退出码 2 生效）。
    fn blocks(&self) -> bool {
        matches!(
            self,
            HookEvent::PreToolUse
                | HookEvent::UserPromptSubmit
                | HookEvent::SessionStart
                | HookEvent::PreCompact
        )
    }

    /// 按工具名匹配（matcher 仅对工具事件有意义）。
    fn is_tool_event(&self) -> bool {
        matches!(
            self,
            HookEvent::PreToolUse
                | HookEvent::PostToolUse
                | HookEvent::PostToolUseFailure
                | HookEvent::PermissionRequest
        )
    }
}

/// PreCompact 检查：压缩前调用，返回 None = 放行；Some(原因) = 被 hook 拒绝。
pub async fn pre_compact_blocked(
    work_dir: Option<&str>,
    message_count: usize,
    estimated_tokens: usize,
) -> Option<String> {
    let wd = work_dir?;
    let hooks = HookRegistry::load(Some(wd));
    if !hooks.has(HookEvent::PreCompact) {
        return None;
    }
    // 显式构造（json! 宏对数字变量的 to_value().unwrap() 触发 clippy 禁用项）
    let mut payload = serde_json::Map::new();
    payload.insert(
        "message_count".to_string(),
        serde_json::Value::from(message_count as u64),
    );
    payload.insert(
        "estimated_tokens".to_string(),
        serde_json::Value::from(estimated_tokens as u64),
    );
    let outcome = hooks
        .run(
            HookEvent::PreCompact,
            wd,
            &serde_json::Value::Object(payload),
            None,
        )
        .await;
    outcome.blocked
}

/// 一次事件执行的汇总结果。
#[derive(Debug, Clone, Default)]
pub struct HookOutcome {
    /// block 原因（Some = 该 hook 拒绝了动作）。
    pub blocked: Option<String>,
    /// 各 hook stdout 拼接（调试用）。
    pub output: String,
}

/// 全局 + 工作区合并的 hook 注册表。
#[derive(Debug, Clone, Default)]
pub struct HookRegistry {
    global: HooksFile,
    workspace: HooksFile,
}

impl HookRegistry {
    /// 加载全局（config_dir/hooks.json）+ 工作区（.stitch/hooks.json）。
    pub fn load(work_dir: Option<&str>) -> Self {
        let mut r = Self::default();
        if let Some(h) = load_hooks_file(&crate::config::config_dir().join("hooks.json")) {
            r.global = h;
        }
        if let Some(h) = work_dir
            .and_then(|wd| load_hooks_file(&PathBuf::from(wd).join(".stitch").join("hooks.json")))
        {
            r.workspace = h;
        }
        r
    }

    /// 该事件是否配置了 hook（无 hook 时零开销路径）。
    pub fn has(&self, event: HookEvent) -> bool {
        !event.specs(&self.global).is_empty() || !event.specs(&self.workspace).is_empty()
    }

    /// 查看当前生效配置（供 /hooks 显示）：全局与工作区 hooks 文件。
    pub fn inspect(&self) -> (&HooksFile, &HooksFile) {
        (&self.global, &self.workspace)
    }

    /// 运行事件全部 hooks（全局先、工作区后；block 后停止）。
    ///
    /// - `session_id`：会话空间标识（CLI 传真实会话 id；agent 循环传 work_dir）
    /// - `input`：业务输入（PreToolUse 传 tool_input；PostToolUse 传
    ///   `{"tool_input":…, "tool_response":…}`；其余传 `{"cwd":…, …}`）
    /// - `matcher_ctx`：工具名（仅工具事件使用）
    pub async fn run(
        &self,
        event: HookEvent,
        session_id: &str,
        input: &Value,
        matcher_ctx: Option<&str>,
    ) -> HookOutcome {
        let mut outcome = HookOutcome::default();
        for spec in event
            .specs(&self.global)
            .iter()
            .chain(event.specs(&self.workspace).iter())
        {
            if event.is_tool_event()
                && spec.matcher.as_deref().is_some_and(|m| {
                    let ctx = matcher_ctx.unwrap_or("");
                    !m.split('|').any(|p| glob_match(p.trim(), ctx))
                })
            {
                continue;
            }
            let payload = event_input(event, session_id, input, matcher_ctx);
            let one = run_one(&spec.command, &payload, event).await;
            if let Some(reason) = one.blocked {
                outcome.blocked = Some(format!("{reason} (hook: {})", spec.command));
                break; // block 后停止后续 hooks（Claude Code 行为）
            }
            if !one.output.is_empty() {
                if !outcome.output.is_empty() {
                    outcome.output.push('\n');
                }
                outcome.output.push_str(&one.output);
            }
        }
        outcome
    }
}

/// 汇总 hooks 的可读视图（/hooks 用）：每个事件的 hook 命令 + 来源。
pub fn summarize(global: &HooksFile, workspace: Option<&HooksFile>) -> String {
    let events = [
        HookEvent::PreToolUse,
        HookEvent::PostToolUse,
        HookEvent::Stop,
        HookEvent::SessionStart,
        HookEvent::SessionEnd,
        HookEvent::UserPromptSubmit,
        HookEvent::Notification,
        HookEvent::SubagentStop,
        HookEvent::PreCompact,
        HookEvent::PostToolUseFailure,
        HookEvent::PermissionRequest,
    ];
    let mut out = String::new();
    for ev in events {
        let g = ev.specs(global);
        let w = workspace.map(|f| ev.specs(f)).unwrap_or(&[]);
        if g.is_empty() && w.is_empty() {
            continue;
        }
        out.push_str(&format!("  {}：\n", ev.name()));
        for s in g {
            out.push_str(&format!("    [全局] {}\n", s.command));
        }
        for s in w {
            out.push_str(&format!("    [工作区] {}\n", s.command));
        }
    }
    if out.is_empty() {
        return "  （未配置任何 hooks）".to_string();
    }
    out.trim_end_matches('\n').to_string()
}

/// 组装传给 hook 的输入 JSON（Claude Code 字段风格）。
fn event_input(
    event: HookEvent,
    session_id: &str,
    input: &Value,
    matcher_ctx: Option<&str>,
) -> Value {
    let mut obj = Value::Object(serde_json::Map::new());
    obj["session_id"] = Value::String(session_id.to_string());
    match event {
        HookEvent::PreToolUse => {
            obj["tool_name"] = Value::String(matcher_ctx.unwrap_or("").to_string());
            obj["tool_input"] = input.clone();
        }
        HookEvent::PostToolUse => {
            obj["tool_name"] = Value::String(matcher_ctx.unwrap_or("").to_string());
            obj["tool_input"] = input.get("tool_input").cloned().unwrap_or(Value::Null);
            obj["tool_response"] = input.get("tool_response").cloned().unwrap_or(Value::Null);
        }
        HookEvent::PostToolUseFailure => {
            obj["tool_name"] = Value::String(matcher_ctx.unwrap_or("").to_string());
            obj["tool_input"] = input.get("tool_input").cloned().unwrap_or(Value::Null);
            obj["tool_response"] = input.get("tool_response").cloned().unwrap_or(Value::Null);
            obj["error"] = input.get("error").cloned().unwrap_or(Value::Null);
        }
        HookEvent::PermissionRequest => {
            obj["tool_name"] = Value::String(matcher_ctx.unwrap_or("").to_string());
            obj["message"] = input.get("message").cloned().unwrap_or(Value::Null);
        }
        HookEvent::Stop => {
            obj["stop_hook_active"] = Value::Bool(true);
            merge_input(&mut obj, input);
        }
        _ => merge_input(&mut obj, input),
    }
    obj
}

/// 把业务字段并入事件对象（Stop / SessionStart / SessionEnd / UserPromptSubmit）。
fn merge_input(obj: &mut Value, input: &Value) {
    for (k, v) in input.as_object().unwrap_or(&serde_json::Map::new()) {
        obj[k] = v.clone();
    }
}

/// 执行单个 hook 命令：stdin 写 JSON，读 stdout，判退出码。
async fn run_one(command: &str, input: &Value, event: HookEvent) -> HookOutcome {
    let mut outcome = HookOutcome::default();

    #[cfg(target_os = "windows")]
    let spawn = tokio::process::Command::new("cmd")
        .args(["/C", command])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    #[cfg(not(target_os = "windows"))]
    let spawn = tokio::process::Command::new("sh")
        .args(["-c", command])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let mut child = match spawn {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(hook = %command, error = %e, "failed to spawn hook");
            return outcome;
        }
    };

    // stdin 写入输入 JSON
    {
        use tokio::io::AsyncWriteExt;
        if let Some(mut stdin) = child.stdin.take() {
            let payload = serde_json::to_string(input).unwrap_or_else(|_| "{}".into());
            if let Err(e) = stdin.write_all(payload.as_bytes()).await {
                tracing::warn!(hook = %command, error = %e, "hook stdin write failed");
            }
            let _ = stdin.shutdown().await;
        }
    }

    match child.wait_with_output().await {
        Ok(out) => {
            let stdout = truncate_utf8(&out.stdout);
            let stderr = truncate_utf8(&out.stderr);
            if !stderr.trim().is_empty() {
                tracing::warn!(hook = %command, stderr = %stderr.trim(), "hook stderr");
            }
            outcome.output = stdout.trim().to_string();

            let code = out.status.code();
            if code == Some(EXIT_BLOCK) && event.blocks() {
                outcome.blocked = Some(
                    extract_block_reason(&stdout)
                        .unwrap_or_else(|| "blocked by hook (exit code 2)".to_string()),
                );
            } else if event == HookEvent::SessionStart && code.is_some_and(|c| c != 0) {
                // SessionStart 语义：非零退出码 = 拒绝启动
                outcome.blocked = Some("session start rejected by hook".to_string());
            } else if code.is_some_and(|c| c != 0) {
                tracing::warn!(
                    hook = %command,
                    exit = code.unwrap_or(0),
                    "hook exited non-zero"
                );
            }
        }
        Err(e) => {
            tracing::warn!(hook = %command, error = %e, "hook execution failed");
        }
    }
    outcome
}

/// 从 stdout JSON 提取 block 原因：`{"decision":"block","reason":"…"}`。
fn extract_block_reason(stdout: &str) -> Option<String> {
    let v: Value = serde_json::from_str(stdout).ok()?;
    if v.get("decision").and_then(|d| d.as_str()) == Some("block") {
        Some(
            v.get("reason")
                .and_then(|r| r.as_str())
                .unwrap_or("blocked")
                .to_string(),
        )
    } else {
        None
    }
}

/// 截断到 MAX_HOOK_OUTPUT 且落在 UTF-8 边界。
fn truncate_utf8(bytes: &[u8]) -> String {
    let end = bytes.len().min(MAX_HOOK_OUTPUT);
    match std::str::from_utf8(&bytes[..end]) {
        Ok(s) => s.to_string(),
        Err(e) => String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned(),
    }
}

/// 极简 glob：`*` 通配任意序列（含空）、`?` 通配单字符。
fn glob_match(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut star_ti): (Option<usize>, usize) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == b'*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if pi < p.len() && (p[pi] == b'?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// 读取单个 hooks.json 文件。
fn load_hooks_file(path: &std::path::Path) -> Option<HooksFile> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<HooksFile>(&text) {
        Ok(h) => {
            tracing::info!(path = %path.display(), "loaded hooks file");
            Some(h)
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "invalid hooks file, ignored");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summarize_lists_events_with_source() {
        let mut global = HooksFile::default();
        global.pre_tool_use.push(HookSpec {
            matcher: Some("edit|write".into()),
            command: "node check.js".into(),
        });
        global.stop.push(HookSpec {
            matcher: None,
            command: "echo turn done".into(),
        });
        let mut ws = HooksFile::default();
        ws.pre_compact.push(HookSpec {
            matcher: None,
            command: "node precompact.js".into(),
        });
        let text = summarize(&global, Some(&ws));
        assert!(text.contains("PreToolUse"), "{text}");
        assert!(text.contains("[全局] node check.js"), "{text}");
        assert!(text.contains("Stop"), "{text}");
        assert!(text.contains("PreCompact"), "{text}");
        assert!(text.contains("[工作区] node precompact.js"), "{text}");
        // 未配置的事件不出现
        assert!(!text.contains("SessionStart"), "{text}");
    }

    #[test]
    fn summarize_empty_reports_none() {
        let global = HooksFile::default();
        let text = summarize(&global, None);
        assert!(text.contains("未配置"), "{text}");
    }

    #[test]
    fn glob_star_matches_anything() {
        assert!(glob_match("*", "read_file"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn glob_prefix_and_suffix() {
        assert!(glob_match("edit*", "edit_file"));
        assert!(glob_match("*edit", "file_edit"));
        assert!(glob_match("*edit*", "x_edit_y"));
        assert!(!glob_match("edit*", "write_file"));
    }

    #[test]
    fn glob_question_matches_single() {
        assert!(glob_match("read_f?le", "read_file"));
        assert!(!glob_match("read_f?le", "read_fxile"));
    }

    #[test]
    fn glob_star_in_middle() {
        assert!(glob_match("run_*_cmd", "run_shell_cmd"));
        assert!(!glob_match("run_*_cmd", "run_shell"));
    }

    #[test]
    fn matcher_pipe_splits_patterns() {
        // glob 语义：`edit` 精确匹配，`edit*` 前缀匹配
        let m = "edit*|write*";
        assert!(m.split('|').any(|p| glob_match(p.trim(), "edit_file")));
        assert!(m.split('|').any(|p| glob_match(p.trim(), "write_file")));
        assert!(!m.split('|').any(|p| glob_match(p.trim(), "read_file")));
        // 无通配的精确匹配
        assert!(glob_match("edit_file", "edit_file"));
        assert!(!glob_match("edit", "edit_file"));
    }

    #[test]
    fn extract_decision_block_reason() {
        let s = r#"{"decision":"block","reason":"禁止写入 secrets"}"#;
        assert_eq!(extract_block_reason(s).as_deref(), Some("禁止写入 secrets"));
        let s2 = r#"{"decision":"approve"}"#;
        assert!(extract_block_reason(s2).is_none());
        let s3 = "not json";
        assert!(extract_block_reason(s3).is_none());
        // 退出码 2 但 stdout 无 reason → 默认文案
        assert_eq!(
            extract_block_reason("").unwrap_or("blocked by hook (exit code 2)".to_string()),
            "blocked by hook (exit code 2)"
        );
    }

    #[test]
    fn parse_hooks_file_with_matcher_and_plain() {
        let json = r#"{
            "PreToolUse": [ { "matcher": "edit|write", "command": "node check.js" } ],
            "Stop": [ { "command": "echo done" } ]
        }"#;
        let h: HooksFile = serde_json::from_str(json).unwrap();
        assert_eq!(h.pre_tool_use.len(), 1);
        assert_eq!(h.pre_tool_use[0].matcher.as_deref(), Some("edit|write"));
        assert_eq!(h.stop.len(), 1);
        assert!(h.post_tool_use.is_empty());
        // 未配置的事件默认空
        let h2: HooksFile = serde_json::from_str("{}").unwrap();
        assert!(h2.session_start.is_empty());
    }

    #[test]
    fn event_input_shapes() {
        // PreToolUse：tool_name + tool_input
        let v = event_input(
            HookEvent::PreToolUse,
            "s1",
            &json!({"path": "a.txt"}),
            Some("write_file"),
        );
        assert_eq!(v["session_id"], "s1");
        assert_eq!(v["tool_name"], "write_file");
        assert_eq!(v["tool_input"]["path"], "a.txt");

        // PostToolUse：tool_input + tool_response
        let v = event_input(
            HookEvent::PostToolUse,
            "s1",
            &json!({"tool_input": {"p": 1}, "tool_response": {"ok": true}}),
            Some("read_file"),
        );
        assert_eq!(v["tool_response"]["ok"], true);

        // UserPromptSubmit：业务字段并入
        let v = event_input(
            HookEvent::UserPromptSubmit,
            "s1",
            &json!({"prompt": "hi", "cwd": "/w"}),
            None,
        );
        assert_eq!(v["prompt"], "hi");
        assert_eq!(v["cwd"], "/w");

        // Notification：业务字段（message）并入
        let v = event_input(
            HookEvent::Notification,
            "s1",
            &json!({"message": "Cost: ¥0.12"}),
            None,
        );
        assert_eq!(v["message"], "Cost: ¥0.12");

        // SubagentStop：subagent 名并入
        let v = event_input(
            HookEvent::SubagentStop,
            "s1",
            &json!({"subagent": "reviewer", "success": true}),
            None,
        );
        assert_eq!(v["subagent"], "reviewer");

        // PreCompact：压缩上下文并入
        let v = event_input(
            HookEvent::PreCompact,
            "s1",
            &json!({"message_count": 120, "estimated_tokens": 80_000}),
            None,
        );
        assert_eq!(v["message_count"], 120);

        // Stop：stop_hook_active + transcript_path
        let v = event_input(
            HookEvent::Stop,
            "s1",
            &json!({"cwd": "/w", "transcript_path": "/w/m.jsonl"}),
            None,
        );
        assert_eq!(v["stop_hook_active"], true);
        assert_eq!(v["transcript_path"], "/w/m.jsonl");
    }

    #[tokio::test]
    async fn run_one_exit_zero_ok() {
        let outcome = run_one("echo hello", &json!({"a": 1}), HookEvent::Stop).await;
        assert!(outcome.blocked.is_none());
        assert_eq!(outcome.output.trim(), "hello");
    }

    #[tokio::test]
    async fn run_one_exit_two_blocks_pretool() {
        let outcome = run_one("exit 2", &json!({}), HookEvent::PreToolUse).await;
        assert!(outcome.blocked.is_some(), "exit 2 must block PreToolUse");
    }

    #[tokio::test]
    async fn run_one_exit_two_ignored_on_stop() {
        // Stop 非 block 事件：退出码 2 仅记录，不 block
        let outcome = run_one("exit 2", &json!({}), HookEvent::Stop).await;
        assert!(outcome.blocked.is_none());
    }

    #[tokio::test]
    async fn run_one_session_start_nonzero_blocks() {
        let outcome = run_one("exit 1", &json!({}), HookEvent::SessionStart).await;
        assert!(outcome.blocked.is_some());
        let outcome = run_one("exit 0", &json!({}), HookEvent::SessionStart).await;
        assert!(outcome.blocked.is_none());
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn run_one_stdin_payload_roundtrip_windows() {
        // findstr 原样回显 stdin；`.*` 匹配任意行
        let outcome = run_one(
            "findstr .*",
            &json!({"tool_name": "write_file"}),
            HookEvent::Stop,
        )
        .await;
        assert!(outcome.blocked.is_none());
        assert!(
            outcome.output.contains("tool_name"),
            "stdin 应经管道送达 hook: {}",
            outcome.output
        );
    }
}
