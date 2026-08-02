//! Local tools available to the agent.
//!
//! Each tool implements execution logic. The `Tool` enum provides
//! unified dispatch since `async fn` in traits is not dyn-compatible.
//!
//! `serde_json::json!` macro uses `unwrap()` internally — allowed.
#![allow(clippy::disallowed_methods)]

pub mod cdp;
pub mod cmd;
pub mod copy_path;
pub mod create_directory;
pub mod delete_path;
pub mod desktop;
pub mod edit_file;
pub mod file;
pub mod find_path;
pub mod git;
pub mod list_dir;
pub mod paths;
pub mod process_win;
pub mod save_skill;
pub mod search;
pub mod undo;
pub mod web_fetch;

use std::collections::HashMap;
use std::sync::Arc;

use crate::mcp_protocol::McpToolRuntime;

/// Description of a tool for the LLM's function calling schema.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Result of executing a tool.
#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    /// Optional per-tool metrics (duration_ms, etc.) for benchmarking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<HashMap<String, f64>>,
}

use serde::Serialize;

impl ToolResult {
    /// Convenience: successful result without metrics.
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            metrics: None,
        }
    }

    /// Convenience: failure result without metrics.
    pub fn fail(output: impl Into<String>) -> Self {
        Self {
            success: false,
            output: output.into(),
            metrics: None,
        }
    }

    /// Attach wall-clock duration (since `started`) as the `duration_ms`
    /// benchmark metric. Existing metrics are preserved.
    pub fn with_duration_ms(mut self, started: std::time::Instant) -> Self {
        self.metrics
            .get_or_insert_with(std::collections::HashMap::new)
            .insert(
                "duration_ms".into(),
                started.elapsed().as_secs_f64() * 1000.0,
            );
        self
    }
}

/// Remote MCP tool registered into the agent registry.
#[derive(Clone)]
pub struct McpRemoteTool {
    pub qualified_name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub runtime: McpToolRuntime,
}

/// Unified enum for all built-in tools.
#[derive(Clone)]
pub enum Tool {
    ListDirectory(list_dir::ListDirectory),
    ReadFile(file::ReadFile),
    WriteFile(file::WriteFile),
    EditFile(edit_file::EditFile),
    RunCommand(cmd::RunCommand),
    GrepSearch(search::GrepSearch),
    GitStatus(git::GitStatus),
    GitDiff(git::GitDiff),
    WebFetch(web_fetch::WebFetch),
    FindPath(find_path::FindPath),
    CreateDirectory(create_directory::CreateDirectory),
    DeletePath(delete_path::DeletePath),
    CopyPath(copy_path::CopyPath),
    UndoLastEdit(undo::UndoLastEdit),
    RedoLastEdit(undo::RedoLastEdit),
    DesktopScreenshot(desktop::DesktopScreenshot),
    DesktopClick(desktop::DesktopClick),
    DesktopType(desktop::DesktopType),
    DesktopKey(desktop::DesktopKey),
    DesktopScroll(desktop::DesktopScroll),
    DesktopHover(desktop::DesktopHover),
    DesktopWindowList(desktop::DesktopWindowList),
    DesktopWindowAction(desktop::DesktopWindowAction),
    DesktopBrowser(desktop::DesktopBrowser),
    DesktopAppLaunch(desktop::DesktopAppLaunch),
    SaveSkill(save_skill::SaveSkill),
    McpRemote(McpRemoteTool),
}

impl Tool {
    pub fn definition(&self) -> ToolDef {
        match self {
            Self::ListDirectory(t) => t.definition(),
            Self::ReadFile(t) => t.definition(),
            Self::WriteFile(t) => t.definition(),
            Self::EditFile(t) => t.definition(),
            Self::RunCommand(t) => t.definition(),
            Self::GrepSearch(t) => t.definition(),
            Self::GitStatus(t) => t.definition(),
            Self::GitDiff(t) => t.definition(),
            Self::WebFetch(t) => t.definition(),
            Self::FindPath(t) => t.definition(),
            Self::CreateDirectory(t) => t.definition(),
            Self::DeletePath(t) => t.definition(),
            Self::CopyPath(t) => t.definition(),
            Self::UndoLastEdit(t) => t.definition(),
            Self::RedoLastEdit(t) => t.definition(),
            Self::DesktopScreenshot(t) => t.definition(),
            Self::DesktopClick(t) => t.definition(),
            Self::DesktopType(t) => t.definition(),
            Self::DesktopKey(t) => t.definition(),
            Self::DesktopScroll(t) => t.definition(),
            Self::DesktopHover(t) => t.definition(),
            Self::DesktopWindowList(t) => t.definition(),
            Self::DesktopWindowAction(t) => t.definition(),
            Self::DesktopBrowser(t) => t.definition(),
            Self::DesktopAppLaunch(t) => t.definition(),
            Self::SaveSkill(t) => t.definition(),
            Self::McpRemote(t) => ToolDef {
                name: t.qualified_name.clone(),
                description: if t.description.is_empty() {
                    format!("MCP tool {}", t.runtime.remote_name)
                } else {
                    t.description.clone()
                },
                parameters: t.parameters.clone(),
            },
        }
    }

