//! System prompt construction.
//!
//! Builds a comprehensive system prompt dynamically from:
//! 1. Core agent persona and safety rules
//! 2. Available tool list (generated from ToolRegistry)
//! 3. Workspace context (directory, git status, OS info, project structure)
//! 4. Optional project rules (.stitchrules or similar)
//! 5. PromptStdio task suite / agent configuration (via MCP)

use crate::tools::ToolRegistry;

/// Context about the current workspace for the system prompt.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceContext {
    pub work_dir: String,
    pub git_branch: Option<String>,
    pub git_status: Option<String>,
    /// Auto-detected project info (framework, language, build system)
    pub project_type: Option<String>,
    pub project_files: Vec<String>,
}

/// Build the system prompt for the agent, with dynamic tool descriptions.
/// Automatically loads rules from `~/.stitchrules` and `<project>/.stitchrules`.
pub fn build_system_prompt(work_dir: &str, tools: &ToolRegistry) -> String {
    build_system_prompt_with_skill(work_dir, tools, None)
}

/// Build the system prompt and inject a Skill file's full content.
pub fn build_system_prompt_with_skill(
    work_dir: &str,
    tools: &ToolRegistry,
    skill_content: Option<&str>,
) -> String {
    let ctx = gather_workspace_context(work_dir);
    let project_rules = super::rules::load_rules(work_dir);
    let project_info = super::project::analyze(work_dir);
    build_system_prompt_with_context(
        work_dir,
        tools,
        &ctx,
        project_rules.as_deref(),
        &project_info,
        skill_content,
    )
}

