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
pub mod ignore;
pub mod list_dir;
pub mod memory;
pub mod paths;
pub mod process_win;
pub mod save_skill;
pub mod search;
pub mod task;
pub mod todo;
pub mod undo;
pub mod web_fetch;
pub mod web_search;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// `--add-dir` 附加根注入 trait：路径工具持有主工作目录 + 附加目录，
/// 路径解析（resolve_under_roots）与确认 gate（path_within_roots）把
/// 附加目录当作工作区路径处理。
pub trait ExtraRoots {
    fn set_extra_roots(&mut self, roots: Vec<PathBuf>);
    fn extra_roots(&self) -> &[PathBuf];
}

/// 给路径工具生成 `extra_roots` 字段与 trait impl。
macro_rules! extra_roots_impl {
    ($ty:ident) => {
        impl $ty {
            fn roots(&self) -> Vec<PathBuf> {
                let mut r = vec![self.work_dir.clone()];
                r.extend(self.extra_roots.iter().cloned());
                r
            }
        }
        impl $crate::tools::ExtraRoots for $ty {
            fn set_extra_roots(&mut self, roots: Vec<PathBuf>) {
                self.extra_roots = roots;
            }
            fn extra_roots(&self) -> &[PathBuf] {
                &self.extra_roots
            }
        }
    };
}
pub(crate) use extra_roots_impl;

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
    WebSearch(web_search::WebSearch),
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
    SaveMemory(memory::SaveMemory),
    TodoWrite(todo::TodoWrite),
    McpRemote(McpRemoteTool),
    Task(task::TaskSubagent),
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
            Self::WebSearch(t) => t.definition(),
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
            Self::SaveMemory(t) => t.definition(),
            Self::TodoWrite(t) => t.definition(),
            Self::Task(_) => task::TaskSubagent::definition(),
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
    /// Box::pin：Task 工具 → run_react_core → 本方法的递归 async 需要装箱。
    pub async fn execute_with_progress(
        &self,
        arguments: serde_json::Value,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
        cancel_flag: Option<&std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<ToolResult> {
        Box::pin(async move {
            match self {
                Self::RunCommand(t) => {
                    t.execute_with_progress(arguments, progress_tx, cancel_flag)
                        .await
                }
                _ => self.execute(arguments).await,
            }
        })
        .await
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
            Self::WebSearch(t) => t.execute(arguments).await,
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
            Self::SaveMemory(t) => t.execute(arguments).await,
            Self::TodoWrite(t) => t.execute(arguments).await,
            Self::Task(t) => t.execute(arguments).await,
            Self::McpRemote(t) => match t.runtime.execute(arguments).await {
                Ok(output) => Ok(ToolResult {
                    metrics: None,
                    success: true,
                    output,
                }),
                Err(e) => Ok(ToolResult::fail(e.to_string())),
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
            Self::WebSearch(_) => "web_search",
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
            Self::SaveMemory(_) => "save_memory",
            Self::TodoWrite(_) => "TodoWrite",
            Self::Task(_) => "Task",
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
        if paths::path_within(&p, Some(wd)) || self.in_extra_roots(&p) {
            return None;
        }
        paths::resolve_scoped(std::path::Path::new(wd), &p).ok()
    }

    /// Whether `user_path` lands inside an `--add-dir` additional root.
    /// Additional roots are treated like workspace paths: reads need no
    /// confirmation, writes follow the permission mode as usual.
    fn in_extra_roots(&self, user_path: &str) -> bool {
        let roots = match self {
            Self::ReadFile(t) => t.extra_roots(),
            Self::WriteFile(t) => t.extra_roots(),
            Self::EditFile(t) => t.extra_roots(),
            Self::ListDirectory(t) => t.extra_roots(),
            Self::GrepSearch(t) => t.extra_roots(),
            Self::CreateDirectory(t) => t.extra_roots(),
            Self::DeletePath(t) => t.extra_roots(),
            Self::CopyPath(t) => t.extra_roots(),
            _ => return false,
        };
        if roots.is_empty() {
            return false;
        }
        paths::path_within_roots(user_path, roots)
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
            (Some(wd), Some(p))
                if !paths::path_within(&p, Some(wd)) && !self.in_extra_roots(&p) =>
            {
                true
            }
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

    /// 工具定义列表（按名称排序——前缀缓存友好：HashMap 随机种子会导致
    /// 每次重建顺序漂移，使每轮首请求 tools 段必然 miss；排序后字节稳定）。
    pub fn definitions(&self) -> Vec<ToolDef> {
        let mut defs: Vec<ToolDef> = self.tools.values().map(|t| t.definition()).collect();
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
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
    build_registry_with_dirs(work_dir, &[])
}

/// 带 `--add-dir` 附加目录的注册表：附加根与主工作目录同等对待——
/// 路径解析允许落在任一根内，附加根内读取免确认（写入仍按权限模式）。
/// TodoWrite 用独立的会话内存储（/todo 命令不可见；桌面无专用 UI）。
pub fn build_registry_with_dirs(work_dir: &str, extra_roots: &[PathBuf]) -> ToolRegistry {
    build_registry_with_todo(
        work_dir,
        extra_roots,
        Arc::new(Mutex::new(todo::TodoStore::new())),
    )
}

/// CLI 专用：TodoWrite 挂共享的会话级任务清单（/todo 命令 + 回合进度行共用）。
pub fn build_registry_with_todo(
    work_dir: &str,
    extra_roots: &[PathBuf],
    todo_store: Arc<Mutex<todo::TodoStore>>,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    let extra = |t: &mut dyn ExtraRoots| {
        t.set_extra_roots(extra_roots.to_vec());
    };
    // 路径工具：注入附加根
    let mut list = list_dir::ListDirectory::new(work_dir);
    extra(&mut list);
    registry.register(Tool::ListDirectory(list));
    let mut read = file::ReadFile::new(work_dir);
    extra(&mut read);
    registry.register(Tool::ReadFile(read));
    let mut write = file::WriteFile::new(work_dir);
    extra(&mut write);
    registry.register(Tool::WriteFile(write));
    let mut edit = edit_file::EditFile::new(work_dir);
    extra(&mut edit);
    registry.register(Tool::EditFile(edit));
    let mut grep = search::GrepSearch::new(work_dir);
    extra(&mut grep);
    registry.register(Tool::GrepSearch(grep));
    let mut create = create_directory::CreateDirectory::new(work_dir);
    extra(&mut create);
    registry.register(Tool::CreateDirectory(create));
    let mut delete = delete_path::DeletePath::new(work_dir);
    extra(&mut delete);
    registry.register(Tool::DeletePath(delete));
    let mut copy = copy_path::CopyPath::new(work_dir);
    extra(&mut copy);
    registry.register(Tool::CopyPath(copy));
    // 其余工具：与 build_registry 一致
    registry.register(Tool::RunCommand(cmd::RunCommand::new(work_dir)));
    registry.register(Tool::GitStatus(git::GitStatus::new(work_dir)));
    registry.register(Tool::GitDiff(git::GitDiff::new(work_dir)));
    registry.register(Tool::WebFetch(web_fetch::WebFetch::new()));
    registry.register(Tool::WebSearch(web_search::WebSearch::new()));
    registry.register(Tool::FindPath(find_path::FindPath::new(work_dir)));
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
    registry.register(Tool::SaveMemory(memory::SaveMemory::new(work_dir)));
    registry.register(Tool::TodoWrite(todo::TodoWrite::with_store(todo_store)));
    registry
}

/// 构建子代理运行时上下文（会话级，调用方保存供事件注入）。
///
/// `base_registry` 须为**未注册 Task 前**的注册表（克隆给子代理做白名单
/// 基底——子代理不支持再委派，同时避免 Arc 循环引用）。
pub fn build_subagent_ctx(
    api_base: &str,
    model: &str,
    api_key: &str,
    max_iterations: usize,
    work_dir: Option<&str>,
    base_registry: &ToolRegistry,
    agents: Vec<crate::agents::SubAgentDef>,
) -> std::sync::Arc<task::SubagentCtx> {
    std::sync::Arc::new(task::SubagentCtx {
        api_base: api_base.to_string(),
        model: model.to_string(),
        api_key: std::sync::Mutex::new(api_key.to_string()),
        max_iterations,
        work_dir: work_dir.map(str::to_string),
        agents,
        depth: std::sync::atomic::AtomicUsize::new(0),
        max_depth: 2,
        event_tx: std::sync::Mutex::new(None),
        registry: base_registry.clone(),
    })
}

/// 把 Task 工具注册进注册表（调用前先 `build_subagent_ctx` 快照基底）。
pub fn attach_subagents(registry: &mut ToolRegistry, ctx: &std::sync::Arc<task::SubagentCtx>) {
    registry.register(Tool::Task(task::TaskSubagent { ctx: ctx.clone() }));
}