    /// Execute with an optional live-output channel (ADR-037): tools that
    /// produce long-running output (today: `run_command`) push each line as
    /// it arrives. Other tools ignore the channel and behave as `execute`.
    pub async fn execute_with_progress(
        &self,
        arguments: serde_json::Value,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
        cancel_flag: Option<&std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<ToolResult> {
        match self {
            Self::RunCommand(t) => {
                t.execute_with_progress(arguments, progress_tx, cancel_flag)
                    .await
            }
            _ => self.execute(arguments).await,
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        match self {
            Self::ListDirectory(t) => t.execute(arguments).await,
            Self::ReadFile(t) => t.execute(arguments).await,
            Self::WriteFile(t) => t.execute(arguments).await,
            Self::EditFile(t) => t.execute(arguments).await,
            Self::RunCommand(t) => t.execute(arguments).await,
            Self::GrepSearch(t) => t.execute(arguments).await,
            Self::GitStatus(t) => t.execute(arguments).await,
            Self::GitDiff(t) => t.execute(arguments).await,
            Self::WebFetch(t) => t.execute(arguments).await,
            Self::FindPath(t) => t.execute(arguments).await,
            Self::CreateDirectory(t) => t.execute(arguments).await,
            Self::DeletePath(t) => t.execute(arguments).await,
            Self::CopyPath(t) => t.execute(arguments).await,
            Self::UndoLastEdit(t) => t.execute(arguments).await,
            Self::RedoLastEdit(t) => t.execute(arguments).await,
            Self::DesktopScreenshot(t) => t.execute(arguments).await,
            Self::DesktopClick(t) => t.execute(arguments).await,
            Self::DesktopType(t) => t.execute(arguments).await,
            Self::DesktopKey(t) => t.execute(arguments).await,
            Self::DesktopScroll(t) => t.execute(arguments).await,
            Self::DesktopHover(t) => t.execute(arguments).await,
            Self::DesktopWindowList(t) => t.execute(arguments).await,
            Self::DesktopWindowAction(t) => t.execute(arguments).await,
            Self::DesktopBrowser(t) => t.execute(arguments).await,
            Self::DesktopAppLaunch(t) => t.execute(arguments).await,
            Self::SaveSkill(t) => t.execute(arguments).await,
            Self::McpRemote(t) => match t.runtime.execute(arguments).await {
                Ok(output) => Ok(ToolResult {
                    metrics: None,
                    success: true,
                    output,
                }),
                Err(e) => Ok(ToolResult {
                    metrics: None,
                    success: false,
                    output: e.to_string(),
                }),
            },
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::ListDirectory(_) => "list_directory",
            Self::ReadFile(_) => "read_file",
            Self::WriteFile(_) => "write_file",
            Self::EditFile(_) => "edit_file",
            Self::RunCommand(_) => "run_command",
            Self::GrepSearch(_) => "search_code",
            Self::GitStatus(_) => "git_status",
            Self::GitDiff(_) => "git_diff",
            Self::WebFetch(_) => "web_fetch",
            Self::FindPath(_) => "find_path",
            Self::CreateDirectory(_) => "create_directory",
            Self::DeletePath(_) => "delete_path",
            Self::CopyPath(_) => "copy_path",
            Self::UndoLastEdit(_) => "undo_last_edit",
            Self::RedoLastEdit(_) => "redo_last_edit",
            Self::DesktopScreenshot(_) => "desktop_screenshot",
            Self::DesktopClick(_) => "desktop_click",
            Self::DesktopType(_) => "desktop_type",
            Self::DesktopKey(_) => "desktop_key",
            Self::DesktopScroll(_) => "desktop_scroll",
            Self::DesktopHover(_) => "desktop_hover",
            Self::DesktopWindowList(_) => "desktop_window_list",
            Self::DesktopWindowAction(_) => "desktop_window_action",
            Self::DesktopBrowser(_) => "desktop_browser",
            Self::DesktopAppLaunch(_) => "desktop_app_launch",
            Self::SaveSkill(_) => "save_skill",
            Self::McpRemote(t) => t.qualified_name.as_str(),
        }
    }

    /// Whether this tool requires user confirmation before execution.
    pub fn requires_confirmation(&self) -> bool {
        matches!(
            self,
            Self::WriteFile(_)
                | Self::EditFile(_)
                | Self::RunCommand(_)
                | Self::DeletePath(_)
                | Self::DesktopClick(_)
                | Self::DesktopType(_)
                | Self::DesktopKey(_)
                | Self::DesktopScroll(_)
                | Self::DesktopHover(_)
                | Self::DesktopBrowser(_)
                | Self::DesktopAppLaunch(_)
                | Self::McpRemote(_)
        )
    }

    /// Human-readable confirmation message for this tool call.
    pub fn confirm_message(&self, args: &serde_json::Value) -> String {
        match self {
            Self::WriteFile(_) => {
                let path = args["path"].as_str().unwrap_or("?");
                format!("Write to file: {path}\nAllow?")
            }
            Self::EditFile(_) => {
                let path = args["path"].as_str().unwrap_or("?");
                let count = args["edits"].as_array().map(|a| a.len()).unwrap_or(0);
                format!("Edit file: {path} ({count} change(s))\nAllow?")
            }
            Self::RunCommand(_) => {
                let cmd = args["command"].as_str().unwrap_or("?");
                format!("Run command: {cmd}\nAllow?")
            }
            Self::DeletePath(_) => {
                let path = args["path"].as_str().unwrap_or("?");
                format!("Delete: {path}\nThis is irreversible. Allow?")
            }
            Self::DesktopClick(_) => {
                let x = args["x"].as_i64().unwrap_or(0);
                let y = args["y"].as_i64().unwrap_or(0);
                format!("Click desktop at ({x}, {y})\nAllow?")
            }
            Self::DesktopType(_) => {
                let text = args["text"].as_str().unwrap_or("?");
                let preview: String = text.chars().take(40).collect();
                let suffix = if text.len() > 40 { "..." } else { "" };
                format!("Type text: \"{preview}{suffix}\"\nAllow?")
            }
            Self::DesktopAppLaunch(_) => {
                let app = args["app"].as_str().unwrap_or("?");
                let args_list = args["args"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                if args_list.is_empty() {
                    format!("Launch application: {app}\nAllow?")
                } else {
                    format!("Launch application: {app} {args_list}\nAllow?")
                }
            }
            Self::McpRemote(t) => {
                format!(
                    "Run MCP tool «{}» on server «{}»?\nAllow?",
                    t.runtime.remote_name, t.runtime.profile.label
                )
            }
            _ => String::new(),
        }
    }

    /// Scope for persisted allow-rule matching, if this call is remember-able:
    /// `("path", value)` or `("command", value)`. Reads, writes and commands
    /// participate; deletes and platform/desktop tools never do.
    pub fn allow_scope(&self, args: &serde_json::Value) -> Option<(String, String)> {
        let path = |args: &serde_json::Value| {
            args["path"]
                .as_str()
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| (crate::allow::SCOPE_PATH.to_string(), p.to_string()))
        };
        match self {
            Self::ReadFile(_)
            | Self::ListDirectory(_)
            | Self::GrepSearch(_)
            | Self::WriteFile(_)
            | Self::EditFile(_) => path(args),
            Self::RunCommand(_) => args["command"]
                .as_str()
                .map(|c| c.trim())
                .filter(|c| !c.is_empty())
                .map(|c| (crate::allow::SCOPE_COMMAND.to_string(), c.to_string())),
            _ => None,
        }
    }

    /// Path argument of a workspace-scoped read tool call (read tools only).
    pub fn read_scope_path(&self, args: &serde_json::Value) -> Option<String> {
        match self {
            Self::ReadFile(_) | Self::ListDirectory(_) | Self::GrepSearch(_) => args["path"]
                .as_str()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty()),
            _ => None,
        }
    }

    /// If this call is a read that resolves outside `work_dir`, returns the
    /// resolved target — the exact scope the user would authorize.
    #[allow(clippy::match_like_matches_macro)]
    pub fn scoped_read_target(
        &self,
        args: &serde_json::Value,
        work_dir: Option<&str>,
    ) -> Option<std::path::PathBuf> {
        #[allow(clippy::question_mark)] // 类型结构不适用 ?
        let Some(wd) = work_dir.map(str::trim).filter(|s| !s.is_empty()) else {
            return None;
        };
        let p = self.read_scope_path(args)?;
        if paths::path_within(&p, Some(wd)) {
            return None;
        }
        paths::resolve_scoped(std::path::Path::new(wd), &p).ok()
    }

    /// Whether this call needs user confirmation:
    /// 1. a persisted allow rule matches → no;
    /// 2. inherently dangerous tools (writes / commands / platform) → yes;
    /// 3. reads resolving outside the workspace → yes (scope authorization).
    #[allow(clippy::match_like_matches_macro)]
    pub fn needs_confirmation(
        &self,
        args: &serde_json::Value,
        work_dir: Option<&str>,
        rules: Option<&crate::allow::AllowRules>,
    ) -> bool {
        if let Some((scope, value)) = self.allow_scope(args)
            && rules.is_some_and(|r| r.matches(self.name(), &scope, &value))
        {
            return false;
        }
        if self.requires_confirmation() {
            return true;
        }
        match (
            work_dir.map(str::trim).filter(|s| !s.is_empty()),
            self.read_scope_path(args),
        ) {
            (Some(wd), Some(p)) if !paths::path_within(&p, Some(wd)) => true,
            _ => false,
        }
    }
}

/// Strip the internal scoped-read marker from incoming args so a
/// model-invented key can never self-authorize; the gate re-injects it only
/// after user approval (or a matching allow rule).
pub fn scrub_scoped_marker(args: &serde_json::Value) -> serde_json::Value {
    let Some(obj) = args.as_object() else {
        return args.clone();
    };
    if !obj.contains_key(crate::allow::SCOPED_MARKER) {
        return args.clone();
    }
    let mut obj = obj.clone();
    obj.remove(crate::allow::SCOPED_MARKER);
    serde_json::Value::Object(obj)
}

#[cfg(test)]
mod confirm_gate_tests {
    use super::*;

