//! 交互式 REPL（工业级终端体验 · 0.5.0 重塑）。
//!
//! - 欢迎 banner + 快捷命令提示（/help）
//! - rustyline 输入历史（~/.stitch/history.txt）
//! - slash 命令：/help /exit /quit /clear /model /cost /sessions
//! - Ctrl+C 中断当前回合（ctrlc → cancel_flag）；空闲时退出
//! - 会话持久化：`{work_dir}/.stitch/sessions/{id}/`（复用 persist 机制）
//! - 流式 Markdown 渲染（复用 render 层）
//! - 输入框（Claude Code 式）：顶边信息行（✳ 模型 · 权限 · 上下文% · 分支右对齐）、
//!   左框线提示符 `│ ❯ 目录`、回合收尾底边右对齐 statusline
//! - 输入高亮细分（/ 命令名青参数灰 · ! 黄 · @ 青）+ fish 式灰提示（slash 续尾 / 历史）

use crate::agent::{self, AgentEvent};
use crate::config::StitchConfig;
use crate::session::{self, Session};
use crate::tools;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// ASCII 欢迎 banner（版本与 CLI 同源）。
fn banner(version: &str) -> String {
    format!(
        r#"
  ███████╗████████╗██╗████████╗ ██████╗██╗  ██╗
  ██╔════╝╚══██╔══╝██║╚══██╔══╝██╔════╝██║  ██║
  ███████╗   ██║   ██║   ██║   ██║     ███████║
  ╚════██║   ██║   ██║   ██║   ██║     ██╔══██║
  ███████║   ██║   ██║   ██║   ╚██████╗██║  ██║
  ╚══════╝   ╚═╝   ╚═╝   ╚═╝   ╚═════╝╚═╝  ╚═╝
  PromptStdio Agent CLI v{version} — type /help for commands, Ctrl+C to interrupt
"#
    )
}

/// slash 命令解析结果。
#[derive(Debug)]
enum SlashAction {
    Exit,
    Clear,
    Model(String),
    Cost,
    Context,
    Compact,
    Sessions,
    Export(Option<String>),
    Permissions(Vec<String>),
    Rewind,
    Agents,
    Config(String),
    Mcp(String),
    Hooks,
    Upgrade,
    Profile(Vec<String>),
    Review,
    Fix,
    Memory(Vec<String>),
    Inspect,
    Retry,
    Draft(Vec<String>),
    Todo(Vec<String>),
    OutputStyle(Vec<String>),
    Statusline(Vec<String>),
    Search(String),
    Think(Vec<String>),
    Init,
    Skill(Vec<String>),
    Plan(Vec<String>),
    Help,
    /// 自定义 slash 命令（.claude/commands/*.md）：(命令名, 参数)
    Custom(String, String),
    Unknown(String),
}

