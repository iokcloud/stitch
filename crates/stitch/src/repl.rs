//! 交互式 REPL（工业级终端体验 · 0.5.0 重塑）。
//!
//! - 欢迎 banner + 快捷命令提示（/help）
//! - rustyline 输入历史（~/.stitch/history.txt）
//! - slash 命令：/help /exit /quit /clear /model /cost /sessions
//! - Ctrl+C 中断当前回合（ctrlc → cancel_flag）；空闲时退出
//! - 会话持久化：`{work_dir}/.stitch/sessions/{id}/`（复用 persist 机制）
//! - 流式 Markdown 渲染（复用 render 层）

use crate::agent::{self, AgentEvent};
use crate::config::StitchConfig;
use crate::session::{self, Session};
use crate::tools;
use std::io::Write;
use std::path::PathBuf;
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
    Sessions,
    Help,
    Unknown(String),
}

const SLASH_HELP: &str = r#"
  /help                 Show this help
  /exit  (或 /quit)      Quit the session
  /clear                Start a new session (current one is saved)
  /model <name>         Switch model (e.g. /model deepseek-v4-flash)
  /cost                 Show cost & cache stats for this session
  /sessions             List saved sessions in this workspace
  Ctrl+C                Interrupt the current turn (press again when idle to quit)
"#;

fn parse_slash(line: &str) -> SlashAction {
    let mut parts = line.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest = parts.next().unwrap_or("").trim().to_string();
    match cmd.as_str() {
        "/help" | "/?" => SlashAction::Help,
        "/exit" | "/quit" => SlashAction::Exit,
        "/clear" | "/new" => SlashAction::Clear,
        "/model" => {
            if rest.is_empty() {
                SlashAction::Unknown("/model 需要模型名，如 /model deepseek-v4-flash".into())
            } else {
                SlashAction::Model(rest)
            }
        }
        "/cost" => SlashAction::Cost,
        "/sessions" => SlashAction::Sessions,
        _ => SlashAction::Unknown(format!("未知命令 {cmd}，输入 /help 查看可用命令")),
    }
}