    fn args(json: serde_json::Value) -> serde_json::Value {
        json
    }

    fn tool(name: &str, work_dir: &str) -> Tool {
        match name {
            "read_file" => Tool::ReadFile(file::ReadFile::new(work_dir)),
            "list_directory" => Tool::ListDirectory(list_dir::ListDirectory::new(work_dir)),
            "write_file" => Tool::WriteFile(file::WriteFile::new(work_dir)),
            "run_command" => Tool::RunCommand(cmd::RunCommand::new(work_dir)),
            _ => panic!("unknown tool {name}"),
        }
    }

    #[test]
    fn reads_inside_workspace_never_confirm() {
        let t = tool("read_file", "C:/work/project");
        assert!(!t.needs_confirmation(
            &args(serde_json::json!({ "path": "src/main.rs" })),
            Some("C:/work/project"),
            None
        ));
    }

    #[test]
    fn reads_outside_workspace_confirm() {
        let t = tool("read_file", "C:/work/project");
        assert!(t.needs_confirmation(
            &args(serde_json::json!({ "path": "C:/Windows/Temp/x.txt" })),
            Some("C:/work/project"),
            None
        ));
        assert!(t.needs_confirmation(
            &args(serde_json::json!({ "path": "../secret.txt" })),
            Some("C:/work/project"),
            None
        ));
    }