const SLASH_HELP: &str = r#"
  /help                    Show this help
  /exit  (或 /quit)         Quit the session
  /clear                   Start a new session (current one is saved)
  /model <name>            Switch model (e.g. /model deepseek-v4-flash)
  /cost                    Show cost & cache stats for this session
  /context                 Show context usage (tokens / limit, layer stats)
  /compact                 Manually condense this session's history
  /export [path]           Export the transcript to a Markdown file
  /permissions             List allow rules
  /permissions add <tool> <scope> <value>      Add an allow rule
  /permissions remove <tool> <scope> <value>   Remove an allow rule
  /permissions clear       Clear all allow rules
  /rewind  (或 /undo)       Roll back to the previous turn (messages + file changes)
  /sessions                List saved sessions in this workspace
  /agents                  List available subagents (.claude/agents/*.md)
  /mcp                     Check MCP server connections (status + tool count)
  /mcp add <名称> <命令>     Add an MCP server (stdio; or --url <端点>)
  /mcp remove <名称>         Remove an MCP server
  /mcp on|off <名称>         Enable / disable an MCP server
  /mcp list                List all configured MCP servers
  /hooks                   Show active hooks (global + workspace)
  /config                  Show current configuration summary
  /config <键>=<值>         Set a config value in-session (e.g. /config max_iterations=20)
  /profile [id]            Switch saved LLM configs (interactive picker)
  /upgrade                 Check for updates and upgrade in place
  /review                  Review uncommitted git changes (bug / security / omissions)
  /fix                     Diagnose & fix the last failed ! command
  /memory                  List the 3 memory layers (global / project / local)
  /memory open <层>         Edit a memory file (local | project | global)
  /memory create <层>       Create a new project or local memory file
  /memory delete <层>       Delete a memory file
  /inspect                 Show the full system prompt (memory + tools + rules)
  /todo                    Show the task list (maintained by TodoWrite tool)
  /todo clear              Clear the task list
  /retry                   Regenerate the last answer (same user message)
  /draft [on|off]          Toggle draft mode (file changes preview only)
  /output-style <风格>      Set reply verbosity: compact | concise | verbose (default)
  /statusline [clear|set]  Show / configure the status line (interactive picker)
  /search <关键词>          Search all sessions' history for a keyword
  /think [on|off]          Show the model's thinking process (default off)
  /plan [on|off|描述]       Plan mode: draft a plan, review & approve, then execute step by step
  /init                    Create a CLAUDE.md project memory file (does not overwrite)
  Tab                      Complete slash commands / file names
  单独一行 {                Multi-line input (end with a line of just })
  Ctrl+C                   Interrupt the current turn (press again when idle to quit)
"#;

fn parse_slash(line: &str) -> SlashAction {
    let mut parts = line.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest = parts.next().unwrap_or("").trim().to_string();
    match cmd.as_str() {
        "/help" | "/?" => SlashAction::Help,
        "/exit" | "/quit" => SlashAction::Exit,
        "/clear" | "/new" => SlashAction::Clear,
        "/model" => SlashAction::Model(rest),
        "/cost" => SlashAction::Cost,
        "/context" | "/usage" => SlashAction::Context,
        "/compact" => SlashAction::Compact,
        "/export" => {
            let path = rest.split_whitespace().next().map(str::to_string);
            SlashAction::Export(path)
        }
        "/permissions" | "/allowed-tools" => {
            SlashAction::Permissions(rest.split_whitespace().map(str::to_string).collect())
        }
        "/rewind" | "/undo" => SlashAction::Rewind,
        "/sessions" => SlashAction::Sessions,
        "/agents" => SlashAction::Agents,
        "/mcp" => SlashAction::Mcp(rest),
        "/hooks" => SlashAction::Hooks,
        "/config" => SlashAction::Config(rest),
        "/profile" => SlashAction::Profile(rest.split_whitespace().map(str::to_string).collect()),
        "/upgrade" => SlashAction::Upgrade,
        "/review" => SlashAction::Review,
        "/fix" => SlashAction::Fix,
        "/memory" => SlashAction::Memory(rest.split_whitespace().map(str::to_string).collect()),
        "/inspect" => SlashAction::Inspect,
        "/retry" => SlashAction::Retry,
        "/draft" => SlashAction::Draft(rest.split_whitespace().map(str::to_string).collect()),
        "/todo" => SlashAction::Todo(rest.split_whitespace().map(str::to_string).collect()),
        "/output-style" => {
            SlashAction::OutputStyle(rest.split_whitespace().map(str::to_string).collect())
        }
        "/statusline" => {
            SlashAction::Statusline(rest.split_whitespace().map(str::to_string).collect())
        }
        "/search" => SlashAction::Search(rest),
        "/think" => SlashAction::Think(rest.split_whitespace().map(str::to_string).collect()),
        "/init" => SlashAction::Init,
        "/skill" => SlashAction::Skill(rest.split_whitespace().map(str::to_string).collect()),
        "/plan" => SlashAction::Plan(rest.split_whitespace().map(str::to_string).collect()),
        _ => {
            let name = cmd.trim_start_matches('/');
            if name.is_empty() {
                SlashAction::Unknown("空命令".into())
            } else {
                SlashAction::Custom(name.to_string(), rest)
            }
        }
    }
}

/// 内置 slash 命令（Tab 补全候选；自定义命令动态加载）。
const BUILTIN_SLASHES: &[&str] = &[
    "/help",
    "/exit",
    "/quit",
    "/clear",
    "/new",
    "/model",
    "/cost",
    "/context",
    "/usage",
    "/compact",
    "/export",
    "/permissions",
    "/allowed-tools",
    "/rewind",
    "/undo",
    "/sessions",
    "/agents",
    "/mcp",
    "/hooks",
    "/config",
    "/profile",
    "/upgrade",
    "/review",
    "/fix",
    "/memory",
    "/inspect",
    "/retry",
    "/draft",
    "/think",
    "/todo",
    "/output-style",
    "/statusline",
    "/search",
    "/init",
    "/skill",
    "/plan",
];

/// rustyline 补全器：`/` 前缀补全 slash 命令（内置 + 自定义），
/// 否则补全工作目录下的文件名（目录候选带尾分隔符）。
#[derive(Clone)]
pub struct StitchCompleter {
    work_dir: String,
}

impl StitchCompleter {
    pub fn new(work_dir: &str) -> Self {
        Self {
            work_dir: work_dir.to_string(),
        }
    }
}

impl rustyline::completion::Completer for StitchCompleter {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        if line.starts_with('/') {
            let prefix = &line[..pos];
            let mut cands: Vec<String> = BUILTIN_SLASHES
                .iter()
                .map(|s| s.to_string())
                .filter(|s| s.starts_with(prefix))
                .collect();
            for c in crate::commands::load_commands(Some(&self.work_dir)) {
                let full = format!("/{}", c.name);
                if full.starts_with(prefix) {
                    cands.push(full);
                }
            }
            cands.sort();
            cands.dedup();
            return Ok((0, cands));
        }
        // 文件名补全：光标前最后一个词，不含路径分隔符才补（工作目录内）
        let up_to_cursor = &line[..pos];
        let token = up_to_cursor.split_whitespace().next_back().unwrap_or("");
        if token.contains('/') || token.contains('\\') {
            return Ok((0, Vec::new()));
        }
        let start = pos - token.len();
        let mut cands = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.work_dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(token) && !name.starts_with('.') {
                    let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    let suffix = if is_dir {
                        std::path::MAIN_SEPARATOR.to_string()
                    } else {
                        String::new()
                    };
                    cands.push(format!("{name}{suffix}"));
                }
            }
        }
        cands.sort();
        Ok((start, cands))
    }
}

impl rustyline::Helper for StitchCompleter {}
impl rustyline::hint::Hinter for StitchCompleter {
    type Hint = String;
    /// fish 式打字提示（灰色显示，见 highlight_hint）：
    /// - `/` 开头：首个匹配命令的续尾（内置 + 自定义，BUILTIN 顺序）
    /// - 其他：委托 rustyline HistoryHinter（历史相似条目续尾，右箭头可接受）
    fn hint(&self, line: &str, pos: usize, ctx: &rustyline::Context<'_>) -> Option<String> {
        if line.starts_with('/') && pos >= line.len() {
            let mut first: Option<String> = None;
            for cand in BUILTIN_SLASHES.iter().map(|s| s.to_string()).chain(
                crate::commands::load_commands(Some(&self.work_dir))
                    .into_iter()
                    .map(|c| format!("/{}", c.name)),
            ) {
                if cand.len() > line.len() && cand.starts_with(line) && first.is_none() {
                    first = Some(cand);
                }
            }
            return first.map(|full| full[line.len()..].to_string());
        }
        if pos >= line.len() {
            return rustyline::hint::HistoryHinter::new().hint(line, pos, ctx);
        }
        None
    }
}
impl rustyline::highlight::Highlighter for StitchCompleter {
    /// 输入高亮（rustyline colored 示例模式，ANSI 分段内联；C0 控制字符
    /// 先剥离——0.5.4 渲染层注入防护同款）：
    /// - `/cmd 参数`：命令名青色加粗、参数灰（一眼区分命令与参数）
    /// - `!命令`：黄色加粗（shell 快捷执行）
    /// - `@文件`：青色加粗（文件引用嵌入）
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> std::borrow::Cow<'l, str> {
        let clean = crate::render::markdown::strip_control(line);
        let clean = clean.as_ref();
        if clean.starts_with('/') {
            // split_once 会吞掉分隔符本身，灰段前补一个空格还原视觉间距
            let (cmd, rest) = clean.split_once(char::is_whitespace).unwrap_or((clean, ""));
            let out = if rest.is_empty() {
                format!("\x1b[1;36m{cmd}\x1b[0m")
            } else {
                format!("\x1b[1;36m{cmd}\x1b[0m\x1b[90m {rest}\x1b[0m")
            };
            std::borrow::Cow::Owned(out)
        } else if clean.starts_with('!') {
            std::borrow::Cow::Owned(format!("\x1b[1;33m{clean}\x1b[0m"))
        } else if clean.starts_with('@') && !clean.starts_with("@@") {
            std::borrow::Cow::Owned(format!("\x1b[1;36m{clean}\x1b[0m"))
        } else if std::ptr::eq(clean, line) {
            std::borrow::Cow::Borrowed(line)
        } else {
            std::borrow::Cow::Owned(clean.to_string())
        }
    }

    fn highlight_char(
        &self,
        _line: &str,
        _pos: usize,
        _kind: rustyline::highlight::CmdKind,
    ) -> bool {
        true // 全行样式由 highlight() 分段控制
    }

    /// 提示（Hinter）灰显：fish 式观感，与全局灰调一致。
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        std::borrow::Cow::Owned(format!("\x1b[90m{hint}\x1b[0m"))
    }
}
impl rustyline::validate::Validator for StitchCompleter {}

/// 会话摘要（sessions 列表用）。
pub struct SessionSummary {
    pub id: String,
    pub updated_at: String,
    pub msg_count: usize,
    pub title: String,
}

/// 会话标题智能提取：从第一条用户消息提炼可读标题。
///
/// - 工具/命令回放消息（「用户执行了命令 `X`…」「[文件引用 X]…」）还原为
///   `!X` / `@X`，不占标题位
/// - 只取首行，60 字符截断（防长命令撑爆列表）
fn session_title(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("").trim();
    let reduced = first_line
        .strip_prefix("用户执行了命令")
        .and_then(|rest| rest.trim().strip_prefix('`'))
        .and_then(|rest| rest.split('`').next())
        .map(|cmd| format!("!{cmd}"))
        .or_else(|| {
            first_line.strip_prefix("[文件引用").and_then(|rest| {
                let path = rest.split_whitespace().next()?;
                let path = path.strip_suffix(']').unwrap_or(path);
                Some(format!("@{path}"))
            })
        })
        .unwrap_or_else(|| first_line.to_string());
    let t = reduced.trim();
    if t.is_empty() {
        "<空消息>".into()
    } else {
        t.chars().take(60).collect()
    }
}

/// 列出工作区保存的会话（按更新时间倒序）。
pub fn list_sessions(work_dir: &str) -> Vec<SessionSummary> {
    let root = PathBuf::from(work_dir).join(".stitch").join("sessions");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let manifest_path = dir.join("manifest.json");
        let Ok(text) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<agent::persist::Manifest>(&text) else {
            continue;
        };
        let id = manifest.session_id.clone();
        // 标题：自定义（sessions rename）优先，否则第一条 user 消息智能提取
        let title = manifest
            .title
            .clone()
            .or_else(|| {
                std::fs::read_to_string(dir.join("messages.jsonl"))
                    .ok()
                    .and_then(|t| {
                        t.lines().skip(1).find_map(|l| {
                            serde_json::from_str::<session::Message>(l)
                                .ok()
                                .and_then(|m| {
                                    (m.role == session::Role::User)
                                        .then(|| session_title(m.content.text()))
                                })
                        })
                    })
            })
            .unwrap_or_default();
        out.push(SessionSummary {
            id,
            updated_at: manifest.updated_at,
            msg_count: manifest.msg_count,
            title,
        });
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

/// 导出会话转录为 Markdown（/export）。
pub fn export_session(session: &Session, path: &std::path::Path) -> anyhow::Result<()> {
    use std::fmt::Write as _;
    let mut md = String::new();
    writeln!(&mut md, "# Stitch 会话导出")?;
    writeln!(&mut md)?;
    for (i, m) in session.messages.iter().enumerate() {
        writeln!(&mut md, "## {i} · {}", m.role.as_str())?;
        if let Some(tcs) = &m.tool_calls {
            for tc in tcs {
                writeln!(
                    &mut md,
                    "\n**工具调用**：`{}({})`",
                    tc.function.name, tc.function.arguments
                )?;
            }
        }
        writeln!(&mut md)?;
        writeln!(&mut md, "{}", m.content.text())?;
        writeln!(&mut md)?;
    }
    std::fs::write(path, md)?;
    Ok(())
}

/// 上下文占用统计（/context）。
fn print_context_usage(session: &Session, model: &str) {
    let limit = agent::tokens::context_limit_for_model(model);
    let est = agent::tokens::estimate_messages(&session.messages);
    let pct = est.saturating_mul(100).checked_div(limit).unwrap_or(0);
    println!(
        "  上下文 {est} / {limit} tokens（{pct}%），消息 {} 条",
        session.messages.len()
    );
    if let Some(lm) = session.layers.as_ref() {
        let stats = lm.estimate_stats(&session.messages, limit);
        println!(
            "  分层：热 {} 条 · 温 {} 条目 · 冷 {} 条目",
            stats.hot_msgs, stats.warm_entries, stats.cold_entries
        );
    }
}

/// 允许规则管理（/permissions）。
fn handle_permissions(
    allow_rules: &Arc<Mutex<crate::allow::AllowRules>>,
    cfg: &mut StitchConfig,
    args: &[String],
) -> anyhow::Result<()> {
    use crate::allow::AllowRule;
    let mut rules = match allow_rules.lock() {
        Ok(g) => g,
        Err(_) => {
            println!("[错误] 规则锁损坏");
            return Ok(());
        }
    };
    match args.first().map(|s| s.as_str()) {
        None => {
            let pc = crate::permission::current();
            println!("  权限模式：{}", pc.mode.as_str());
            if pc.deny_tools.is_empty() {
                println!("  禁用工具：无");
            } else {
                println!("  禁用工具：{}", pc.deny_tools.join(", "));
            }
            if rules.is_empty() {
                println!("  暂无允许规则（对话确认卡勾选「记住此规则」后出现）");
            } else {
                for r in &rules.rules {
                    println!("  {} {} {}", r.tool, r.scope, r.value);
                }
            }
        }
        Some("mode") if args.len() == 2 => {
            match crate::permission::PermissionMode::parse(&args[1]) {
                Some(mode) => {
                    cfg.permission_mode = Some(mode.as_str().to_string());
                    let _ = cfg.save();
                    let mut pc = crate::permission::current();
                    pc.mode = mode;
                    crate::permission::set_config(pc);
                    println!("[权限模式] → {}", mode.as_str());
                }
                None => println!("[无效模式] 可选：default / accept_edits / plan / bypass"),
            }
        }
        Some("deny") if args.len() == 2 => {
            let tool = args[1].clone();
            let mut pc = crate::permission::current();
            if pc.deny_tools.contains(&tool) {
                println!("[已存在] {tool} 已在禁用列表");
            } else {
                pc.deny_tools.push(tool.clone());
                crate::permission::set_config(pc);
                if !cfg.disallowed_tools.contains(&tool) {
                    cfg.disallowed_tools.push(tool.clone());
                    let _ = cfg.save();
                }
                println!("[已禁用] {tool}（该工具将始终被拒绝，含 bypass 模式）");
            }
        }
        Some("undeny") | Some("allow") if args.len() == 2 => {
            let tool = args[1].clone();
            let mut pc = crate::permission::current();
            pc.deny_tools.retain(|d| *d != tool);
            crate::permission::set_config(pc);
            cfg.disallowed_tools.retain(|d| *d != tool);
            let _ = cfg.save();
            println!("[已解禁] {tool}");
        }
        Some("mode") => println!("[用法] /permissions mode <default|accept_edits|plan|bypass>"),
        Some("deny") | Some("undeny") | Some("allow") => {
            println!("[用法] /permissions deny <tool>   /   /permissions undeny <tool>");
        }
        Some("add") if args.len() >= 4 => {
            let rule = AllowRule {
                tool: args[1].clone(),
                scope: args[2].clone(),
                value: args[3].clone(),
            };
            if rules.add(rule) {
                let _ = rules.save();
                println!("[已添加] {} {} {}", args[1], args[2], args[3]);
            } else {
                println!("[已存在] 同规则已记住");
            }
        }
        Some("remove") if args.len() >= 4 => {
            if rules.remove(&args[1], &args[2], &args[3]) {
                let _ = rules.save();
                println!("[已移除] {} {} {}", args[1], args[2], args[3]);
            } else {
                println!("[未找到] 该规则不存在");
            }
        }
        Some("clear") => {
            rules.clear();
            let _ = rules.save();
            println!("[已清空] 所有允许规则");
        }
        _ => {
            println!("  用法：/permissions（列出）");
            println!("        /permissions mode <default|accept_edits|plan|bypass>");
            println!("        /permissions deny <tool>   /   undeny <tool>");
            println!("        /permissions add <tool> <scope> <value>");
            println!("        /permissions remove <tool> <scope> <value>");
            println!("        /permissions clear");
        }
    }
    Ok(())
}

/// 回合确认等待表（ConfirmRequest 事件 → oneshot 放行）。
type ConfirmTable =
    Arc<Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>>;

/// REPL 主循环。
/// `!` 快捷命令：执行 shell 命令并返回 (stdout+stderr 上限 32KB, 是否成功)。
/// 在 `work_dir` 下执行；超时 30s 返回错误文本而非 panic。
async fn run_shell_output(cmd: &str, work_dir: &str) -> anyhow::Result<(String, bool)> {
    let cmd = cmd.to_string();
    let work_dir = work_dir.to_string();
    let out = tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        let shell = ["cmd", "/C"];
        #[cfg(not(windows))]
        let shell = ["sh", "-c"];
        std::process::Command::new(shell[0])
            .args(&shell[1..])
            .arg(&cmd)
            .current_dir(&work_dir)
            .output()
    })
    .await
    .map_err(|e| anyhow::anyhow!("命令执行失败：{e}"))?;
    let out = out.map_err(|e| anyhow::anyhow!("命令执行失败：{e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    let success = out.status.success();
    if !success {
        text = format!("(退出码 {}) {text}", out.status.code().unwrap_or(-1));
    }
    if text.len() > 32 * 1024 {
        text.truncate(32 * 1024);
        text.push_str("\n…(输出过长已截断)");
    }
    if text.trim().is_empty() {
        text = "(无输出)".into();
    }
    Ok((text, success))
}

/// `@文件引用`：把 `@路径` 展开为文件内容嵌入消息。
/// 路径相对主工作目录解析（也允许附加根内绝对路径）；文件 ≤ 64KB 才
/// 嵌入（与规则文件上限一致）；解析失败保留原样，不打断输入。
fn expand_at_references(line: &str, work_dir: &str, extra_roots: &[PathBuf]) -> String {
    const MAX_REF_SIZE: u64 = 64 * 1024;
    let roots = std::iter::once(PathBuf::from(work_dir))
        .chain(extra_roots.iter().cloned())
        .collect::<Vec<_>>();
    let mut out = String::with_capacity(line.len() + 256);
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '@' && i + 1 < chars.len() && chars[i + 1] != '@' && chars[i + 1] != ' ' {
            // 收集路径 token：到空白或常见终止符结束
            let start = i + 1;
            let mut end = start;
            while end < chars.len() {
                let ch = chars[end];
                if ch.is_whitespace()
                    || matches!(ch, '，' | '。' | '！' | '？' | ',' | '.' | ';' | '；' | ')')
                {
                    break;
                }
                end += 1;
            }
            let token: String = chars[start..end].iter().collect();
            let replaced = match crate::tools::paths::resolve_under_roots(&roots, &token) {
                Ok(path) if path.is_file() => match std::fs::metadata(&path) {
                    Ok(m) if m.len() <= MAX_REF_SIZE => match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            let rel = crate::tools::paths::display_rel_under_work_dir(
                                std::path::Path::new(work_dir),
                                &path,
                            );
                            format!("\n[文件引用 {rel}]\n```\n{}\n```\n", content.trim())
                        }
                        Err(_) => "@".to_string() + &token,
                    },
                    _ => "@".to_string() + &token,
                },
                _ => "@".to_string() + &token,
            };
            out.push_str(&replaced);
            i = end;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// 解析 `--add-dir` 参数为绝对路径（相对路径按当前目录解析）。
/// 目录必须存在，否则报错拒绝启动。
pub fn resolve_add_dirs(add_dirs: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    if add_dirs.is_empty() {
        return Ok(Vec::new());
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut out = Vec::with_capacity(add_dirs.len());
    for d in add_dirs {
        let raw = PathBuf::from(d);
        let abs = if raw.is_absolute() {
            raw.clone()
        } else {
            cwd.join(&raw)
        };
        if !abs.exists() {
            anyhow::bail!("附加目录不存在：{d}");
        }
        out.push(std::fs::canonicalize(&abs).unwrap_or(abs));
    }
    Ok(out)
}

/// fork 目标解析：`id` 或 `id:seq`（消息序号 1-based，越界钳制）。
/// session id 只含 [A-Za-z0-9-_] 无冒号，`id:seq` 切分安全。
fn parse_fork_target(target: &str) -> (&str, Option<usize>) {
    if let Some((id, seq)) = target.rsplit_once(':')
        && let Ok(n) = seq.parse::<usize>()
    {
        return (id, Some(n));
    }
    (target, None)
}

/// fork 截断点：最后一条 User 消息之后（含）；无 User → 仅保留系统提示。
fn fork_cut_point(messages: &[session::Message]) -> usize {
    messages
        .iter()
        .rposition(|m| m.role == session::Role::User)
        .map(|i| i + 1)
        .unwrap_or(1)
}

/// 输出风格指令（/output-style compact|concise|verbose，Claude Code 语义）。
const OUTPUT_STYLE_DIRECTIVES: &[(&str, &str)] = &[
    (
        "compact",
        "回复尽量简短：只说结论与关键步骤，代码只贴改动片段，不要重复环境信息与客套话。",
    ),
    (
        "concise",
        "回复精炼：结论先行，步骤简洁，示例代码按需精简，不过度解释。",
    ),
    (
        "verbose",
        "回复详尽：完整解释推理过程、每步改动与理由，示例代码贴完整内容。",
    ),
];

/// 输出风格注入：系统提示（messages[0]）内嵌 `[Stitch output style: …--]`
/// 指令段。切换时先移除旧段再追加新段；default 仅移除（恢复默认详细度）。
fn apply_output_style(session: &mut Session, style: &str) {
    let Some(first) = session.messages.first_mut() else {
        return;
    };
    let text = first.content.text_mut();
    const MARKER: &str = "[Stitch output style:";
    if let Some(pos) = text.find(MARKER)
        && let Some(end) = text[pos..].find("--]").map(|i| pos + i + 3)
    {
        text.replace_range(pos..end, "");
    }
    let Some((_, directive)) = OUTPUT_STYLE_DIRECTIVES
        .iter()
        .find(|(name, _)| *name == style)
    else {
        return;
    };
    text.push_str(&format!(
        "\n[Stitch output style: {style}--]\n{directive}\n"
    ));
}

/// 跨会话全文搜索命中（/search）。
pub struct SearchHit {
    pub session_id: String,
    pub title: String,
    pub role: session::Role,
    pub snippet: String,
    /// 会话内消息序号（分组展示时保持原始顺序）。
    pub seq: usize,
}

/// 关键词周边窗口片段（命中位置前后各 `radius` 字符，超界省略号标记）。
fn snippet_around(content: &str, keyword: &str, radius: usize) -> String {
    let Some(pos) = content.find(keyword) else {
        return content.chars().take(160).collect();
    };
    let start = pos.saturating_sub(radius);
    let end = (pos + keyword.len() + radius).min(content.len());
    let mut s = String::new();
    if start > 0 {
        s.push('…');
    }
    s.push_str(&content[start..end]);
    if end < content.len() {
        s.push('…');
    }
    s
}

/// 跨会话全文搜索：遍历所有会话的 messages.jsonl，命中消息内容包含关键词
/// 的消息（跳过系统提示行）。返回按会话分组的命中（新会话在前）。
pub fn search_sessions(work_dir: &str, keyword: &str) -> Vec<SearchHit> {
    let titles: std::collections::HashMap<String, String> = list_sessions(work_dir)
        .into_iter()
        .map(|s| (s.id, s.title))
        .collect();
    let root = PathBuf::from(work_dir).join(".stitch").join("sessions");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let id = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(text) = std::fs::read_to_string(dir.join("messages.jsonl")) else {
            continue;
        };
        for (seq, line) in text.lines().skip(1).enumerate() {
            let Ok(m) = serde_json::from_str::<session::Message>(line) else {
                continue;
            };
            let content = m.content.text();
            if content.contains(keyword) {
                out.push(SearchHit {
                    session_id: id.clone(),
                    title: titles.get(&id).cloned().unwrap_or_default(),
                    role: m.role,
                    snippet: snippet_around(content, keyword, 60),
                    seq,
                });
            }
        }
    }
    // 按会话分组、会话内按消息顺序（read_dir 顺序不稳定，需显式排序）
    out.sort_by_key(|h| (h.session_id.clone(), h.seq));
    out
}

#[allow(clippy::too_many_arguments)] // 内部 helper：CLI 启动参数直传
pub async fn run_chat(
    mut cfg: StitchConfig,
    resume: Option<String>,
    continue_last: bool,
    model_override: Option<String>,
    add_dirs: Vec<String>,
    budget: Option<f64>,
    max_turns: Option<usize>,
    fork: Option<String>,
) -> anyhow::Result<()> {
    let api_key = cfg.require_llm_key()?.to_string();
    let work_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    let settings = crate::workspace_settings::WorkspaceSettings::load(&work_dir);
    let extra_roots = resolve_add_dirs(&add_dirs)?;
    // 会话级任务清单：TodoWrite 工具 + /todo 命令 + 回合进度行共用
    let todo_store = Arc::new(Mutex::new(tools::todo::TodoStore::new()));
    let mut tools = tools::build_registry_with_todo(&work_dir, &extra_roots, todo_store.clone());
    // MCP 工具接入：enabled 服务器（config + --mcp-config 外部合并）发现工具
    // 挂进注册表（连接失败静默跳过，不阻塞会话启动）
    let mcp_servers = crate::mcp_protocol::effective_servers(&cfg.mcp_servers);
    let mcp_tools = crate::mcp_protocol::discover_enabled(&mcp_servers).await;
    tools.attach_mcp_tools(&mcp_tools, &mcp_servers);
    let sub_ctx = tools::build_subagent_ctx(
        &cfg.llm_api_base,
        &cfg.llm_model,
        &api_key,
        cfg.max_iterations,
        Some(&work_dir),
        &tools,
        crate::agents::load_agents(Some(&work_dir)),
    );
    tools::attach_subagents(&mut tools, &sub_ctx);

    // ── 会话装载：fork / resume / continue / 新建 ──
    // fork：从历史会话截断到 fork 点（默认最后一条 User 消息），
    // 作为新会话继续（原会话保留）
    let mut forked_note: Option<String> = None;
    let session_id;
    let mut manifest;
    let (mut session, resumed) = if let Some(target) = fork {
        let (src_id, seq) = parse_fork_target(&target);
        let src_dir = agent::persist::session_dir(PathBuf::from(&work_dir).as_path(), src_id)
            .ok_or_else(|| anyhow::anyhow!("非法会话 id：{src_id}"))?;
        let (mut s, _) = agent::persist::load_session(&src_dir)?
            .ok_or_else(|| anyhow::anyhow!("会话不存在：{src_id}（stitch sessions 查看）"))?;
        let cut = seq
            .map(|n| n.clamp(1, s.messages.len()))
            .unwrap_or_else(|| fork_cut_point(&s.messages));
        s.messages.truncate(cut);
        s.iteration = 0;
        s.tokens_used = 0;
        session_id = new_session_id();
        manifest = agent::persist::Manifest::new(&session_id, PathBuf::from(&work_dir).as_path());
        forked_note = Some(format!("{src_id} → {session_id}（保留 {cut} 条消息）"));
        (s, true)
    } else {
        session_id = if let Some(ref id) = resume {
            id.clone()
        } else if continue_last {
            list_sessions(&work_dir)
                .into_iter()
                .next()
                .map(|s| s.id)
                .unwrap_or_else(new_session_id)
        } else {
            new_session_id()
        };
        let session_dir =
            agent::persist::session_dir(PathBuf::from(&work_dir).as_path(), &session_id)
                .ok_or_else(|| anyhow::anyhow!("非法会话 id"))?;
        manifest = agent::persist::Manifest::new(&session_id, PathBuf::from(&work_dir).as_path());
        match agent::persist::load_session(&session_dir) {
            Ok(Some((s, m))) => {
                manifest = m;
                (s, true)
            }
            _ if resume.is_some() => {
                // 用户明确给了 id（--resume <id> / --session-id）：不存在即报错
                anyhow::bail!("会话不存在：{session_id}（stitch sessions 查看）");
            }
            _ => {
                let mut system_prompt = agent::prompt::build_system_prompt(&work_dir, &tools);
                agent::prompt::append_additional_dirs(&mut system_prompt, &extra_roots);
                (session::Session::new(system_prompt), false)
            }
        }
    };

    let session_dir = agent::persist::session_dir(PathBuf::from(&work_dir).as_path(), &session_id)
        .ok_or_else(|| anyhow::anyhow!("非法会话 id"))?;

    let mut model = crate::workspace_settings::WorkspaceSettings::resolve_model(
        model_override.as_deref(),
        &settings,
        &cfg.llm_model,
    );
    // 输出风格（/output-style 切换；default = 不注入指令）
    let mut output_style = "default".to_string();
    let mut turn_count = 0usize;
    // 成本预算（--budget ¥）：累计回合成本，达到后提示停止（防跑飞）
    let mut total_cost: f64 = 0.0;
    // 回合耗时记录：进度提示「上轮 Xs」预估等待时间用
    let mut turn_durations: Vec<f64> = Vec::new();
    // 回合级 checkpoint：每回合前的消息快照（/rewind 恢复对话用；
    // 磁盘改动由 tools::undo 的回合边界标记负责）
    let mut turn_checkpoints: Vec<Vec<session::Message>> = Vec::new();
    // 最后一条失败的 ! 命令（/fix 用）：命令 + 输出
    let mut last_failed: Option<String> = None;
    // 草稿模式（/draft）：文件改动只预览不落盘（回合结束自动回滚）
    let mut draft_mode = false;
    // Plan 模式（/plan）：先规划 → 交互批准 → 逐步执行（Claude Code 语义）；
    // 与 /permissions mode plan 等效（两入口都触发规划回合）
    let mut plan_mode = false;
    // 当前激活的 Skill 名（/skill <名称> 加载，off 清除）
    let mut active_skill: Option<String> = None;

    // ── 终端交互 ──
    println!("{}", banner(env!("CARGO_PKG_VERSION")));
    let dir_short = work_dir
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(&work_dir);
    let extra_hint = if extra_roots.is_empty() {
        String::new()
    } else {
        format!(" · 附加目录 {}", extra_roots.len())
    };
    println!(
        "\x1b[90m  模型 {model} · 目录 {dir_short} · 权限 {}{extra_hint}\x1b[0m",
        crate::permission::current().mode.as_str()
    );
    println!();
    if let Some(note) = &forked_note {
        println!("[已分支] {note}");
    } else if resumed {
        let title = session
            .messages
            .iter()
            .find(|m| m.role == session::Role::User)
            .map(|m| session_title(m.content.text()));
        match title {
            Some(t) => println!(
                "[会话已恢复] {session_id} · {t}（{} 条消息）",
                session.messages.len()
            ),
            None => println!(
                "[会话已恢复] {session_id}（{} 条消息）",
                session.messages.len()
            ),
        }
    }
    if turn_count == 0 {
        println!("工作目录：{work_dir}");
    }
    println!();

    // Hooks：SessionStart（非零退出码 = 拒绝启动）
    {
        let hooks = crate::hooks::HookRegistry::load(Some(&work_dir));
        if hooks.has(crate::hooks::HookEvent::SessionStart) {
            let outcome = hooks
                .run(
                    crate::hooks::HookEvent::SessionStart,
                    &session_id,
                    &serde_json::json!({ "cwd": work_dir }),
                    None,
                )
                .await;
            if let Some(reason) = outcome.blocked {
                anyhow::bail!("session start rejected by hook: {reason}");
            }
        }
    }

    let mut rl = rustyline::Editor::<StitchCompleter, rustyline::history::DefaultHistory>::new()?;
    rl.set_helper(Some(StitchCompleter::new(&work_dir)));
    let history_path = crate::config::config_dir().join("history.txt");
    let _ = rl.load_history(&history_path);

    let cancel_flag = Arc::new(AtomicBool::new(false));
    // Ctrl+C：回合中置 cancel（中断）；空闲时置位后由 readline 层退出
    let cancel_for_signal = cancel_flag.clone();
    ctrlc::set_handler(move || {
        cancel_for_signal.store(true, Ordering::SeqCst);
    })
    .map_err(|e| anyhow::anyhow!("无法安装 Ctrl+C 处理：{e}"))?;

    let confirm_pending: ConfirmTable = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let allow_rules = Arc::new(Mutex::new(crate::allow::AllowRules::load()));

    loop {
        // 空闲 Ctrl+C 已置位 → 退出
        if cancel_flag.load(Ordering::SeqCst) {
            println!("\n[退出]");
            break;
        }
        // 输入框顶边（Claude Code 式）：✳ 模型 · 权限模式 · 上下文占用% · 分支，
        // 每轮输入前实时刷新（模型/权限/上下文都可能变化）
        let pc = crate::permission::current();
        let ctx_pct = {
            let limit = agent::tokens::context_limit_for_model(&model);
            let est = agent::tokens::estimate_messages(&session.messages);
            est.saturating_mul(100).checked_div(limit).unwrap_or(0)
        };
        let branch = git_branch_short(&work_dir);
        println!(
            "{}",
            input_box_top(
                &model,
                pc.mode.as_str(),
                ctx_pct,
                branch.as_deref(),
                term_width()
            )
        );
        // 提示符：左框线 │ + ❯ + 目录短名（模型/权限/上下文已上移到输入框顶边）。
        // rustyline 用 raw 算宽度（Windows 无法解析 ANSI 转义），styled 上屏渲染
        // —— 二者显示宽度必须一致，否则光标位置偏移。
        let dir_short = work_dir
            .rsplit(['/', '\\'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&work_dir);
        let (raw_prompt, styled_prompt) = build_prompt(dir_short);
        match rl.readline(&(raw_prompt.as_str(), styled_prompt.as_str())) {
            Ok(line) => {
                let _ = rl.add_history_entry(line.as_str());
                let mut line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                // 多行输入：单独一行 `{` 进入，逐行收集，单独一行 `}` 结束
                if line == "{" {
                    let mut buf = String::new();
                    println!(
                        "\x1b[90m  多行输入中：每行回车收集，单独一行 }} 结束，Ctrl+C 取消\x1b[0m"
                    );
                    let mut cancelled = false;
                    loop {
                        // 续行提示符带左框线（与输入框一体）
                        match rl.readline(&("│ … ", "\x1b[90m│\x1b[0m \x1b[1;36m…\x1b[0m "))
                        {
                            Ok(l) => {
                                if l.trim() == "}" {
                                    break;
                                }
                                buf.push_str(&l);
                                buf.push('\n');
                            }
                            Err(_) => {
                                cancelled = true;
                                break;
                            }
                        }
                    }
                    if cancelled || buf.is_empty() {
                        println!("\x1b[90m  已取消\x1b[0m");
                        continue;
                    }
                    line = buf.trim_end().to_string();
                }
                // ! 快捷执行 shell 命令（Claude Code 语义）：直接执行，
                // 输出并入本次用户消息（模型可见）；失败记录供 /fix 用
                if line.starts_with('!') && !line.starts_with("!!") {
                    let cmd = line[1..].trim().to_string();
                    if cmd.is_empty() {
                        println!("  用法：!命令 —— 直接执行 shell 命令，输出并入上下文");
                        continue;
                    }
                    let (output, ok) = run_shell_output(&cmd, &work_dir).await?;
                    if !ok {
                        last_failed = Some(format!(
                            "用户执行了命令 `{cmd}` 但失败了，输出如下：\n\n{output}"
                        ));
                    }
                    line = format!(
                        "用户执行了命令 `{cmd}`，以下是输出（失败时含错误信息）：\n\n{output}"
                    );
                    println!("\x1b[90m  ! {cmd}\x1b[0m");
                }
                // @文件引用：@路径 把文件内容嵌入消息（Claude Code 语义，
                // 相对主工作目录解析，附加根也可引用）
                if !line.starts_with('/') && line.contains('@') {
                    line = expand_at_references(&line, &work_dir, &extra_roots);
                }
                if line.starts_with('/') {
                    let mut custom_prompt: Option<String> = None;
                    match parse_slash(&line) {
                        SlashAction::Exit => {
                            println!("[退出]");
                            break;
                        }
                        SlashAction::Clear => {
                            let _ =
                                agent::persist::save_session(&session_dir, &session, &mut manifest);
                            let new_id = new_session_id();
                            let mut system_prompt =
                                agent::prompt::build_system_prompt(&work_dir, &tools);
                            agent::prompt::append_additional_dirs(&mut system_prompt, &extra_roots);
                            session = session::Session::new(system_prompt);
                            turn_checkpoints.clear(); // 新会话无可回滚的历史
                            println!("[新会话] 上一个会话已保存为 {session_id}，当前 {new_id}");
                            let _ = session_id; // 保留旧 id 供提示
                            let _ = new_id;
                        }
                        SlashAction::Retry => {
                            // Claude Code 语义：重新生成上次回答——截断到
                            // 最后一条 user 消息（含），重跑同一回合
                            let Some(pos) = session
                                .messages
                                .iter()
                                .rposition(|m| m.role == session::Role::User)
                            else {
                                println!("  [retry] 没有可重试的用户消息");
                                continue;
                            };
                            let last_user = session.messages[pos].content.text().to_string();
                            if last_user.trim().is_empty() {
                                println!("  [retry] 没有可重试的用户消息");
                                continue;
                            }
                            session.messages.truncate(pos + 1);
                            println!("  [retry] 重新生成上次回答：{last_user}");
                            line = last_user;
                        }
                        SlashAction::Draft(args) => {
                            let new_state = match args.first().map(String::as_str) {
                                Some("on") => Some(true),
                                Some("off") => Some(false),
                                None => Some(!draft_mode),
                                _ => {
                                    println!("  [draft] 用法：/draft [on|off]");
                                    continue;
                                }
                            };
                            draft_mode = new_state.unwrap();
                            println!(
                                "  [draft] 草稿模式已{}——文件改动只预览不落盘（命令照常执行）",
                                if draft_mode { "开启" } else { "关闭" }
                            );
                            continue;
                        }
                        SlashAction::Plan(args) => {
                            let switch = args.first().map(String::as_str);
                            match switch {
                                Some("on") => {
                                    plan_mode = true;
                                    println!("  [plan] 规划模式已开启——每回合先出计划，批准后执行");
                                    continue;
                                }
                                Some("off") => {
                                    plan_mode = false;
                                    println!("  [plan] 规划模式已关闭");
                                    continue;
                                }
                                Some(desc) => {
                                    // /plan <描述>：开启规划模式并立即以描述为任务
                                    plan_mode = true;
                                    println!("  [plan] 规划模式已开启——先出计划，批准后执行");
                                    line = desc.to_string();
                                }
                                None => {
                                    plan_mode = !plan_mode;
                                    println!(
                                        "  [plan] 规划模式已{}",
                                        if plan_mode { "开启" } else { "关闭" }
                                    );
                                    continue;
                                }
                            }
                        }
                        SlashAction::Todo(args) => {
                            if args.first().map(String::as_str) == Some("clear") {
                                if let Ok(mut store) = todo_store.lock() {
                                    store.clear();
                                    println!("  [todo] 已清空任务清单");
                                }
                                continue;
                            }
                            let Ok(store) = todo_store.lock() else {
                                continue;
                            };
                            let items = store.list();
                            if items.is_empty() {
                                println!("  [todo] 暂无任务（模型用 TodoWrite 工具维护任务清单）");
                                continue;
                            }
                            println!(
                                "  [todo] 任务清单（{}/{} 完成）：",
                                store.done_count(),
                                items.len()
                            );
                            for item in &items {
                                let mark = if item.done {
                                    "✓"
                                } else if item.in_progress {
                                    "▶"
                                } else {
                                    " "
                                };
                                println!("    [{mark}] {} {}", item.id, item.content);
                            }
                            println!("  用 /todo clear 清空清单");
                        }
                        SlashAction::OutputStyle(args) => {
                            let style = args.first().map(String::as_str).unwrap_or("");
                            if style.is_empty() {
                                println!(
                                    "  [风格] 当前：{output_style} · 可用：default / compact / concise / verbose"
                                );
                                println!(
                                    "  例：/output-style concise（指令注入系统提示，仅本会话生效，/reset 后恢复 default）"
                                );
                                continue;
                            }
                            if !matches!(style, "default" | "compact" | "concise" | "verbose") {
                                println!(
                                    "  [风格] 未知风格「{style}」· 可用：default / compact / concise / verbose"
                                );
                                continue;
                            }
                            apply_output_style(&mut session, style);
                            output_style = style.to_string();
                            println!(
                                "  [风格] → {output_style}{}",
                                if output_style == "default" {
                                    "（恢复默认详细度）"
                                } else {
                                    ""
                                }
                            );
                            let _ =
                                agent::persist::save_session(&session_dir, &session, &mut manifest);
                        }
                        SlashAction::Statusline(args) => {
                            let mut it = args.iter();
                            let first = it.next().map(String::as_str).unwrap_or("");
                            match first {
                                "" => {
                                    let cur = crate::statusline::resolved(
                                        settings.statusline.as_deref(),
                                        cfg.statusline.as_deref(),
                                    )
                                    .unwrap_or_else(|| "（未设置）".into());
                                    println!("  [状态行] 当前：{cur}");
                                    println!(
                                        "  用法：/statusline clear 清除 · /statusline set <shell 命令> 自定义"
                                    );
                                    println!(
                                        "  快捷：/statusline time（时间）· dir（目录）· branch（git 分支）"
                                    );
                                }
                                "clear" => {
                                    cfg.statusline = None;
                                    cfg.save()?;
                                    println!("  [状态行] 已清除（恢复默认无状态行）");
                                }
                                "set" => {
                                    let value: Vec<String> = it.cloned().collect();
                                    if value.is_empty() {
                                        println!(
                                            "  [状态行] 用法：/statusline set <shell 命令>（每回合后执行，输出即状态行）"
                                        );
                                        continue;
                                    }
                                    let value = value.join(" ");
                                    cfg.statusline = Some(value.clone());
                                    cfg.save()?;
                                    println!("  [状态行] 已设置：{value}");
                                }
                                "time" | "dir" | "branch" => {
                                    let value = match first {
                                        "time" => "date \"+%H:%M\"",
                                        "dir" => "pwd",
                                        _ => "git rev-parse --abbrev-ref HEAD",
                                    };
                                    cfg.statusline = Some(value.into());
                                    cfg.save()?;
                                    println!("  [状态行] 已设置：{value}");
                                }
                                other => println!(
                                    "  [状态行] 未知参数「{other}」：/statusline [clear | set <命令> | time | dir | branch]"
                                ),
                            }
                        }
                        SlashAction::Search(keyword) => {
                            if keyword.is_empty() {
                                println!("  [搜索] 用法：/search <关键词>（跨全部会话全文搜索）");
                                continue;
                            }
                            let hits = search_sessions(&work_dir, &keyword);
                            if hits.is_empty() {
                                println!("  [搜索] 「{keyword}」无命中");
                                continue;
                            }
                            println!(
                                "  [搜索] 「{keyword}」命中 {} 条（显示前 20 条）：",
                                hits.len()
                            );
                            let mut shown = 0usize;
                            let mut last_id: Option<&str> = None;
                            for hit in hits.iter().take(20) {
                                shown += 1;
                                if last_id != Some(hit.session_id.as_str()) {
                                    last_id = Some(&hit.session_id);
                                    println!(
                                        "    ── {}（{id}）",
                                        if hit.title.is_empty() {
                                            &hit.session_id
                                        } else {
                                            &hit.title
                                        },
                                        id = hit.session_id
                                    );
                                }
                                let role = match hit.role {
                                    session::Role::User => "你",
                                    session::Role::Assistant => "Stitch",
                                    session::Role::System => "系统",
                                    session::Role::Tool => "工具",
                                };
                                println!("      [{role}] {}", hit.snippet);
                            }
                            if hits.len() > shown {
                                println!(
                                    "    … 其余 {} 条略（结果按会话分组，新会话在前）",
                                    hits.len() - shown
                                );
                            }
                        }
                        SlashAction::Think(args) => {
                            let mode = args.first().map(String::as_str).unwrap_or("");
                            match mode {
                                "" => println!(
                                    "  [思考] 当前：{} · /think on 显示模型思考过程（费 token）· /think off 恢复默认",
                                    if crate::llm::thinking_enabled() {
                                        "开"
                                    } else {
                                        "关"
                                    }
                                ),
                                "on" => {
                                    crate::llm::set_thinking(true);
                                    println!(
                                        "  [思考] 已开启——从下一回合起显示思考过程（浅色，不入会话记录）"
                                    );
                                }
                                "off" => {
                                    crate::llm::set_thinking(false);
                                    println!("  [思考] 已关闭（默认）");
                                }
                                other => println!("  [思考] 未知参数「{other}」：/think [on|off]"),
                            }
                        }
                        SlashAction::Rewind => match turn_checkpoints.pop() {
                            Some(prev_messages) => {
                                session.messages = prev_messages;
                                let _ = agent::persist::save_session(
                                    &session_dir,
                                    &session,
                                    &mut manifest,
                                );
                                match tools::undo::undo_until_marker() {
                                    Ok(descs) => {
                                        println!(
                                            "[回滚] 已恢复到上一回合（{} 条消息）",
                                            session.messages.len()
                                        );
                                        for d in descs {
                                            println!("  · {d}");
                                        }
                                    }
                                    Err(e) => println!("[回滚] 消息已恢复，但文件回滚失败：{e}"),
                                }
                            }
                            None => println!("[回滚] 没有可回滚的回合"),
                        },
                        SlashAction::Model(m) => {
                            let picked = if m.is_empty() {
                                // Claude Code 语义：/model 无参数 → 选择器
                                pick_model(&model)?
                            } else {
                                m
                            };
                            model = crate::config::migrate_llm_model(&picked)
                                .map(str::to_string)
                                .unwrap_or(picked);
                            println!("[模型] → {model}");
                        }
                        SlashAction::Cost => {
                            print_session_cost(&session, &model, &work_dir);
                            // Notification hook：提示宿主显示成本（Claude Code 语义）
                            let hooks = crate::hooks::HookRegistry::load(Some(&work_dir));
                            if hooks.has(crate::hooks::HookEvent::Notification) {
                                let usage = agent::tokens::TokenUsage {
                                    input_tokens: agent::tokens::estimate_messages(
                                        &session.messages,
                                    ),
                                    output_tokens: 0,
                                    cache_hit_tokens: 0,
                                    cache_miss_tokens: 0,
                                };
                                let _ = hooks
                                    .run(
                                        crate::hooks::HookEvent::Notification,
                                        &session_id,
                                        &serde_json::json!({
                                            "message": format!(
                                                "Cost: ¥{:.4}",
                                                agent::tokens::estimate_cost(&usage, &model)
                                            ),
                                        }),
                                        None,
                                    )
                                    .await;
                            }
                        }
                        SlashAction::Context => {
                            print_context_usage(&session, &model);
                        }
                        SlashAction::Compact => {
                            let ctx_limit = agent::tokens::context_limit_for_model(&model);
                            let hard_lim = agent::persist::hard_token_limit(ctx_limit);
                            // PreCompact hook：压缩前拦截（拒绝则跳过）
                            let est = agent::tokens::estimate_messages(&session.messages);
                            if let Some(reason) = crate::hooks::pre_compact_blocked(
                                Some(&work_dir),
                                session.messages.len(),
                                est,
                            )
                            .await
                            {
                                println!("[压缩] 被 hook 拒绝：{reason}");
                            } else {
                                let before = session.messages.len();
                                println!("[压缩] 正在整理上下文…");
                                let compacted = agent::context::maybe_compact_llm(
                                    &mut session,
                                    &agent::context::ContextConfig {
                                        max_tokens: hard_lim,
                                        keep_recent: agent::context::ContextConfig::default()
                                            .keep_recent,
                                    },
                                    Some(agent::context::CompactLlm {
                                        api_base: &cfg.llm_api_base,
                                        model: &model,
                                        api_key: &api_key,
                                    }),
                                )
                                .await;
                                if compacted {
                                    let _ = agent::persist::save_session(
                                        &session_dir,
                                        &session,
                                        &mut manifest,
                                    );
                                    println!(
                                        "[压缩] 完成：{} → {} 条消息",
                                        before,
                                        session.messages.len()
                                    );
                                } else {
                                    println!("[压缩] 无需压缩（上下文未超限）");
                                }
                            }
                        }
                        SlashAction::Export(path) => {
                            let path =
                                path.unwrap_or_else(|| format!("stitch-export-{session_id}.md"));
                            let p = std::path::PathBuf::from(&path);
                            match export_session(&session, &p) {
                                Ok(()) => println!("[导出] {}", p.display()),
                                Err(e) => println!("[导出失败] {e}"),
                            }
                        }
                        SlashAction::Permissions(args) => {
                            handle_permissions(&allow_rules, &mut cfg, &args)?;
                        }
                        SlashAction::Sessions => {
                            for s in list_sessions(&work_dir) {
                                println!(
                                    "  {}\t{}\t{} 条\t{}",
                                    s.id, s.updated_at, s.msg_count, s.title
                                );
                            }
                        }
                        SlashAction::Agents => {
                            let agents = crate::agents::load_agents(Some(&work_dir));
                            if agents.is_empty() {
                                println!(
                                    "[子代理] 无定义——在 .claude/agents/*.md 或 config_dir/agents/*.md 添加"
                                );
                            }
                            for a in agents {
                                let tools = match &a.tools {
                                    Some(list) => format!("工具：{}", list.join(", ")),
                                    None => "工具：全部".into(),
                                };
                                let model = match &a.model {
                                    Some(m) => format!(" · 模型：{m}"),
                                    None => String::new(),
                                };
                                println!("  {}——{}{}", a.name, a.description, model);
                                println!("      {tools}");
                            }
                        }
                        SlashAction::Mcp(args) => {
                            let args = args.trim();
                            if args.is_empty() {
                                // 健康视图：逐个连接生效服务器（config + 外部合并）
                                let servers =
                                    crate::mcp_protocol::effective_servers(&cfg.mcp_servers);
                                if servers.is_empty() {
                                    println!(
                                        "  MCP 服务器：无（/mcp add <名称> <命令> 或 <名称> --url <端点> 添加）"
                                    );
                                } else {
                                    println!("  MCP 服务器（{}）：", servers.len());
                                    for p in servers {
                                        let url =
                                            p.url.as_deref().or(p.command.as_deref()).unwrap_or("");
                                        match crate::mcp_protocol::list_tools(&p).await {
                                            Ok(tools) => println!(
                                                "    \x1b[32m✓\x1b[0m {} — {}（{} 工具）",
                                                p.label,
                                                url,
                                                tools.len()
                                            ),
                                            Err(e) => println!(
                                                "    \x1b[31m✗\x1b[0m {} — {}（连接失败：{e}）",
                                                p.label, url
                                            ),
                                        }
                                    }
                                }
                                continue;
                            }
                            let (verb, arg) = match args.split_once(char::is_whitespace) {
                                Some((v, a)) => (v, a.trim()),
                                None => (args, ""),
                            };
                            let apply = |cfg: &mut StitchConfig| -> anyhow::Result<()> {
                                match verb {
                                    "list" => {
                                        if cfg.mcp_servers.is_empty() {
                                            println!("  暂无 MCP 服务器配置");
                                        } else {
                                            println!("  已配置的 MCP 服务器：");
                                            for s in &cfg.mcp_servers {
                                                let target = match s.transport.as_str() {
                                                    "stdio" => {
                                                        s.command.clone().unwrap_or_default()
                                                    }
                                                    _ => s.url.clone().unwrap_or_default(),
                                                };
                                                println!(
                                                    "    {}  [{}]  {}  {}",
                                                    s.id,
                                                    if s.enabled { "开" } else { "关" },
                                                    s.transport,
                                                    target
                                                );
                                            }
                                        }
                                    }
                                    "add" => {
                                        let (name, spec) = match arg.split_once(char::is_whitespace)
                                        {
                                            Some((n, s)) => (n, s.trim()),
                                            None => (arg, ""),
                                        };
                                        if name.is_empty() || spec.is_empty() {
                                            println!(
                                                "  用法：/mcp add <名称> <命令>  或  /mcp add <名称> --url <端点>"
                                            );
                                            return Ok(());
                                        }
                                        use crate::config::McpServerProfile;
                                        let profile = if let Some(u) = spec.strip_prefix("--url ") {
                                            McpServerProfile {
                                                id: name.to_string(),
                                                label: name.to_string(),
                                                transport: if u.starts_with("http") {
                                                    "http".into()
                                                } else {
                                                    "sse".into()
                                                },
                                                enabled: true,
                                                command: None,
                                                args: Vec::new(),
                                                env: std::collections::HashMap::new(),
                                                cwd: None,
                                                url: Some(u.to_string()),
                                                headers: std::collections::HashMap::new(),
                                            }
                                        } else {
                                            let mut parts = spec.split_whitespace();
                                            let bin = parts.next().unwrap_or_default();
                                            McpServerProfile {
                                                id: name.to_string(),
                                                label: name.to_string(),
                                                transport: "stdio".into(),
                                                enabled: true,
                                                command: Some(bin.to_string()),
                                                args: parts.map(str::to_string).collect(),
                                                env: std::collections::HashMap::new(),
                                                cwd: None,
                                                url: None,
                                                headers: std::collections::HashMap::new(),
                                            }
                                        };
                                        cfg.upsert_mcp_server(profile)?;
                                        cfg.save()?;
                                        println!(
                                            "  [已添加] MCP 服务器 `{name}`（当前会话连接后生效）"
                                        );
                                    }
                                    "remove" | "rm" => {
                                        if arg.is_empty() {
                                            println!("  用法：/mcp remove <名称>");
                                            return Ok(());
                                        }
                                        match cfg.delete_mcp_server(arg) {
                                            Ok(()) => {
                                                cfg.save()?;
                                                println!("  [已移除] MCP 服务器 `{arg}`");
                                            }
                                            Err(e) => println!("  {e}"),
                                        }
                                    }
                                    "on" | "off" => {
                                        if arg.is_empty() {
                                            println!("  用法：/mcp {verb} <名称>");
                                            return Ok(());
                                        }
                                        match cfg.set_mcp_server_enabled(arg, verb == "on") {
                                            Ok(()) => {
                                                cfg.save()?;
                                                println!(
                                                    "  [已{}] MCP 服务器 `{arg}`",
                                                    if verb == "on" { "启用" } else { "停用" }
                                                );
                                            }
                                            Err(e) => println!("  {e}"),
                                        }
                                    }
                                    _ => {
                                        println!(
                                            "  用法：/mcp [list|add <名称> <命令|--url 端点>|remove <名称>|on|off <名称>]"
                                        );
                                    }
                                }
                                Ok(())
                            };
                            if let Err(e) = apply(&mut cfg) {
                                println!("  \x1b[31m{e}\x1b[0m");
                            }
                        }
                        SlashAction::Hooks => {
                            let hooks = crate::hooks::HookRegistry::load(Some(&work_dir));
                            let (global, ws) = hooks.inspect();
                            let ws_path = std::path::Path::new(&work_dir)
                                .join(".stitch")
                                .join("hooks.json");
                            println!(
                                "  全局 hooks：{}",
                                crate::config::config_dir().join("hooks.json").display()
                            );
                            println!(
                                "  工作区 hooks：{}",
                                if ws_path.exists() {
                                    ws_path.display().to_string()
                                } else {
                                    "（未配置）".to_string()
                                }
                            );
                            println!("{}", crate::hooks::summarize(global, Some(ws)));
                        }
                        SlashAction::Config(arg) => {
                            // /config key=value 带内改设置（Claude Code 2.1.181+ 语义）
                            if let Some(eq) = arg.find('=') {
                                let (key, value) = (arg[..eq].trim(), arg[eq + 1..].trim());
                                if key.is_empty() {
                                    println!(
                                        "[用法] /config <键>=<值>（如 /config max_iterations=20）"
                                    );
                                    continue;
                                }
                                match cfg.set(key, value) {
                                    Ok(()) => {
                                        if let Err(e) = cfg.save() {
                                            println!("[保存失败] {e}");
                                        } else if key == "llm_api_key" || key == "api_token" {
                                            println!("[已设置] {key} = ****");
                                        } else {
                                            println!("[已设置] {key} = {value}");
                                        }
                                    }
                                    Err(e) => println!("[设置失败] {e}"),
                                }
                                continue;
                            }
                            if !arg.is_empty() {
                                println!("[用法] /config <键>=<值>；无参数显示当前配置");
                                continue;
                            }
                            let pc = crate::permission::current();
                            let mode = pc.mode.as_str();
                            let hooks_dir = crate::config::config_dir();
                            let ws_hooks = std::path::Path::new(&work_dir)
                                .join(".stitch")
                                .join("hooks.json");
                            let st = crate::statusline::resolved(
                                settings.statusline.as_deref(),
                                cfg.statusline.as_deref(),
                            )
                            .unwrap_or_else(|| "（未配置）".into());
                            println!("  当前配置：");
                            println!("    模型：{model}");
                            println!("    API：{}", cfg.llm_api_base);
                            println!("    最大迭代：{}", cfg.max_iterations);
                            println!("    权限模式：{mode}");
                            println!("    statusLine：{st}");
                            println!(
                                "    MCP 服务器：{}",
                                if cfg.mcp_servers.is_empty() {
                                    "（无）".to_string()
                                } else {
                                    cfg.mcp_servers
                                        .iter()
                                        .map(|m| m.label.clone())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                }
                            );
                            println!("    hooks 配置：{}", hooks_dir.join("hooks.json").display());
                            if ws_hooks.exists() {
                                println!("    hooks（工作区）：{}", ws_hooks.display());
                            }
                            println!(
                                "    配置文件：{}",
                                crate::config::config_dir().join("config.toml").display()
                            );
                            println!(
                                "  编辑入口：/model 切模型 · /permissions 权限 · 其他项改 config.toml"
                            );
                        }
                        SlashAction::Help => {
                            print!("{SLASH_HELP}");
                            let cmds = crate::commands::load_commands(Some(&work_dir));
                            if !cmds.is_empty() {
                                println!(
                                    "
  自定义命令（.claude/commands/*.md）："
                                );
                                for c in cmds {
                                    let hint = c.argument_hint.as_deref().unwrap_or("");
                                    let desc = if c.description.is_empty() {
                                        "自定义命令".to_string()
                                    } else {
                                        c.description
                                    };
                                    println!("  /{}{}    {desc}", c.name, hint);
                                }
                            }
                        }
                        SlashAction::Custom(name, args) => {
                            let cmds = crate::commands::load_commands(Some(&work_dir));
                            match cmds.iter().find(|c| c.name == name) {
                                Some(def) => custom_prompt = Some(def.render(&args)),
                                None => println!("未知命令 /{name}，输入 /help 查看可用命令"),
                            }
                        }
                        SlashAction::Upgrade => {
                            println!("  [upgrade] 检查并升级…");
                            match crate::upgrade::run().await {
                                Ok(()) => println!("  [upgrade] 完成"),
                                Err(e) => println!("  [upgrade] \x1b[31m{e}\x1b[0m"),
                            }
                        }
                        SlashAction::Init => match crate::cmd_init().await {
                            Ok(()) => {}
                            Err(e) => println!("  \x1b[31m{e}\x1b[0m"),
                        },
                        SlashAction::Profile(args) => {
                            cfg.ensure_llm_profiles_seeded();
                            match args.as_slice() {
                                [] => {
                                    if cfg.llm_profiles.len() <= 1 {
                                        println!(
                                            "  当前只有默认配置（{label} · {model}）——用 `stitch config` 向导换模型/Key 后，\n  再次 `stitch config` 可另存新配置（config 向导第 5 项「保存为新配置」）。",
                                            label = cfg.llm_profiles[0].label,
                                            model = cfg.llm_profiles[0].model,
                                        );
                                        continue;
                                    }
                                    println!("  已保存的模型配置：");
                                    for (i, p) in cfg.llm_profiles.iter().enumerate() {
                                        let active =
                                            cfg.active_profile_id.as_deref() == Some(&p.id);
                                        println!(
                                            "    {}. {} {label} · {model} · {base} · {key}",
                                            i + 1,
                                            if active { "✓" } else { " " },
                                            label = p.label,
                                            model = p.model,
                                            base = p.api_base,
                                            key = if p.api_key.is_some() {
                                                "有 Key"
                                            } else {
                                                "无 Key"
                                            },
                                        );
                                    }
                                    if std::io::stdin().is_terminal() {
                                        use dialoguer::{Select, theme::ColorfulTheme};
                                        let items: Vec<String> = cfg
                                            .llm_profiles
                                            .iter()
                                            .map(|p| {
                                                format!(
                                                    "{} · {} · {}",
                                                    p.label,
                                                    p.model,
                                                    if p.api_key.is_some() {
                                                        "有 Key"
                                                    } else {
                                                        "无 Key"
                                                    }
                                                )
                                            })
                                            .collect();
                                        let current = cfg
                                            .active_profile_id
                                            .as_deref()
                                            .and_then(|id| {
                                                cfg.llm_profiles.iter().position(|p| p.id == id)
                                            })
                                            .unwrap_or(0);
                                        let idx = Select::with_theme(&ColorfulTheme::default())
                                            .with_prompt("切换模型配置")
                                            .default(current)
                                            .items(&items)
                                            .interact()?;
                                        let id = cfg.llm_profiles[idx].id.clone();
                                        cfg.activate_profile(&id)?;
                                        cfg.save()?;
                                        model = cfg.llm_model.clone();
                                        println!(
                                            "  [已切换] {} · {}",
                                            cfg.llm_profiles[idx].label, cfg.llm_model
                                        );
                                    } else {
                                        println!("  用法：/profile <编号或 id> 直接切换");
                                    }
                                }
                                [sel] => {
                                    // 支持编号（1-based）或 id/label 匹配
                                    let target = sel
                                        .parse::<usize>()
                                        .ok()
                                        .and_then(|n| cfg.llm_profiles.get(n - 1))
                                        .map(|p| p.id.clone())
                                        .or_else(|| {
                                            cfg.llm_profiles
                                                .iter()
                                                .find(|p| p.id == *sel || p.label == *sel)
                                                .map(|p| p.id.clone())
                                        });
                                    match target {
                                        Some(id) => {
                                            cfg.activate_profile(&id)?;
                                            cfg.save()?;
                                            model = cfg.llm_model.clone();
                                            println!(
                                                "  [已切换] {} · {}",
                                                cfg.profile(&id)
                                                    .map(|p| p.label.as_str())
                                                    .unwrap_or(&id),
                                                cfg.llm_model
                                            );
                                        }
                                        None => {
                                            println!(
                                                "  [错误] 找不到模型配置：{sel}（/profile 查看列表）"
                                            )
                                        }
                                    }
                                }
                                _ => println!("  用法：/profile [编号或 id]"),
                            }
                        }
                        SlashAction::Skill(args) => {
                            let skill_dir = PathBuf::from(&work_dir).join(".agents").join("skills");
                            match args.as_slice() {
                                [] => {
                                    // 列表 + 当前激活
                                    println!(
                                        "  已加载 Skill：{}",
                                        active_skill
                                            .as_deref()
                                            .unwrap_or("（无，/skill <名称> 加载）")
                                    );
                                    let mut names: Vec<String> = Vec::new();
                                    if let Ok(rd) = std::fs::read_dir(&skill_dir) {
                                        for e in rd.flatten() {
                                            if e.path().join("SKILL.md").is_file() {
                                                names.push(
                                                    e.file_name().to_string_lossy().into_owned(),
                                                );
                                            }
                                        }
                                    }
                                    names.sort();
                                    if names.is_empty() {
                                        println!(
                                            "  可用 Skill：无（.agents/skills/ 下放 SKILL.md，或让模型 save_skill 沉淀）"
                                        );
                                    } else {
                                        println!(
                                            "  可用 Skill（{}）：{}",
                                            names.len(),
                                            names.join(" · ")
                                        );
                                    }
                                }
                                [name] if name == "off" => {
                                    let base =
                                        agent::prompt::build_system_prompt(&work_dir, &tools);
                                    let mut sp = base;
                                    agent::prompt::append_additional_dirs(&mut sp, &extra_roots);
                                    session.messages[0].content = session::Content::Text(sp);
                                    active_skill = None;
                                    println!("  [skill] 已清除（系统提示恢复默认）");
                                }
                                [name] => {
                                    // slug 校验：仅字母数字下划线连字符，防路径穿越
                                    if !name
                                        .chars()
                                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                                    {
                                        println!("  [skill] 非法名称（仅字母/数字/_/-）");
                                        continue;
                                    }
                                    let path = skill_dir.join(name).join("SKILL.md");
                                    match std::fs::read_to_string(&path) {
                                        Ok(content) => {
                                            let with_skill =
                                                agent::prompt::build_system_prompt_with_skill(
                                                    &work_dir,
                                                    &tools,
                                                    Some(&content),
                                                );
                                            let mut sp = with_skill;
                                            agent::prompt::append_additional_dirs(
                                                &mut sp,
                                                &extra_roots,
                                            );
                                            session.messages[0].content =
                                                session::Content::Text(sp);
                                            active_skill = Some(name.clone());
                                            println!(
                                                "  [skill] 已加载「{name}」（系统提示注入 Active Skill 段）"
                                            );
                                        }
                                        Err(_) => {
                                            println!(
                                                "  [skill] 未找到 Skill「{name}」（.agents/skills/{name}/SKILL.md，/skill 查看列表）"
                                            );
                                        }
                                    }
                                }
                                _ => println!(
                                    "  用法：/skill <名称> 加载 · /skill off 清除 · /skill 列表"
                                ),
                            }
                        }
                        SlashAction::Review => {
                            let diff = tools::git::collect_uncommitted_diff(&work_dir).await;
                            if diff.is_empty() {
                                println!("  [review] 没有未提交的改动（git diff HEAD 为空）");
                                continue;
                            }
                            // 落入普通回合：模型可读文件核实，按严重程度输出问题
                            custom_prompt = Some(format!(
                                "请审查以下未提交的 git 改动。找出 bug、安全问题、逻辑遗漏与风格问题，\
                                 按严重程度从高到低列出，每条给出文件位置和修改建议。\
                                 需要更多上下文时自行读取相关文件核实。\n\n<diff>\n{diff}\n</diff>"
                            ));
                        }
                        SlashAction::Fix => match last_failed.as_ref() {
                            Some(info) => {
                                // 落入普通回合：模型诊断并修复（改文件/重跑命令）
                                custom_prompt = Some(format!(
                                    "{info}\n\n请诊断失败原因并修复：检查命令与相关代码，直接修改或给出修复后的命令，\
                                     修复后重新执行验证是否成功。"
                                ));
                            }
                            None => {
                                println!(
                                    "  [fix] 没有失败的 ! 命令记录（!命令失败后可用 /fix 让模型修复）"
                                );
                                continue;
                            }
                        },
                        SlashAction::Memory(args) => match args.first().map(String::as_str) {
                            Some("open") => {
                                let which = args.get(1).map(String::as_str).unwrap_or("");
                                match memory_layer_path(&work_dir, which) {
                                    Some(path) => {
                                        if !path.exists() {
                                            println!(
                                                "  [memory] 文件不存在：{}（用 /memory create {which} 创建）",
                                                path.display()
                                            );
                                            continue;
                                        }
                                        match open_editor(&path) {
                                            Ok(()) => {
                                                println!("  [memory] 已编辑：{}", path.display())
                                            }
                                            Err(e) => {
                                                println!("  [memory] 打开编辑器失败：{e}")
                                            }
                                        }
                                    }
                                    None => {
                                        println!(
                                            "  [memory] 用法：/memory open <local|project|global>"
                                        )
                                    }
                                }
                            }
                            Some("create") => {
                                let which = args.get(1).map(String::as_str).unwrap_or("");
                                match memory_layer_path(&work_dir, which) {
                                    Some(path)
                                        if which == "local"
                                            || which == "project"
                                            || which == "global" =>
                                    {
                                        if path.exists() {
                                            println!(
                                                "  [memory] 已存在：{}（用 /memory open {which} 编辑）",
                                                path.display()
                                            );
                                            continue;
                                        }
                                        let template = if which == "local" {
                                            "# CLAUDE.local.md\n\n本机私有记忆——不提交版本库的内容放这里（已在 .gitignore）。\n"
                                                .to_string()
                                        } else if which == "project" {
                                            "# CLAUDE.md\n\n项目记忆——团队共享的规范与上下文（提交版本库）。\n"
                                                .to_string()
                                        } else {
                                            "# CLAUDE.md\n\n全局记忆——所有项目的用户级记忆（config_dir）。\n"
                                                .to_string()
                                        };
                                        if let Err(e) = std::fs::write(&path, template) {
                                            println!("  [memory] 创建失败：{e}");
                                            continue;
                                        }
                                        println!("  [memory] 已创建：{}", path.display());
                                        match open_editor(&path) {
                                            Ok(()) => {
                                                println!("  [memory] 已编辑：{}", path.display())
                                            }
                                            Err(e) => {
                                                println!("  [memory] 打开编辑器失败：{e}")
                                            }
                                        }
                                    }
                                    _ => {
                                        println!(
                                            "  [memory] 用法：/memory create <local|project|global>"
                                        )
                                    }
                                }
                            }
                            Some("delete") => {
                                let which = args.get(1).map(String::as_str).unwrap_or("");
                                match memory_layer_path(&work_dir, which) {
                                    Some(path) => {
                                        if !path.exists() {
                                            println!("  [memory] 文件不存在：{}", path.display());
                                            continue;
                                        }
                                        if crate::render::dialog::confirm(&format!(
                                            "删除记忆文件 {}？",
                                            path.display()
                                        )) {
                                            match std::fs::remove_file(&path) {
                                                Ok(()) => println!("  [memory] 已删除"),
                                                Err(e) => println!("  [memory] 删除失败：{e}"),
                                            }
                                        } else {
                                            println!("  [memory] 已取消");
                                        }
                                    }
                                    None => {
                                        println!(
                                            "  [memory] 用法：/memory delete <local|project|global>"
                                        )
                                    }
                                }
                            }
                            _ => print_memory_layers(&work_dir),
                        },
                        SlashAction::Inspect => {
                            // 完整系统提示 = 会话初始化同源组装（记忆 + 工具 + 规则 + 附加目录）
                            let mut sp = agent::prompt::build_system_prompt(&work_dir, &tools);
                            agent::prompt::append_additional_dirs(&mut sp, &extra_roots);
                            let sections: Vec<&str> =
                                sp.lines().filter(|l| l.starts_with("## ")).collect();
                            println!(
                                "[inspect] 系统提示共 {} 字符 · {} 节（记忆/工具/规则注入核查用）：",
                                sp.len(),
                                sections.len()
                            );
                            for s in &sections {
                                println!("  {s}");
                            }
                            println!();
                            println!("{sp}");
                        }
                        SlashAction::Unknown(msg) => println!("{msg}"),
                    }
                    // 自定义命令命中 → 渲染正文走普通回合；否则回到输入
                    match custom_prompt {
                        Some(prompt) => line = prompt,
                        None => continue,
                    }
                }

                // Hooks：UserPromptSubmit（block 则丢弃该输入，不产生回合）
                {
                    let hooks = crate::hooks::HookRegistry::load(Some(&work_dir));
                    if hooks.has(crate::hooks::HookEvent::UserPromptSubmit) {
                        let outcome = hooks
                            .run(
                                crate::hooks::HookEvent::UserPromptSubmit,
                                &session_id,
                                &serde_json::json!({ "prompt": line, "cwd": work_dir }),
                                None,
                            )
                            .await;
                        if let Some(reason) = outcome.blocked {
                            println!("[输入被 hook 拒绝] {reason}");
                            continue;
                        }
                    }
                }

                // ── 回合：用户消息 → agent → 流式渲染 ──
                // 回合前 checkpoint：消息快照 + undo 回合边界（/rewind 用）
                turn_checkpoints.push(session.messages.clone());
                tools::undo::push_turn_marker();
                session.add_user_message(&line);
                turn_count += 1;
                cancel_flag.store(false, Ordering::SeqCst);
                // 进度提示：回合开始 + 上轮耗时（预估等待时间）+ 工具步骤计数
                let turn_started = std::time::Instant::now();
                let mut step_count: usize = 0;
                let mut last_progress = std::time::Instant::now();
                // 对话界面：回合框线 + 回显用户输入 + 助手标记
                let last_hint = turn_durations
                    .last()
                    .map(|d| format!(" · 上轮 {d:.1}s"))
                    .unwrap_or_default();
                let draft_hint = if draft_mode { " · draft" } else { "" };
                let style_hint = if output_style == "default" {
                    ""
                } else {
                    &format!(" · {output_style}")
                };
                // Plan 模式：/plan 开关或权限模式 plan（两入口等效）
                let plan_active = plan_mode
                    || crate::permission::current().mode == crate::permission::PermissionMode::Plan;
                let plan_hint = if plan_active { " · plan" } else { "" };
                println!(
                    "\x1b[90m╭─ 回合 {turn_count} · {model}{last_hint}{draft_hint}{style_hint}{plan_hint}\x1b[0m"
                );
                println!(
                    "
\x1b[1;33m❯ 你\x1b[0m
{line}
"
                );
                println!("\x1b[1;34m❯ Stitch\x1b[0m");

                let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
                let max_iterations = cfg.max_iterations;
                let api_base = cfg.llm_api_base.clone();
                let handle = tokio::spawn({
                    let tools = tools.clone();
                    let confirm_pending = confirm_pending.clone();
                    let allow_rules = allow_rules.clone();
                    let cancel_flag = cancel_flag.clone();
                    let api_key = api_key.clone();
                    let model = model.clone();
                    let work_dir = work_dir.clone();
                    async move {
                        let result = if plan_active {
                            // Plan 模式：规划 → 批准 → 逐步执行（run_plan_turn
                            // 内部复用 run_react_core；确认走同一 oneshot 通道）
                            agent::run_plan_turn_streaming(
                                &mut session,
                                &api_base,
                                &model,
                                &api_key,
                                &tools,
                                max_iterations,
                                confirm_pending,
                                Some(&work_dir),
                                allow_rules,
                                &event_tx,
                                &cancel_flag,
                                None,
                            )
                            .await
                        } else {
                            agent::run_react_streaming(
                                &mut session,
                                &api_base,
                                &model,
                                &api_key,
                                &tools,
                                max_iterations,
                                confirm_pending,
                                Some(&work_dir),
                                allow_rules,
                                &event_tx,
                                &cancel_flag,
                                None,
                            )
                            .await
                        };
                        (result, session)
                    }
                });

                // 渲染事件流直到回合结束
                let mut turn_cost: Option<f64> = None;
                // 首字节前的「正在生成」提示：每秒原地刷新，首个内容事件到达时清行
                let mut generating = true;
                let mut last_tick = std::time::Instant::now();
                // 思考过程行首状态（┆ 前缀只在行首加）
                let mut thinking_line_start = true;
                loop {
                    tokio::select! {
                        event = event_rx.recv() => {
                            let Some(ev) = event else { break };
                            // 任何内容事件都替代「正在生成」提示
                            if generating {
                                generating = false;
                                print!("\r\x1b[K");
                                let _ = std::io::stdout().flush();
                            }
                            match ev {
                                AgentEvent::Token { text } => {
                                    crate::render::render_token(&text);
                                }
                                AgentEvent::Thinking { text } => {
                                    // 思考过程灰色 ┆ 前缀（/think on），不入会话消息；
                                    // 控制字符剥离防注入
                                    let mut out = std::io::stdout();
                                    for (i, seg) in text.split('\n').enumerate() {
                                        if i > 0 {
                                            let _ = writeln!(out);
                                        }
                                        if thinking_line_start && !seg.is_empty() {
                                            let _ = write!(out, "\x1b[90m┆\x1b[0m ");
                                        }
                                        let _ = write!(out, "{}", markdown_strip_control(seg));
                                        thinking_line_start = true;
                                    }
                                    if !text.ends_with('\n') {
                                        thinking_line_start = false;
                                    }
                                    let _ = out.flush();
                                }
                                AgentEvent::ConfirmRequest { id, tool, message } => {
                                    let allow = crate::render::dialog::confirm(&format!("{tool}: {message}"));
                                    if let Some(tx) = confirm_pending.lock().ok().and_then(|mut m| m.remove(&id)) {
                                        let _ = tx.send(allow);
                                    }
                                }
                                AgentEvent::PlanProposed { id, plan } => {
                                    // 计划渲染 + 终端确认；结果经同一 oneshot 通道回传
                                    // （run_plan_turn 的 confirm_plan 在另一端等待）
                                    println!("\n\x1b[1;36m计划（批准后逐步执行）\x1b[0m\n{}", plan.format());
                                    let allow = crate::render::dialog::confirm("批准计划并开始执行？");
                                    if let Some(tx) = confirm_pending.lock().ok().and_then(|mut m| m.remove(&id)) {
                                        let _ = tx.send(allow);
                                    }
                                }
                                AgentEvent::PlanApproved => {
                                    println!("\x1b[1;36m计划已批准，开始逐步执行\x1b[0m");
                                }
                                AgentEvent::PlanRejected => {
                                    println!("\x1b[90m计划未获批准，未执行任何改动\x1b[0m");
                                }
                                AgentEvent::PlanStepStart { index, total, description } => {
                                    println!("\n\x1b[1;33m步骤 {}/{}：{}\x1b[0m", index + 1, total, description);
                                }
                                AgentEvent::ToolStart { name, .. } => {
                                    step_count += 1;
                                    // 工具名青色加粗；Done 只补 ✓/✗ 与摘要，名字不重复
                                    print!("\n\x1b[90m·\x1b[0m \x1b[1;36m{name}\x1b[0m ");
                                    let _ = std::io::stdout().flush();
                                }
                                AgentEvent::ToolOutput { text, .. } => {
                                    // 工具直播输出（ADR-037）：dim 灰色透传，剥控制字符
                                    print!("\x1b[2m{}\x1b[0m", markdown_strip_control(&text));
                                    let _ = std::io::stdout().flush();
                                }
                                AgentEvent::ToolDone { success, summary, .. } => {
                                    let mark = if success {
                                        "\x1b[1;32m✓\x1b[0m"
                                    } else {
                                        "\x1b[1;31m✗\x1b[0m"
                                    };
                                    // 摘要灰色 + 140 字符截断（防长输出糊屏；chars 保 UTF-8 边界）
                                    let s: String =
                                        markdown_strip_control(&summary).chars().take(140).collect();
                                    println!("{mark} \x1b[90m{s}\x1b[0m");
                                }
                                AgentEvent::Done { cost, .. } => {
                                    turn_cost = Some(cost);
                                    crate::render::finish_stream();
                                    println!();
                                }
                                AgentEvent::Error { message } => {
                                    crate::render::finish_stream();
                                    println!("\n\x1b[1;31m✗ 错误：\x1b[0m{}", markdown_strip_control(&message));
                                }
                                _ => {}
                            }
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                            if cancel_flag.load(Ordering::SeqCst) {
                                // 回合被 Ctrl+C 中断：等待 agent 收尾（run_react 会尽快返回）
                            } else if generating {
                                if last_tick.elapsed() >= std::time::Duration::from_secs(1) {
                                    last_tick = std::time::Instant::now();
                                    let secs = turn_started.elapsed().as_secs();
                                    print!("\r\x1b[K\x1b[90m正在生成 · {secs}s（Ctrl+C 中断）\x1b[0m");
                                    let _ = std::io::stdout().flush();
                                }
                            } else if last_progress.elapsed() >= std::time::Duration::from_secs(10) {
                                // 长回合进度心跳：已用时 + 工具步数（用户可据节奏预估剩余）
                                last_progress = std::time::Instant::now();
                                let secs = turn_started.elapsed().as_secs();
                                println!(
                                    "\x1b[90m… 仍在处理 · 已 {secs}s · 第 {step_count} 步工具（Ctrl+C 中断）\x1b[0m"
                                );
                            }
                        }
                    }
                    if event_rx.is_empty() && handle.is_finished() {
                        break;
                    }
                }
                // 回合结束：清掉可能残留的「正在生成」提示
                if generating {
                    print!("\r\x1b[K");
                    let _ = std::io::stdout().flush();
                }
                crate::render::finish_stream();
                // 中断标注：Ctrl+C 置位过 → 回合框线补「已中断」
                let interrupted = cancel_flag.load(Ordering::SeqCst);
                cancel_flag.store(false, Ordering::SeqCst);

                // 回合结束拿回 session（spawn 内 move 出去的）
                let (turn_result, returned_session) = handle
                    .await
                    .map_err(|e| anyhow::anyhow!("回合执行失败：{e}"))?;
                session = returned_session;
                if let Err(e) = turn_result {
                    eprintln!("[回合错误] {e}");
                    println!();
                    continue;
                }

                // 落盘会话
                let _ = agent::persist::save_session(&session_dir, &session, &mut manifest);

                // Hooks：Stop（回合结束通知）
                {
                    let hooks = crate::hooks::HookRegistry::load(Some(&work_dir));
                    if hooks.has(crate::hooks::HookEvent::Stop) {
                        let transcript = session_dir.join("messages.jsonl");
                        let _ = hooks
                            .run(
                                crate::hooks::HookEvent::Stop,
                                &session_id,
                                &serde_json::json!({
                                    "cwd": work_dir,
                                    "transcript_path": transcript.display().to_string(),
                                }),
                                None,
                            )
                            .await;
                    }
                }

                let elapsed = turn_started.elapsed().as_secs_f64();
                turn_durations.push(elapsed);
                let interrupted_hint = if interrupted { " · 已中断" } else { "" };
                if let Some(cost) = turn_cost {
                    total_cost += cost;
                    println!(
                        "
\x1b[90m╰─ 回合 {turn_count} · {elapsed:.1}s · 成本 ¥{cost:.4}{interrupted_hint}\x1b[0m"
                    );
                } else {
                    println!(
                        "
\x1b[90m╰─ 回合 {turn_count} · {elapsed:.1}s{interrupted_hint}\x1b[0m"
                    );
                }
                // --max-turns 最大回合数：达到上限自动停（防自动模式跑飞）
                if let Some(limit) = max_turns
                    && turn_count >= limit
                {
                    println!("\n[回合] 已达到 --max-turns {limit} 上限，自动停止");
                    break;
                }
                // --budget 成本预算：累计达到后提示停止（非 TTY 直接停，防脚本跑飞）
                if let Some(limit) = budget
                    && total_cost >= limit
                {
                    println!("\n[预算] 累计成本 ¥{total_cost:.4} 已达到预算 ¥{limit:.4}");
                    if crate::render::dialog::confirm("已达到成本预算，继续会话？") {
                        println!("[预算] 继续（累计成本将超出预算）");
                    } else {
                        println!("[预算] 已停止（用 --budget 加大限额后重开）");
                        break;
                    }
                }
                // 草稿模式：回滚本回合全部文件改动（只预览不落盘）
                if draft_mode {
                    match tools::undo::undo_until_marker() {
                        Ok(descs) if !descs.is_empty() => {
                            println!("  [draft] 草稿预览（已回滚，未落盘）：");
                            for d in &descs {
                                println!("    · {d}");
                            }
                            println!(
                                "  [draft] 确认后：/draft 关闭草稿模式，重新发送请求即真正写入"
                            );
                        }
                        Ok(_) => println!("  [draft] 本回合无文件改动"),
                        Err(e) => println!("  [draft] 回滚失败：{e}"),
                    }
                }
                // 任务清单进度（模型用过 TodoWrite 时显示）
                if let Ok(store) = todo_store.lock()
                    && !store.is_empty()
                {
                    println!(
                        "  [todo] 任务进度 {}/{} 完成（/todo 查看）",
                        store.done_count(),
                        store.list().len()
                    );
                }
                // statusLine：--setting > settings.json > config（失败静默）——
                // 右对齐进输入框底边（Claude Code 式，替代独立 ▌ 行）
                let statusline = match crate::statusline::resolved(
                    settings.statusline.as_deref(),
                    cfg.statusline.as_deref(),
                ) {
                    Some(cmd) => crate::statusline::run(&cmd).await,
                    None => None,
                };
                println!("{}", input_box_bottom(statusline.as_deref(), term_width()));
            }

            Err(rustyline::error::ReadlineError::Interrupted) => {
                // 回合中 Ctrl+C：ctrlc handler 已置 cancel；空闲时退出
                if cancel_flag.load(Ordering::SeqCst) {
                    println!("\n[已中断当前回合]");
                    continue;
                }
                println!("\n[退出]（Ctrl+D 或 /exit）");
                break;
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                println!("\n[退出]");
                break;
            }
            Err(e) => anyhow::bail!("输入错误：{e}"),
        }
    }

    // Hooks：SessionEnd（退出收尾；非零退出码仅记录）
    {
        let hooks = crate::hooks::HookRegistry::load(Some(&work_dir));
        if hooks.has(crate::hooks::HookEvent::SessionEnd) {
            let transcript = session_dir.join("messages.jsonl");
            let _ = hooks
                .run(
                    crate::hooks::HookEvent::SessionEnd,
                    &session_id,
                    &serde_json::json!({
                        "cwd": work_dir,
                        "transcript_path": transcript.display().to_string(),
                    }),
                    None,
                )
                .await;
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

fn new_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 三层记忆文件路径：local → 工作区 CLAUDE.local.md（本地私有）·
/// project → 工作区 CLAUDE.md（团队共享）· global → config_dir/CLAUDE.md（用户级）。
fn memory_layer_path(work_dir: &str, which: &str) -> Option<PathBuf> {
    match which {
        "local" => Some(PathBuf::from(work_dir).join("CLAUDE.local.md")),
        "project" => Some(PathBuf::from(work_dir).join("CLAUDE.md")),
        "global" => Some(crate::config::config_dir().join("CLAUDE.md")),
        _ => None,
    }
}

/// 记忆文件内容预览：前 3 个非空行拼接，超 60 字符截断。
fn memory_preview(content: &str) -> String {
    let joined: String = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ");
    if joined.chars().count() > 60 {
        format!("{}…", joined.chars().take(60).collect::<String>())
    } else {
        joined
    }
}

/// 列出三层记忆文件状态与内容预览（/memory 无参）。
fn print_memory_layers(work_dir: &str) {
    println!("[memory] 记忆三层（优先级 local > project > global，冲突后加载者覆盖）：");
    for (label, which) in [
        ("全局记忆", "global"),
        ("项目记忆", "project"),
        ("本地记忆", "local"),
    ] {
        let Some(path) = memory_layer_path(work_dir, which) else {
            continue;
        };
        let status = match std::fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => {
                format!("{} 字节 · {}", content.len(), memory_preview(&content))
            }
            Ok(_) => "存在但为空".to_string(),
            Err(_) => "未创建".to_string(),
        };
        println!("  · {label}（{which}）");
        println!("      {}", path.display());
        println!("      {status}");
    }
    println!("  操作：/memory open|create|delete <local|project|global>");
}

/// 用系统编辑器打开文件（$EDITOR 优先；Windows 缺省 notepad，其他缺省 vi）。
fn open_editor(path: &Path) -> anyhow::Result<()> {
    let default = if cfg!(target_os = "windows") {
        "notepad"
    } else {
        "vi"
    };
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| default.to_string());
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or(default).to_string();
    let mut cmd = std::process::Command::new(&program);
    cmd.args(parts).arg(path);
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("编辑器 {program} 退出码 {}", status)
    }
}

/// 全部会话 manifest 的 estimated_tokens 总和（累计成本近似）。
fn all_manifest_tokens(work_dir: &str) -> usize {
    let root = PathBuf::from(work_dir).join(".stitch").join("sessions");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return 0;
    };
    entries.flatten().fold(0usize, |acc, entry| {
        let Ok(text) = std::fs::read_to_string(entry.path().join("manifest.json")) else {
            return acc;
        };
        let Ok(m) = serde_json::from_str::<agent::persist::Manifest>(&text) else {
            return acc;
        };
        acc + m.estimated_tokens
    })
}

/// 回合成本/缓存统计（会话当前状态 + 全会话累计）。
fn print_session_cost(session: &Session, model: &str, work_dir: &str) {
    let usage = agent::tokens::TokenUsage {
        input_tokens: agent::tokens::estimate_messages(&session.messages),
        output_tokens: session
            .messages
            .iter()
            .filter(|m| m.role == session::Role::Assistant)
            .map(|m| agent::tokens::estimate_text(m.content.text()))
            .sum(),
        cache_hit_tokens: 0,
        cache_miss_tokens: 0,
    };
    println!(
        "  会话估算：输入 {} tokens · 输出 {} tokens · 成本约 ¥{:.4}（服务端缓存统计见回合输出）",
        usage.input_tokens,
        usage.output_tokens,
        agent::tokens::estimate_cost(&usage, model),
    );
    // 全会话累计（Claude Code /cost 语义：工作区内所有会话的估算成本）
    let total = all_manifest_tokens(work_dir);
    if total > 0 {
        let cumulative = agent::tokens::TokenUsage {
            input_tokens: total,
            output_tokens: 0,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
        };
        println!(
            "  全部会话累计（估算）：{total} tokens · 成本约 ¥{:.4}",
            agent::tokens::estimate_cost(&cumulative, model),
        );
    }
}

/// 常用模型快速选择（/model 无参数时；Claude Code 语义）。
const MODEL_PICKER: &[&str] = &[
    "deepseek-v4-flash",
    "deepseek-v4-pro",
    "gpt-4o",
    "glm-4-flash",
    "kimi-k2.5",
    "MiniMax-M2.5",
    "qwen3-vl:8b (Ollama 本地)",
];

/// /model 选择器：返回用户选中的模型名（Esc 取消 → 保持当前）。
fn pick_model(current: &str) -> anyhow::Result<String> {
    let mut options: Vec<String> = MODEL_PICKER.iter().map(|s| s.to_string()).collect();
    if !options.iter().any(|o| o == current) {
        options.insert(0, current.to_string());
    }
    let idx = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("选择模型（Esc 取消）")
        .items(&options)
        .default(0)
        .interact_opt()?;
    match idx {
        Some(i) => Ok(options[i].clone()),
        None => Ok(current.to_string()),
    }
}

/// 供 run_command 分发的命令实现（占位，main.rs 调用）。
#[allow(dead_code)]
pub fn cmd_stub() {}

/// 构建交互提示符：`(raw, styled)` 双通道。
///
/// raw 必须是纯文本（rustyline 在 Windows 上无法解析 ANSI 转义，用它计算
/// 光标宽度）；styled 带颜色用于实际渲染。二者可见宽度必须一致。
/// 剥离终端控制字符（转义注入防护）——复用 markdown 渲染器同一实现，
/// 工具输出/思考/错误等外部文本进终端前统一过滤。
fn markdown_strip_control(s: &str) -> std::borrow::Cow<'_, str> {
    crate::render::markdown::strip_control(s)
}

/// 输入提示符：左框线 │ + ❯ + 目录短名（模型/权限/上下文已上移到输入框顶边）。
/// raw 无 ANSI（rustyline 用 raw 算光标宽度，Windows 无法解析转义），
/// styled 上屏渲染，剥色后必须与 raw 一致。
fn build_prompt(dir_short: &str) -> (String, String) {
    let raw = format!("│ ❯ {dir_short} ");
    let styled = format!("\x1b[90m│\x1b[0m \x1b[1;36m❯\x1b[0m \x1b[90m{dir_short}\x1b[0m ");
    (raw, styled)
}

/// 终端当前宽度（列）；非 TTY 或查询失败回退 80。
fn term_width() -> u16 {
    if !std::io::stdout().is_terminal() {
        return 80;
    }
    crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80)
}

/// 剥除 ANSI 转义序列（CSI / OSC / 两字符序列），返回纯文本。宽度计算用。
fn strip_ansi_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.next() {
                Some('[') => {
                    // CSI：跳到终止符（0x40–0x7E）
                    for c2 in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c2) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC：到 BEL 或 ESC \ 结束
                    for c2 in chars.by_ref() {
                        if c2 == '\x07' || (c2 == '\x1b') {
                            if c2 == '\x1b' {
                                chars.next(); // 吞掉 \
                            }
                            break;
                        }
                    }
                }
                Some(_) => {} // 两字符序列（ESC X）：跳过
                None => break,
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// 显示宽度：剥 ANSI 后按 unicode-width 计（CJK 双宽，statusline/分支右对齐用）。
fn display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    UnicodeWidthStr::width(strip_ansi_codes(s).as_str())
}

/// 输入框顶边（Claude Code 式信息行）：`╭─ ✳ 模型 · 权限 · 上下文% ── 分支 ─╮`
///
/// - 边框灰调（90）；✳ 与模型青色加粗；上下文百分比按占用着色：
///   <60% 默认 · 60–80% 黄 · ≥80% 红（压缩预警）
/// - 分支右对齐贴右角；终端过窄时先丢分支，仍不够则行钳到信息宽度
///   （病态窄终端不截断信息，宁可行略超宽由终端折行）。
fn input_box_top(
    model: &str,
    mode: &str,
    ctx_pct: usize,
    branch: Option<&str>,
    width: u16,
) -> String {
    let pct = if ctx_pct >= 80 {
        format!("\x1b[1;31m{ctx_pct}%\x1b[90m")
    } else if ctx_pct >= 60 {
        format!("\x1b[1;33m{ctx_pct}%\x1b[90m")
    } else {
        format!("{ctx_pct}%")
    };
    let info = format!("\x1b[1;36m✳ {model}\x1b[90m · {mode} · {pct}");
    let info_w = display_width(&info);
    let w = usize::from(width).max(info_w + 5);
    // 分支名来自外部 git 仓库（不可信输入），进终端前剥离 C0 控制字符
    // ——0.5.4 渲染层转义注入防护同款（git check-ref-format 已拒控制
    // 字符，此处为纵深防御）
    let mut right = branch.map(|b| {
        let clean = crate::render::markdown::strip_control(b);
        format!(" {clean} ")
    });
    loop {
        let right_w = right.as_deref().map_or(0, display_width);
        let fill = w.saturating_sub(5 + info_w + right_w);
        if fill >= 1 {
            let fill_s = "─".repeat(fill);
            let right_s = right.unwrap_or_default();
            return format!("\x1b[90m╭─ {info}{fill_s}{right_s}─╮\x1b[0m");
        }
        if right.is_some() {
            right = None;
            continue;
        }
        // 病态窄终端：恰好信息宽度，无填充
        return format!("\x1b[90m╭─ {info}─╮\x1b[0m");
    }
}

/// 输入框底边：`╰─ ─────── 状态行 ─╯`，状态行右对齐（保留其 ANSI 颜色，
/// 宽度按剥色后计算）；无状态行时纯边框。状态行过长按显示宽度截断加省略号。
fn input_box_bottom(status: Option<&str>, width: u16) -> String {
    let w = usize::from(width).max(12);
    match status {
        Some(s) if !s.trim().is_empty() => {
            let s = s.trim_end();
            let sw = display_width(s);
            if sw + 6 > w {
                let plain = strip_ansi_codes(s);
                let (cut, truncated) = truncate_disp(&plain, w.saturating_sub(8).max(1));
                let s = if truncated { format!("{cut}…") } else { cut };
                let sw = display_width(&s);
                let fill = w.saturating_sub(6 + sw).max(1);
                format!("\x1b[90m╰─{fill} {s} ─╯\x1b[0m", fill = "─".repeat(fill))
            } else {
                let fill = w.saturating_sub(6 + sw).max(1);
                format!("\x1b[90m╰─{fill} {s} ─╯\x1b[0m", fill = "─".repeat(fill))
            }
        }
        _ => format!("\x1b[90m╰─{}─╯\x1b[0m", "─".repeat(w.saturating_sub(4))),
    }
}

/// 按显示宽度截断（保 UTF-8 边界），返回（截断文本，是否被截）。
fn truncate_disp(s: &str, max: usize) -> (String, bool) {
    use unicode_width::UnicodeWidthChar;
    let mut w = 0usize;
    let mut cut = String::new();
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > max {
            return (cut, true);
        }
        cut.push(c);
        w += cw;
    }
    (cut, false)
}

/// 当前 git 分支短名（非 git 仓库 / detached / 失败 → None）。
/// 每次提示符前调用（~ms 级进程开销，换取 Claude Code 式实时分支显示）。
/// 用 symbolic-ref 而非 rev-parse --abbrev-ref HEAD：后者在无提交的
/// 空仓库（unborn branch）里 fatal 退出，前者两者皆可、detached 时失败。
fn git_branch_short(work_dir: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(work_dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyline::completion::Completer as _;

    /// 剥离 ANSI CSI 转义序列（\x1b[...m），测试用。
    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_esc = false;
        for c in s.chars() {
            if in_esc {
                if c == 'm' {
                    in_esc = false;
                }
            } else if c == '\x1b' {
                in_esc = true;
            } else {
                out.push(c);
            }
        }
        out
    }

    /// 提示符回归锁：raw 必须无 ANSI（Windows rustyline 宽度计算会把它
    /// 计入光标位置，导致「光标离得很远」），styled 剥掉 ANSI 后必须与
    /// raw 完全一致（显示宽度一致）。
    #[test]
    fn prompt_raw_plain_and_styled_matches() {
        let (raw, styled) = build_prompt("promptstdio");
        assert!(!raw.contains('\x1b'), "raw 提示符不得含 ANSI 转义");
        assert_eq!(strip_ansi(&styled), raw);
        // 期望形态：│ ❯ <目录>（尾部空格）
        assert_eq!(raw, "│ ❯ promptstdio ");
    }

    /// 输入框顶部信息行：总显示宽度 == 给定宽度；branch 右对齐。
    #[test]
    fn input_box_top_widths_and_branch() {
        // 60 列：info 33 列 + 右边框 3 + branch「 main 」6 + 边框 3 = 60
        let line = input_box_top("deepseek-v4-flash", "default", 12, Some("main"), 60);
        assert_eq!(display_width(&line), 60, "整行宽度必须等于终端宽");
        assert!(strip_ansi(&line).starts_with("╭─"), "以圆角左上角开头");
        assert!(
            strip_ansi(&line).ends_with(" main ─╮"),
            "branch 右对齐于右边框内"
        );
        assert!(strip_ansi(&line).contains("✳ deepseek-v4-flash · default · 12%"));

        // 窄宽度（20 < info 宽 35+5）：丢弃 branch，行钳到信息宽度
        let narrow = input_box_top("deepseek-v4-flash", "default", 12, Some("main"), 20);
        assert!(
            !strip_ansi(&narrow).contains("main"),
            "窄宽度不得携带 branch"
        );
        assert_eq!(display_width(&narrow), 40, "行钳到 info 宽 + 5");

        // 病态窄宽度（5 < info 宽）→ 同上，宁可行略超宽由终端折行
        let tiny = input_box_top("deepseek-v4-flash", "default", 12, Some("main"), 5);
        assert_eq!(display_width(&tiny), 40);

        // 恶意分支名（注入 ANSI/OSC/BEL）进终端前被剥离
        let evil = input_box_top("m", "default", 10, Some("\x1b[31mred\x1b]0;x\x07evil"), 80);
        // 行内样式 ESC（\x1b[90m/\x1b[1;36m 等）是合法的——只断言注入序列不存活
        assert!(
            !evil.contains("\x1b[31m") && !evil.contains('\x07'),
            "注入的控制序列必须剥离"
        );
        assert!(
            strip_ansi(&evil).contains("[31mred]0;xevil"),
            "可打印字符保留（CSI 载荷文本不算控制字符）"
        );
    }

    /// ctx 占用阈值着色：<60 无色、≥60 黄、≥80 红。
    #[test]
    fn input_box_top_ctx_threshold_colors() {
        let low = input_box_top("m", "default", 40, None, 80);
        assert!(!low.contains("\x1b[1;33m") && !low.contains("\x1b[1;31m"));
        let mid = input_box_top("m", "default", 70, None, 80);
        assert!(mid.contains("\x1b[1;33m"), "≥60% 应标黄");
        let high = input_box_top("m", "default", 90, None, 80);
        assert!(high.contains("\x1b[1;31m"), "≥80% 应标红");
    }

    /// 输入框底部：status 右对齐；无 status 时纯底边；长 status 截断 + …。
    #[test]
    fn input_box_bottom_status() {
        let with_s = input_box_bottom(Some("12:03"), 60);
        assert_eq!(display_width(&with_s), 60);
        assert!(
            strip_ansi(&with_s).ends_with("12:03 ─╯"),
            "status 右对齐于底边"
        );

        let none = input_box_bottom(None, 30);
        assert_eq!(display_width(&none), 30);
        assert!(
            strip_ansi(&none).ends_with("─╯"),
            "无 status 时仍是完整底边"
        );

        let long = input_box_bottom(Some(&"x".repeat(60)), 30);
        assert_eq!(display_width(&long), 30, "长 status 必须截断到框内");
        assert!(strip_ansi(&long).contains('…'), "截断处应有省略号");
        assert!(strip_ansi(&long).ends_with("─╯"));
    }

    /// display_width：ANSI 剥除后按 unicode-width 计宽（CJK 双宽）。
    #[test]
    fn display_width_counts_visible_chars() {
        assert_eq!(display_width("12:03"), 5);
        assert_eq!(display_width("\x1b[31m12:03\x1b[0m"), 5);
        assert_eq!(display_width("中文"), 4);
    }

    /// 输入高亮：slash 命令青、参数灰；! 黄；@ 青；注入的 ESC 被剥离。
    #[test]
    fn highlight_styles_lines() {
        use rustyline::highlight::Highlighter as _;
        let c = StitchCompleter::new("");
        let styled = c.highlight("/model deepseek-v4-flash", 0);
        assert!(
            styled.contains("\x1b[1;36m/model\x1b[0m"),
            "命令名应青色加粗"
        );
        assert!(
            styled.contains("\x1b[90m deepseek-v4-flash\x1b[0m"),
            "参数应灰色"
        );
        assert_eq!(strip_ansi(&styled), "/model deepseek-v4-flash");

        let shell = c.highlight("!ls -la", 0);
        assert!(shell.contains("\x1b[1;33m"), "! 指令应黄色加粗");

        let mention = c.highlight("@README.md", 0);
        assert!(mention.contains("\x1b[1;36m"), "@ 文件引用应青色加粗");

        // 纯文本保持借用，不产生新分配
        let plain = c.highlight("hello", 0);
        assert!(matches!(plain, std::borrow::Cow::Borrowed(_)));

        // C0 控制字符注入被剥离（0.5.4 防护，复用于高亮）
        let injected = c.highlight("/m\x1b[31mred", 0);
        assert!(!injected.contains("\x1b[31m"), "注入的 CSI 必须被剥离");
    }

    /// fish 式灰提示：/ 前缀给出命令续尾；普通输入走历史建议。
    #[test]
    fn hinter_suggests_slash_and_history() {
        let c = StitchCompleter::new("");
        let ctx = repl_test_ctx();
        use rustyline::hint::Hinter as _;
        assert_eq!(c.hint("/mod", 4, &ctx).as_deref(), Some("el"));
        assert_eq!(c.hint("/model", 6, &ctx), None, "已输全命令无提示");
        assert_eq!(c.hint("/xyz", 4, &ctx), None, "未知命令无提示");
        assert_eq!(
            c.hint("/", 1, &ctx).as_deref(),
            Some("help"),
            "裸 / 提示首个内置"
        );

        // 历史建议：hello → " world"
        use rustyline::history::History as _;
        let mut hist = rustyline::history::DefaultHistory::default();
        hist.add("hello world").unwrap();
        let hctx = rustyline::Context::new(&hist);
        assert_eq!(c.hint("hello", 5, &hctx).as_deref(), Some(" world"));
    }

    /// git 分支探测：有效分支名返回 Some；非仓库 / detached 返回 None。
    #[test]
    fn git_branch_short_probe() {
        let dir = tempfile::tempdir().unwrap();
        let ok = std::process::Command::new("git")
            .args(["init", "-b", "my-branch"])
            .current_dir(dir.path())
            .output()
            .expect("git 必须可用");
        assert!(ok.status.success(), "git init 失败: {:?}", ok);
        assert_eq!(
            git_branch_short(dir.path().to_str().unwrap()),
            Some("my-branch".to_string())
        );

        // CARGO_MANIFEST_DIR 在仓库内，不能当非仓库用例——用新临时目录
        let non_repo = tempfile::tempdir().unwrap();
        assert_eq!(
            git_branch_short(non_repo.path().to_str().unwrap()),
            None,
            "非 git 仓库返回 None"
        );
    }

    /// 补全测试用的 Context（rustyline 15 的 Context::new 需要 history 参数）。
    fn repl_test_ctx() -> rustyline::Context<'static> {
        use std::sync::OnceLock;
        static HIST: OnceLock<rustyline::history::DefaultHistory> = OnceLock::new();
        rustyline::Context::new(HIST.get_or_init(rustyline::history::DefaultHistory::default))
    }

    #[test]
    fn slash_parsing() {
        assert!(matches!(parse_slash("/exit"), SlashAction::Exit));
        assert!(matches!(parse_slash("/quit"), SlashAction::Exit));
        assert!(matches!(parse_slash("/help"), SlashAction::Help));
        assert!(matches!(parse_slash("/?"), SlashAction::Help));
        assert!(matches!(parse_slash("/clear"), SlashAction::Clear));
        match parse_slash("/model deepseek-v4-flash") {
            SlashAction::Model(m) => assert_eq!(m, "deepseek-v4-flash"),
            other => panic!("expected Model, got {other:?}"),
        }
        match parse_slash("/model") {
            SlashAction::Model(m) => assert!(m.is_empty()),
            other => panic!("expected Model(empty), got {other:?}"),
        }
        // 未知命令 → 按自定义命令解析（处理时查 commands 表，查不到再报错）
        match parse_slash("/bogus") {
            SlashAction::Custom(name, args) => {
                assert_eq!(name, "bogus");
                assert!(args.is_empty());
            }
            other => panic!("expected Custom, got {other:?}"),
        }
        match parse_slash("/bogus2 带参数") {
            SlashAction::Custom(name, args) => {
                assert_eq!(name, "bogus2");
                assert_eq!(args, "带参数");
            }
            other => panic!("expected Custom with args, got {other:?}"),
        }
        // 内置：/review /fix 是原生 slash（不再落入自定义命令）
        assert!(matches!(parse_slash("/review"), SlashAction::Review));
        assert!(matches!(parse_slash("/fix"), SlashAction::Fix));
        // 新命令
        assert!(matches!(parse_slash("/context"), SlashAction::Context));
        assert!(matches!(parse_slash("/usage"), SlashAction::Context));
        assert!(matches!(parse_slash("/compact"), SlashAction::Compact));
        assert!(matches!(parse_slash("/rewind"), SlashAction::Rewind));
        assert!(matches!(parse_slash("/undo"), SlashAction::Rewind));
        assert!(matches!(parse_slash("/agents"), SlashAction::Agents));
        assert!(matches!(parse_slash("/mcp"), SlashAction::Mcp(_)));
        assert!(matches!(
            parse_slash("/mcp add demo --url http://localhost:8080"),
            SlashAction::Mcp(rest) if rest == "add demo --url http://localhost:8080"
        ));
        assert!(matches!(parse_slash("/hooks"), SlashAction::Hooks));
        assert!(matches!(parse_slash("/config"), SlashAction::Config(_)));
        // /config key=value 带内改设置
        match parse_slash("/config max_iterations=20") {
            SlashAction::Config(a) => assert_eq!(a, "max_iterations=20"),
            _ => panic!("expected Config(arg)"),
        }
        match parse_slash("/config model=deepseek-v4-flash") {
            SlashAction::Config(a) => assert_eq!(a, "model=deepseek-v4-flash"),
            _ => panic!("expected Config(arg)"),
        }
        match parse_slash("/export out.md") {
            SlashAction::Export(Some(p)) => assert_eq!(p, "out.md"),
            other => panic!("expected Export(Some), got {other:?}"),
        }
        assert!(matches!(parse_slash("/export"), SlashAction::Export(None)));
        match parse_slash("/permissions add read_file path C:/ref") {
            SlashAction::Permissions(args) => {
                assert_eq!(args, ["add", "read_file", "path", "C:/ref"]);
            }
            other => panic!("expected Permissions, got {other:?}"),
        }
        match parse_slash("/allowed-tools") {
            SlashAction::Permissions(args) => assert!(args.is_empty()),
            other => panic!("expected Permissions(empty), got {other:?}"),
        }
        // 第六轮：/memory /inspect 原生 slash
        assert!(matches!(parse_slash("/memory"), SlashAction::Memory(_)));
        match parse_slash("/memory open local") {
            SlashAction::Memory(args) => assert_eq!(args, ["open", "local"]),
            other => panic!("expected Memory(args), got {other:?}"),
        }
        assert!(matches!(parse_slash("/inspect"), SlashAction::Inspect));
        // 第七轮：/retry /draft /todo 原生 slash
        assert!(matches!(parse_slash("/retry"), SlashAction::Retry));
        match parse_slash("/draft on") {
            SlashAction::Draft(args) => assert_eq!(args, ["on"]),
            other => panic!("expected Draft(on), got {other:?}"),
        }
        match parse_slash("/draft") {
            SlashAction::Draft(args) => assert!(args.is_empty()),
            other => panic!("expected Draft(empty), got {other:?}"),
        }
        match parse_slash("/todo clear") {
            SlashAction::Todo(args) => assert_eq!(args, ["clear"]),
            other => panic!("expected Todo(clear), got {other:?}"),
        }
        assert!(matches!(parse_slash("/todo"), SlashAction::Todo(_)));
        // 第十轮：/output-style /statusline /search 原生 slash
        match parse_slash("/output-style concise") {
            SlashAction::OutputStyle(args) => assert_eq!(args, ["concise"]),
            other => panic!("expected OutputStyle(concise), got {other:?}"),
        }
        assert!(matches!(
            parse_slash("/output-style"),
            SlashAction::OutputStyle(_)
        ));
        match parse_slash("/statusline set echo hi") {
            SlashAction::Statusline(args) => assert_eq!(args, ["set", "echo", "hi"]),
            other => panic!("expected Statusline(set echo hi), got {other:?}"),
        }
        assert!(matches!(
            parse_slash("/statusline clear"),
            SlashAction::Statusline(args) if args == ["clear"]
        ));
        match parse_slash("/search 权限模式") {
            SlashAction::Search(kw) => assert_eq!(kw, "权限模式"),
            other => panic!("expected Search(权限模式), got {other:?}"),
        }
        assert!(matches!(parse_slash("/search"), SlashAction::Search(kw) if kw.is_empty()));
        match parse_slash("/think on") {
            SlashAction::Think(args) => assert_eq!(args, ["on"]),
            other => panic!("expected Think(on), got {other:?}"),
        }
        assert!(matches!(parse_slash("/think"), SlashAction::Think(args) if args.is_empty()));
        assert!(matches!(parse_slash("/init"), SlashAction::Init));
        // 第十四轮：/skill 原生 slash
        assert!(matches!(parse_slash("/skill"), SlashAction::Skill(args) if args.is_empty()));
        match parse_slash("/skill excel-report") {
            SlashAction::Skill(args) => assert_eq!(args, ["excel-report"]),
            other => panic!("expected Skill(args), got {other:?}"),
        }
        match parse_slash("/skill off") {
            SlashAction::Skill(args) => assert_eq!(args, ["off"]),
            other => panic!("expected Skill(off), got {other:?}"),
        }
        assert!(matches!(parse_slash("/plan"), SlashAction::Plan(args) if args.is_empty()));
        match parse_slash("/plan on") {
            SlashAction::Plan(args) => assert_eq!(args, ["on"]),
            other => panic!("expected Plan(on), got {other:?}"),
        }
        match parse_slash("/plan off") {
            SlashAction::Plan(args) => assert_eq!(args, ["off"]),
            other => panic!("expected Plan(off), got {other:?}"),
        }
        match parse_slash("/plan 修一下登录页的报错") {
            SlashAction::Plan(args) => assert_eq!(args, ["修一下登录页的报错"]),
            other => panic!("expected Plan(描述), got {other:?}"),
        }
        // 非 slash 行不该走到这里（调用方先判断 starts_with('/')）
    }

    #[test]
    fn fork_target_parsing() {
        // 纯 id
        assert_eq!(parse_fork_target("abc123"), ("abc123", None));
        // id:seq
        assert_eq!(parse_fork_target("abc123:5"), ("abc123", Some(5)));
        // 非数字 seq → 整体当 id（id 只含 [A-Za-z0-9-_]，冒号不会是 id 一部分）
        assert_eq!(parse_fork_target("abc123:x"), ("abc123:x", None));
        assert_eq!(parse_fork_target("abc123:"), ("abc123:", None));
        // 多个冒号 → rsplit 取最后一个冒号后的数字
        assert_eq!(parse_fork_target("a-b_c:7"), ("a-b_c", Some(7)));
    }

    #[test]
    fn fork_cut_keeps_last_user_message() {
        use session::Role;
        let mut s = session::Session::new("system");
        s.add_user_message("第一个问题");
        s.add_assistant_message("第一个回答");
        s.add_user_message("第二个问题");
        // fork 点 = 最后一条 User 之后（保留它，丢其后所有）
        let cut = fork_cut_point(&s.messages);
        assert_eq!(cut, 4);
        assert_eq!(s.messages.len(), 4);
        // 截断后：system + user1 + assistant1 + user2（无未答复回复被丢）
        s.messages.truncate(cut);
        let roles: Vec<Role> = s.messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            vec![Role::System, Role::User, Role::Assistant, Role::User]
        );
    }

    #[test]
    fn fork_cut_with_unanswered_user() {
        let mut s = session::Session::new("system");
        s.add_user_message("问题");
        s.add_assistant_message("回答");
        s.add_user_message("新问题（未答）");
        // 最后一条就是 User（未答复）→ 保留它
        assert_eq!(fork_cut_point(&s.messages), 4);
    }

    #[test]
    fn fork_cut_empty_keeps_system() {
        let s = session::Session::new("system");
        // 无 User 消息 → 仅保留系统提示
        assert_eq!(fork_cut_point(&s.messages), 1);
    }

    #[test]
    fn output_style_injects_switches_removes() {
        let mut s = session::Session::new("system 提示");
        apply_output_style(&mut s, "compact");
        let text = s.messages[0].content.text().to_string();
        assert!(text.contains("[Stitch output style: compact--]"));
        assert!(text.contains("只说结论与关键步骤"));
        // 切换：旧段移除，新段注入
        apply_output_style(&mut s, "verbose");
        let text = s.messages[0].content.text().to_string();
        assert!(text.contains("[Stitch output style: verbose--]"));
        assert!(!text.contains("compact"));
        assert!(text.contains("完整解释推理过程"));
        // default：仅移除标记，恢复原提示
        apply_output_style(&mut s, "default");
        let text = s.messages[0].content.text().to_string();
        assert!(!text.contains("[Stitch output style:"));
        assert!(text.starts_with("system 提示"));
        // 未知风格：不注入、不移除既有标记（保持原样）
        apply_output_style(&mut s, "bogus");
        let text = s.messages[0].content.text().to_string();
        assert!(!text.contains("[Stitch output style:"));
    }

    #[test]
    fn snippet_around_windows_and_truncates() {
        // 关键词居中：4 字符半径窗口 + 前省略号
        assert_eq!(snippet_around("abcdefghijkXYZ", "XYZ", 4), "…hijkXYZ");
        // 关键词在开头：无前省略号
        assert_eq!(snippet_around("XYZWXYZ", "XYZ", 4), "XYZWXYZ");
        // 无命中：直接截 160 字符
        assert_eq!(snippet_around("没有关键词", "xyz", 4), "没有关键词");
        let long = "a".repeat(300);
        let s = snippet_around(&long, "xyz", 4);
        assert_eq!(s.chars().count(), 160);
    }

    #[test]
    fn search_sessions_across_workspace() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let work = dir.path().display().to_string();
        let root = PathBuf::from(&work).join(".stitch").join("sessions");
        // 会话 A：含关键词两条消息（顺序保留）；会话 B：不含
        for (id, with_kw) in [("sess-a", true), ("sess-b", false)] {
            let sdir = agent::persist::session_dir(Path::new(&work), id).unwrap();
            let mut s = session::Session::new("系统提示");
            s.add_user_message(if with_kw {
                "如何配置权限模式 default"
            } else {
                "如何配置模型"
            });
            s.add_assistant_message(if with_kw {
                "用 config set 配置权限模式"
            } else {
                "用 /model 切换"
            });
            let mut m = agent::persist::Manifest::new(id, Path::new(&work));
            m.msg_count = s.messages.len();
            agent::persist::save_session(&sdir, &s, &mut m).unwrap();
            assert!(root.join(id).join("messages.jsonl").exists());
        }
        let hits = search_sessions(&work, "权限模式");
        assert_eq!(hits.len(), 2, "sess-a 两条消息都命中");
        assert!(hits.iter().all(|h| h.session_id == "sess-a"));
        assert_eq!(hits[0].seq, 0);
        assert_eq!(hits[1].seq, 1);
        assert!(hits[0].snippet.contains("权限模式"));
        // 无命中关键词
        assert!(search_sessions(&work, "不存在的词").is_empty());
        // 会话标题 = 第一条 user 消息的智能提取（list_sessions 同源）
        assert_eq!(hits[0].title, "如何配置权限模式 default");
    }

    #[test]
    fn session_title_extracts_readable() {
        // 普通请求取首行
        assert_eq!(
            session_title("修复登录页样式\n再看看导航"),
            "修复登录页样式"
        );
        // 工具回放消息还原为 !命令 / @文件
        assert_eq!(
            session_title("用户执行了命令 `cargo test`，结果：全部通过"),
            "!cargo test"
        );
        assert_eq!(
            session_title("[文件引用 src/main.rs] 请优化这段代码"),
            "@src/main.rs"
        );
        // 超长截断 60 字符
        let long = "好长的标题".repeat(20);
        assert_eq!(session_title(&long).chars().count(), 60);
        // 空白兜底
        assert_eq!(session_title("   \n  "), "<空消息>");
    }

    #[test]
    fn memory_layer_path_resolves_three_layers() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let work = dir.path().display().to_string();
        let local = memory_layer_path(&work, "local").unwrap();
        assert_eq!(local, PathBuf::from(&work).join("CLAUDE.local.md"));
        let project = memory_layer_path(&work, "project").unwrap();
        assert_eq!(project, PathBuf::from(&work).join("CLAUDE.md"));
        let global = memory_layer_path(&work, "global").unwrap();
        assert_eq!(global, crate::config::config_dir().join("CLAUDE.md"));
        assert!(memory_layer_path(&work, "bogus").is_none());
    }

    #[test]
    fn memory_preview_truncates_and_joins() {
        assert_eq!(memory_preview("a\nb\n\nc\nd"), "a | b | c");
        let long = "很长的内容".repeat(30);
        let out = memory_preview(&long);
        assert_eq!(out.chars().count(), 61); // 60 字符 + 省略号
        assert!(out.ends_with('…'));
    }

    #[test]
    fn export_session_writes_markdown() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let out = dir.path().join("export.md");
        let mut session = Session::new(String::from("system prompt"));
        session.add_user_message("帮我看看这个 bug");
        let path = out.clone();
        export_session(&session, &path).expect("export");
        let text = std::fs::read_to_string(&out).expect("read");
        assert!(text.contains("# Stitch 会话导出"));
        assert!(text.contains("## 1 · user"));
        assert!(text.contains("帮我看看这个 bug"));
    }

    #[test]
    fn list_sessions_scans_manifests() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let root = dir.path().join(".stitch").join("sessions").join("abc-123");
        std::fs::create_dir_all(&root).expect("mkdir");
        // manifest
        let manifest = agent::persist::Manifest::new("abc-123", dir.path());
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_string(&manifest).expect("json"),
        )
        .expect("write manifest");
        // messages.jsonl: system + user（标题 = 第二条）
        let system = session::Message {
            role: session::Role::System,
            content: "sys".into(),
            tool_calls: None,
            tool_call_id: None,
        };
        let user = session::Message {
            role: session::Role::User,
            content: "帮我重构这个函数".into(),
            tool_calls: None,
            tool_call_id: None,
        };
        let mut lines = String::new();
        lines.push_str(&serde_json::to_string(&system).unwrap());
        lines.push('\n');
        lines.push_str(&serde_json::to_string(&user).unwrap());
        lines.push('\n');
        std::fs::write(root.join("messages.jsonl"), lines).expect("write messages");

        let sessions = list_sessions(dir.path().to_string_lossy().as_ref());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "abc-123");
        assert_eq!(sessions[0].title, "帮我重构这个函数");
    }

    #[test]
    fn completer_slash_prefix_lists_builtin_and_custom() {
        let c = StitchCompleter::new(".");
        // /h 前缀 → /help + /hooks
        let (start, cands) = c.complete("/h", 2, &repl_test_ctx()).unwrap();
        assert_eq!(start, 0);
        assert!(cands.iter().any(|s| s == "/help"), "got {cands:?}");
        assert!(cands.iter().any(|s| s == "/hooks"), "got {cands:?}");
        // 未知前缀 → 无候选（不 panic）
        let (_, cands) = c.complete("/zzz", 4, &repl_test_ctx()).unwrap();
        assert!(cands.is_empty());
    }

    #[test]
    fn completer_slash_prefix_only_matches_at_line_start() {
        let c = StitchCompleter::new(".");
        // 前缀比输入短（中间插入）——按光标位置截取
        let (start, cands) = c.complete("/mod", 4, &repl_test_ctx()).unwrap();
        assert_eq!(start, 0);
        assert!(cands.iter().any(|s| s == "/model"), "got {cands:?}");
    }

    #[test]
    fn completer_file_names_in_work_dir() {
        let dir = std::env::temp_dir().join(format!("stitch-comp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("alpha.md"), "x").unwrap();
        std::fs::write(dir.join("beta.txt"), "x").unwrap();
        std::fs::create_dir(dir.join("subdir")).unwrap();
        let c = StitchCompleter::new(dir.to_string_lossy().as_ref());
        let (start, cands) = c.complete("al", 2, &repl_test_ctx()).unwrap();
        assert_eq!(start, 0);
        assert_eq!(cands, vec!["alpha.md"]);
        let (_, cands) = c.complete("", 0, &repl_test_ctx()).unwrap();
        assert!(cands.iter().any(|s| s.as_str() == "beta.txt"));
        let subdir_cand = format!("subdir{}", std::path::MAIN_SEPARATOR);
        assert!(
            cands.iter().any(|s| s.as_str() == subdir_cand),
            "目录候选带分隔符"
        );
        // 含路径分隔符的 token 不补全
        let (_, cands) = c.complete("C:/Users/x", 10, &repl_test_ctx()).unwrap();
        assert!(cands.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_add_dirs_rejects_missing() {
        let err = resolve_add_dirs(&["C:/definitely/not/a/real/dir".into()]).unwrap_err();
        assert!(err.to_string().contains("附加目录不存在"));
    }

    #[test]
    fn resolve_add_dirs_accepts_existing_and_empty() {
        assert!(resolve_add_dirs(&[]).unwrap().is_empty());
        let dir = tempfile::tempdir().unwrap();
        let resolved = resolve_add_dirs(&[dir.path().display().to_string()]).unwrap();
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].is_absolute());
    }
}