/// Build a prompt with explicit workspace context and optional project rules.
pub fn build_system_prompt_with_context(
    _work_dir: &str,
    tools: &ToolRegistry,
    ctx: &WorkspaceContext,
    project_rules: Option<&str>,
    project_info: &super::project::ProjectInfo,
    skill_content: Option<&str>,
) -> String {
    let os = os_info();
    let now = chrono_now();

    let mut prompt = String::new();

    // ── Persona ──────────────────────────────────────────────────────
    prompt.push_str(
        "You are Stitch, an expert AI coding agent from PromptStdio. \
         You help users complete software engineering tasks by understanding \
         their codebase, making careful changes, and explaining your work clearly. \
         Use your broad knowledge of programming languages, frameworks, design \
         patterns, and engineering best practices to solve problems pragmatically.\n\n",
    );

    // ── Skill content (injected from .agents/skills/*/SKILL.md) ──────
    if let Some(skill) = skill_content {
        prompt.push_str("## Active Skill\n\n");
        prompt.push_str(
            "The user has loaded the following Skill. Follow its steps carefully and use ",
        );
        prompt.push_str("the referenced tools to complete the task.\n\n");
        prompt.push_str(skill);
        prompt.push_str("\n\n");
    }

    // ── Environment ──────────────────────────────────────────────────
    prompt.push_str("## Environment\n");
    prompt.push_str(&format!("- Working directory: {}\n", ctx.work_dir));
    prompt.push_str(
        "- All file creates/edits/deletes stay inside this working directory \
         (relative paths only). Put generated project files here by default.\n",
    );
    prompt.push_str(&format!("- OS: {os}\n"));
    prompt.push_str(&format!("- Current time: {now}\n"));

    if let Some(ref branch) = ctx.git_branch {
        prompt.push_str(&format!("- Git branch: {branch}\n"));
    }
    if let Some(ref status) = ctx.git_status
        && !status.is_empty()
    {
        prompt.push_str(&format!("- Git status: {status}\n"));
    }
    if let Some(ref proj) = ctx.project_type {
        prompt.push_str(&format!("- Project type: {proj}\n"));
    }

    // Project analysis — build/test/lint commands, versions
    let pa = super::project::format_for_prompt(project_info);
    if !pa.is_empty() {
        prompt.push_str(&pa);
        prompt.push('\n');
    }

    prompt.push('\n');

    // ── Available Tools ──────────────────────────────────────────────
    prompt.push_str("## Available Tools\n\n");
    prompt.push_str("You have access to these tools. Use them wisely:\n\n");

    for def in tools.definitions() {
        let params_desc = summarize_params(&def.parameters);
        prompt.push_str(&format!(
            "### {}\n{}\nParameters: {}\n\n",
            def.name, def.description, params_desc
        ));
        // ── Tool Calling Format ──────────────────────────────────────────────────────
        prompt.push_str("## How to Call Tools\n\n");
        prompt.push_str("CRITICAL: After </think>, ONLY output tool calls, never descriptions.\n");
        prompt.push_str("Do NOT write plans or explanations. Just raw calls. One per line.\n\n");
        prompt.push_str("Format: tool_name(\"arg1\", \"arg2\")\n");
        prompt.push_str("- Arguments are positional (same order as Parameters above).\n");
        prompt.push_str("- No markdown, no extra parentheses, no commentary.\n");
        prompt.push_str("- Example: write_file(\"hello.py\", \"print('hi')\")\n");
        prompt.push_str("- Example: run_command(\"python hello.py\")\n\n");
    }

    // ── Rules ────────────────────────────────────────────────────────
    prompt.push_str("## Rules\n\n");
    prompt.push_str(include_str!("prompt_rules.txt"));
    prompt.push('\n');

    // ── Project Rules ────────────────────────────────────────────────
    if let Some(rules) = project_rules
        && !rules.trim().is_empty()
    {
        prompt.push_str("## Project-Specific Rules\n\n");
        prompt.push_str(rules);
        prompt.push_str("\n\n");
    }

    // ── Output Style ─────────────────────────────────────────────────
    prompt.push_str("## Communication & Output\n\n");
    prompt.push_str(
        "- Be concise and direct. Show your reasoning briefly, then act.\n\
         - When writing code, include complete content, not fragments.\n\
         - After making changes, summarize what you did.\n\
         - Use the project's existing conventions and style — don't impose your own.\n\
         - If you're unsure about something, ask for clarification.\n\
         - Never use emoji or emoticons in replies (product tone).\n\
         - Prefer plain, neutral wording; avoid hype or marketing phrases.\n",
    );

    // ── Desktop automation guidance (when desktop tools are available) ────
    let has_desktop = tools
        .definitions()
        .iter()
        .any(|d| d.name.starts_with("desktop_"));
    if has_desktop {
        prompt.push_str("\n## Desktop Automation Guidelines\n\n");
        prompt.push_str(
            "You are operating on a real Windows desktop. Follow these rules:\n\n\
             - **Before any task**: use desktop_window_list to survey the screen. \
             If any overlapping/floating windows (Settings, notifications, dialogs) \
             block your target, use desktop_window_action to minimize or close them FIRST.\n\
             - **Seeing the screen**: use desktop_screenshot with ocr=true to read \
             screen content. OCR extracts visible text — it won't see images, \
             but it reads window titles, menu items, buttons, and body text.\n\
             - **Navigating**: prefer desktop_key shortcuts (ctrl+l for address bar, \
             ctrl+t for new tab, tab to move focus, enter to confirm) over blind clicking.\n\
             - **Scrolling**: use desktop_scroll to browse long pages.\n\
             - **Keyboard input goes to the foreground window only**: desktop_type and \
             desktop_key affect whatever window currently has focus. Before typing, confirm \
             the target window is on top — use desktop_window_action focus on it (or minimize \
             covering windows), then verify with a screenshot. After launching an app, check \
             it actually came to the foreground; the tool output reports the foreground window \
             title after each type/key, so watch it.\n\
             - **Window titles depend on the OS locale**: a notepad window may be titled \
             \"记事本\" (Chinese) or \"Notepad\"/\"Untitled - Notepad\" (English). Always read \
             the ACTUAL title from desktop_window_list and match on the real text — never \
             assume a fixed name.\n\
             - **Window management**: desktop_window_action supports minimize, close, \
             restore, maximize, and focus. Minimize, don't close, windows you may need later.\n\
             - **Closing windows**: after a close, ALWAYS call desktop_window_list to verify \
             the window is really gone. If it remains, the close was blocked by a modal dialog \
             (e.g. an unsaved-changes prompt) — do not give up: screenshot it (ocr=true), \
             dismiss the dialog (press the 'Don't Save' / 不保存 button via keyboard or click, \
             or Esc), then retry the close and verify again. The user asked to close the \
             window, so discarding unsaved changes is intended.\n\
             - **Be autonomous**: don't ask permission to minimize overlapping windows. \
             If a window blocks your view of the target, minimize it immediately.\n\
             - **Verify after each action**: take a screenshot or window list to confirm the result.\n",
        );
    }

    prompt
}