    #[test]
    fn writes_and_commands_always_confirm_without_rules() {
        let wd = "C:/work/project";
        let w = tool("write_file", wd);
        assert!(w.needs_confirmation(
            &args(serde_json::json!({ "path": "src/main.rs" })),
            Some(wd),
            None
        ));
        let c = tool("run_command", wd);
        assert!(c.needs_confirmation(
            &args(serde_json::json!({ "command": "cargo test" })),
            Some(wd),
            None
        ));
    }

    #[test]
    fn allow_rule_short_circuits_confirm() {
        let wd = "C:/work/project";
        let mut rules = crate::allow::AllowRules::default();
        rules.add(crate::allow::AllowRule {
            tool: "read_file".into(),
            scope: crate::allow::SCOPE_PATH.into(),
            value: "C:/work/reference".into(),
        });
        rules.add(crate::allow::AllowRule {
            tool: "run_command".into(),
            scope: crate::allow::SCOPE_COMMAND.into(),
            value: "cargo test".into(),
        });
        let r = tool("read_file", wd);
        assert!(!r.needs_confirmation(
            &args(serde_json::json!({ "path": "C:/work/reference/notes.md" })),
            Some(wd),
            Some(&rules)
        ));
        let c = tool("run_command", wd);
        assert!(!c.needs_confirmation(
            &args(serde_json::json!({ "command": "cargo test --lib" })),
            Some(wd),
            Some(&rules)
        ));
        // Boundary: rule for C:/work/reference must not cover reference2.
        assert!(r.needs_confirmation(
            &args(serde_json::json!({ "path": "C:/work/reference2/x.md" })),
            Some(wd),
            Some(&rules)
        ));
    }