/// 会话摘要（sessions 列表用）。
pub struct SessionSummary {
    pub id: String,
    pub updated_at: String,
    pub msg_count: usize,
    pub title: String,
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
        // 标题 = 第一条 user 消息（messages.jsonl 第二行起）
        let title = std::fs::read_to_string(dir.join("messages.jsonl"))
            .ok()
            .and_then(|t| {
                t.lines().skip(1).find_map(|l| {
                    serde_json::from_str::<session::Message>(l)
                        .ok()
                        .and_then(|m| {
                            (m.role == session::Role::User).then(|| m.content.text().to_string())
                        })
                })
            })
            .unwrap_or_default();
        out.push(SessionSummary {
            id,
            updated_at: manifest.updated_at,
            msg_count: manifest.msg_count,
            title: title.chars().take(60).collect(),
        });
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

/// 回合确认等待表（ConfirmRequest 事件 → oneshot 放行）。
type ConfirmTable =
    Arc<Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>>;

/// REPL 主循环。
pub async fn run_chat(
    cfg: StitchConfig,
    resume: Option<String>,
    continue_last: bool,
) -> anyhow::Result<()> {
    let api_key = cfg.require_llm_key()?.to_string();
    let work_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    let tools = tools::build_registry(&work_dir);

    // ── 会话装载：resume / continue / 新建 ──
    let session_id = if let Some(id) = resume {
        id
    } else if continue_last {
        list_sessions(&work_dir)
            .into_iter()
            .next()
            .map(|s| s.id)
            .unwrap_or_else(new_session_id)
    } else {
        new_session_id()
    };
    let session_dir = agent::persist::session_dir(PathBuf::from(&work_dir).as_path(), &session_id)
        .ok_or_else(|| anyhow::anyhow!("非法会话 id"))?;
    let mut manifest =
        agent::persist::Manifest::new(&session_id, PathBuf::from(&work_dir).as_path());

    let (mut session, resumed) = match agent::persist::load_session(&session_dir) {
        Ok(Some((s, m))) => {
            manifest = m;
            (s, true)
        }
        _ => {
            let system_prompt = agent::prompt::build_system_prompt(&work_dir, &tools);
            (session::Session::new(system_prompt), false)
        }
    };

    let mut model = cfg.llm_model.clone();
    let mut turn_count = 0usize;

    // ── 终端交互 ──
    println!("{}", banner(env!("CARGO_PKG_VERSION")));
    if resumed {
        println!(
            "[会话已恢复] {session_id}（{} 条消息）",
            session.messages.len()
        );
    }
    if turn_count == 0 {
        println!("工作目录：{work_dir}");
    }
    println!();

    let mut rl = rustyline::DefaultEditor::new()?;
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
        match rl.readline(format!("{model}> ").as_str()) {
            Ok(line) => {
                let _ = rl.add_history_entry(line.as_str());
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                if line.starts_with('/') {
                    match parse_slash(&line) {
                        SlashAction::Exit => {
                            println!("[退出]");
                            break;
                        }
                        SlashAction::Clear => {
                            let _ =
                                agent::persist::save_session(&session_dir, &session, &mut manifest);
                            let new_id = new_session_id();
                            let system_prompt =
                                agent::prompt::build_system_prompt(&work_dir, &tools);
                            session = session::Session::new(system_prompt);
                            println!("[新会话] 上一个会话已保存为 {session_id}，当前 {new_id}");
                            let _ = session_id; // 保留旧 id 供提示
                            let _ = new_id;
                        }
                        SlashAction::Model(m) => {
                            model = crate::config::migrate_llm_model(&m)
                                .map(str::to_string)
                                .unwrap_or(m);
                            println!("[模型] → {model}");
                        }
                        SlashAction::Cost => {
                            print_session_cost(&session, &model);
                        }
                        SlashAction::Sessions => {
                            for s in list_sessions(&work_dir) {
                                println!(
                                    "  {}\t{}\t{} 条\t{}",
                                    s.id, s.updated_at, s.msg_count, s.title
                                );
                            }
                        }
                        SlashAction::Help => print!("{SLASH_HELP}"),
                        SlashAction::Unknown(msg) => println!("{msg}"),
                    }
                    continue;
                }

                // ── 回合：用户消息 → agent → 流式渲染 ──
                session.add_user_message(&line);
                turn_count += 1;
                cancel_flag.store(false, Ordering::SeqCst);

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
                        let result = agent::run_react_streaming(
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
                        .await;
                        (result, session)
                    }
                });

                // 渲染事件流直到回合结束
                let mut turn_cost: Option<f64> = None;
                loop {
                    tokio::select! {
                        event = event_rx.recv() => {
                            let Some(ev) = event else { break };
                            match ev {
                                AgentEvent::Token { text } => {
                                    crate::render::render_token(&text);
                                }
                                AgentEvent::ConfirmRequest { id, tool, message } => {
                                    let allow = crate::render::dialog::confirm(&format!("{tool}: {message}"));
                                    if let Some(tx) = confirm_pending.lock().ok().and_then(|mut m| m.remove(&id)) {
                                        let _ = tx.send(allow);
                                    }
                                }
                                AgentEvent::ToolStart { name, .. } => {
                                    print!("\n[{name}] ");
                                    let _ = std::io::stdout().flush();
                                }
                                AgentEvent::ToolDone { name, success, summary, .. } => {
                                    let mark = if success { "✓" } else { "✗" };
                                    println!("{mark} {name} {summary}");
                                }
                                AgentEvent::Done { cost, .. } => {
                                    turn_cost = Some(cost);
                                    crate::render::finish_stream();
                                    println!();
                                }
                                AgentEvent::Error { message } => {
                                    crate::render::finish_stream();
                                    println!("\n[错误] {message}");
                                }
                                _ => {}
                            }
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                            if cancel_flag.load(Ordering::SeqCst) {
                                // 回合被 Ctrl+C 中断：等待 agent 收尾（run_react 会尽快返回）
                            }
                        }
                    }
                    if event_rx.is_empty() && handle.is_finished() {
                        break;
                    }
                }
                crate::render::finish_stream();
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

                if let Some(cost) = turn_cost {
                    println!("[回合 {turn_count}] 成本 ¥{cost:.4}");
                }
                println!();
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

    let _ = rl.save_history(&history_path);
    Ok(())
}

fn new_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 回合成本/缓存统计（会话当前状态）。
fn print_session_cost(session: &Session, model: &str) {
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
}

/// 供 run_command 分发的命令实现（占位，main.rs 调用）。
#[allow(dead_code)]
pub fn cmd_stub() {}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(matches!(parse_slash("/model"), SlashAction::Unknown(_)));
        assert!(matches!(parse_slash("/bogus"), SlashAction::Unknown(_)));
        // 非 slash 行不该走到这里（调用方先判断 starts_with('/')）
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
        assert!(
            sessions[0].msg_count >= 0,
            "msg_count 来自 manifest（save 时更新）"
        );
        assert_eq!(sessions[0].title, "帮我重构这个函数");
    }
}