/// Gather workspace context quickly without blocking.
fn gather_workspace_context(work_dir: &str) -> WorkspaceContext {
    let mut ctx = WorkspaceContext {
        work_dir: work_dir.to_string(),
        ..Default::default()
    };

    // Git branch (fast)
    {
        let mut cmd = std::process::Command::new("git");
        cmd.args(["branch", "--show-current"]).current_dir(work_dir);
        crate::tools::process_win::hide_console_std(&mut cmd);
        if let Ok(output) = cmd.output() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() {
                ctx.git_branch = Some(branch);
            }
        }
    }

    // Git status (short)
    {
        let mut cmd = std::process::Command::new("git");
        cmd.args(["status", "--short"]).current_dir(work_dir);
        crate::tools::process_win::hide_console_std(&mut cmd);
        if let Ok(output) = cmd.output() {
            let status = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = status.lines().take(20).collect();
            let summary = lines.join("\n");
            if !summary.is_empty() {
                ctx.git_status = Some(summary);
            }
        }
    }

    // Detect project type
    ctx.project_type = detect_project_type(work_dir);

    // Quick project file scan (top-level only)
    ctx.project_files = quick_file_scan(work_dir);

    ctx
}

/// Detect the project type from known config files.
fn detect_project_type(work_dir: &str) -> Option<String> {
    let indicators: &[(&str, &str)] = &[
        ("Cargo.toml", "Rust (Cargo workspace)"),
        ("package.json", "Node.js / JavaScript"),
        ("tsconfig.json", "TypeScript"),
        ("go.mod", "Go"),
        ("requirements.txt", "Python"),
        ("pyproject.toml", "Python"),
        ("Makefile", "C/C++ (Make)"),
        ("CMakeLists.txt", "C/C++ (CMake)"),
        ("build.gradle", "Java/Kotlin (Gradle)"),
        ("pom.xml", "Java (Maven)"),
        ("composer.json", "PHP"),
        ("Gemfile", "Ruby"),
        ("mix.exs", "Elixir"),
    ];

    let mut found: Vec<&str> = Vec::new();
    let wp = std::path::Path::new(work_dir);

    for (file, label) in indicators {
        if wp.join(file).exists() {
            found.push(*label);
        }
    }

    if found.is_empty() {
        None
    } else {
        Some(found.join(", "))
    }
}

/// Quick scan of top-level files (not recursive — just to give context).
fn quick_file_scan(work_dir: &str) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    let wp = std::path::Path::new(work_dir);

    if let Ok(entries) = std::fs::read_dir(wp) {
        for entry in entries.filter_map(|e| e.ok()).take(100) {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') {
                continue;
            }
            let rel = path.strip_prefix(wp).unwrap_or(&path);
            if path.is_dir() {
                files.push(format!("{}/", rel.display()));
            } else {
                files.push(rel.display().to_string());
            }
        }
    }

    files.sort();
    files.truncate(80);
    files
}

/// Generate a human-readable parameter summary from a JSON Schema.
fn summarize_params(schema: &serde_json::Value) -> String {
    let props = match schema.get("properties") {
        Some(p) => p,
        None => return "none".to_string(),
    };

    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut parts: Vec<String> = Vec::new();
    if let Some(obj) = props.as_object() {
        for (name, prop) in obj {
            let ptype = prop.get("type").and_then(|t| t.as_str()).unwrap_or("any");
            let marker = if required.contains(&name.as_str()) {
                ""
            } else {
                "?"
            };
            parts.push(format!("{name}{marker}: {ptype}"));
        }
    }

    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(", ")
    }
}

fn os_info() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    secs_to_rough_local(secs)
}

fn secs_to_rough_local(secs: u64) -> String {
    let total_days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;

    let mut y = 1970i64;
    let mut d = total_days as i64;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }
    let month_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0;
    while m < 12 && d >= month_days[m] {
        d -= month_days[m];
        m += 1;
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        y,
        m + 1,
        d + 1,
        hours,
        minutes
    )
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}