    #[test]
    fn scoped_read_target_resolves_outside_path() {
        let t = tool("read_file", "C:/work/project");
        let target = t.scoped_read_target(
            &args(serde_json::json!({ "path": "C:/other/ref.md" })),
            Some("C:/work/project"),
        );
        let target = target.expect("outside read should yield a scope target");
        assert_eq!(target, std::path::Path::new("C:/other/ref.md"));
        let inside = t.scoped_read_target(
            &args(serde_json::json!({ "path": "src/main.rs" })),
            Some("C:/work/project"),
        );
        assert!(inside.is_none());
    }

    #[test]
    fn scrub_removes_only_the_marker() {
        let spoofed = serde_json::json!({
            "path": "C:/Windows/x.txt",
            "__stitch_scoped": true,
            "other": 1,
        });
        let scrubbed = scrub_scoped_marker(&spoofed);
        assert!(scrubbed.get("__stitch_scoped").is_none());
        assert_eq!(scrubbed["other"], 1);
        assert!(
            scrub_scoped_marker(&serde_json::json!({ "path": "a" }))
                .get("__stitch_scoped")
                .is_none()
        );
    }
}

/// Registry of all available tools.
#[derive(Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Tool>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Tool) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn definitions(&self) -> Vec<ToolDef> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.tools.get(name)
    }

    /// Attach discovered MCP protocol tools (qualified names).
    pub fn attach_mcp_tools(
        &mut self,
        discovered: &[crate::mcp_protocol::DiscoveredMcpTool],
        profiles: &[crate::config::McpServerProfile],
    ) {
        for d in discovered {
            let Some(profile) = profiles.iter().find(|p| p.id == d.server_id) else {
                continue;
            };
            let qualified = crate::mcp_protocol::qualify_tool_name(&d.server_id, &d.remote_name);
            if self.tools.contains_key(&qualified) {
                continue;
            }
            self.register(Tool::McpRemote(McpRemoteTool {
                qualified_name: qualified,
                description: d.description.clone(),
                parameters: d.input_schema.clone(),
                runtime: McpToolRuntime {
                    profile: Arc::new(profile.clone()),
                    remote_name: d.remote_name.clone(),
                },
            }));
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the default tool registry with all built-in tools.
pub fn build_registry(work_dir: &str) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Tool::ListDirectory(list_dir::ListDirectory::new(work_dir)));
    registry.register(Tool::ReadFile(file::ReadFile::new(work_dir)));
    registry.register(Tool::WriteFile(file::WriteFile::new(work_dir)));
    registry.register(Tool::EditFile(edit_file::EditFile::new(work_dir)));
    registry.register(Tool::RunCommand(cmd::RunCommand::new(work_dir)));
    registry.register(Tool::GrepSearch(search::GrepSearch::new(work_dir)));
    registry.register(Tool::GitStatus(git::GitStatus::new(work_dir)));
    registry.register(Tool::GitDiff(git::GitDiff::new(work_dir)));
    registry.register(Tool::WebFetch(web_fetch::WebFetch::new()));
    registry.register(Tool::FindPath(find_path::FindPath::new(work_dir)));
    registry.register(Tool::CreateDirectory(
        create_directory::CreateDirectory::new(work_dir),
    ));
    registry.register(Tool::DeletePath(delete_path::DeletePath::new(work_dir)));
    registry.register(Tool::CopyPath(copy_path::CopyPath::new(work_dir)));
    registry.register(Tool::UndoLastEdit(undo::UndoLastEdit::new()));
    registry.register(Tool::RedoLastEdit(undo::RedoLastEdit::new()));
    // Desktop automation — Windows-only; no-ops on other platforms
    registry.register(Tool::DesktopScreenshot(desktop::DesktopScreenshot::new(
        work_dir,
    )));
    registry.register(Tool::DesktopClick(desktop::DesktopClick));
    registry.register(Tool::DesktopType(desktop::DesktopType));
    registry.register(Tool::DesktopKey(desktop::DesktopKey));
    registry.register(Tool::DesktopScroll(desktop::DesktopScroll));
    registry.register(Tool::DesktopHover(desktop::DesktopHover));
    registry.register(Tool::DesktopWindowList(desktop::DesktopWindowList));
    registry.register(Tool::DesktopWindowAction(desktop::DesktopWindowAction));
    registry.register(Tool::DesktopBrowser(desktop::DesktopBrowser));
    registry.register(Tool::DesktopAppLaunch(desktop::DesktopAppLaunch));
    registry.register(Tool::SaveSkill(save_skill::SaveSkill::new(work_dir)));
    registry
}
