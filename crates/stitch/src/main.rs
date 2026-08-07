#![allow(dead_code)]
#![allow(clippy::disallowed_methods)] // json! 宏展开内含 unwrap（项目惯例）

// Allow dead_code — many types are consumed only by stitch-desktop or external callers.
mod agent;
mod agents;
mod allow;
mod auth;
mod cli;
mod commands;
mod config;
mod hooks;
mod llm;
mod mcp;
mod mcp_protocol;
mod permission;
mod render;
mod repl;
mod session;
mod statusline;
mod tools;
mod upgrade;
mod workspace_settings;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command, ConfigAction, McpAction};
use std::io::IsTerminal;
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let default_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level)),
        )
        .with_target(false)
        .without_time()
        // 日志走 stderr——stdout 留给程序输出（--json 等机器可读模式不被污染）
        .with_writer(std::io::stderr)
        .init();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async { run_command(cli).await })
}

async fn run_command(cli: Cli) -> anyhow::Result<()> {
    // 工作区设置 .stitch/settings.json：项目内覆盖全局（CLI flag > settings > config）
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    let settings = workspace_settings::WorkspaceSettings::load(&cwd);

    // 追加系统提示：CLI flag + settings.json 合并（prompt 构建时最末尾注入）
    let mut extras = cli.append_system_prompt.clone();
    for e in &settings.append_system_prompt {
        if !extras.contains(e) {
            extras.push(e.clone());
        }
    }
    agent::prompt::set_append_prompts(extras);

    // 权限模式 + deny 规则：CLI flag > settings > config > 默认 default/空
    {
        let cfg = config::StitchConfig::load().unwrap_or_default();
        let deny =
            workspace_settings::WorkspaceSettings::merged_deny(&cfg.disallowed_tools, &settings);
        permission::apply_from_cli(
            cli.permission_mode.as_deref(),
            settings
                .permission_mode
                .as_deref()
                .or(cfg.permission_mode.as_deref()),
            &deny,
            &cli.disallowed_tools,
        )?;
    }

    // claude 语义：无参数直接进入交互对话；`-p/--print` 走单次管道模式
    let Some(command) = cli.command else {
        if let Some(prompt) = &cli.print {
            // `stitch -p "任务"` / `echo "…" | stitch -p`：跑完即退
            return cmd_run(
                vec![prompt.clone()],
                None,
                false,
                cli.json,
                cli.output_format,
                cli.verbose,
                cli.add_dir.clone(),
                cli.max_turns,
            )
            .await;
        }
        let cfg = config::StitchConfig::load()?;
        crate::upgrade::check_update_and_hint().await;
        return repl::run_chat(
            cfg,
            None,
            false,
            None,
            cli.add_dir.clone(),
            cli.budget,
            cli.max_turns,
            None,
        )
        .await;
    };
    match command {
        Command::Run { prompt, model, yes } => {
            cmd_run(
                prompt,
                model,
                yes,
                cli.json,
                cli.output_format,
                cli.verbose,
                cli.add_dir.clone(),
                cli.max_turns,
            )
            .await
        }
        Command::Chat {
            resume,
            continue_,
            fork,
            model,
        } => {
            // --fork 与 --resume / --continue 互斥（fork 本身就是一种恢复方式）
            if fork.is_some() && (resume.is_some() || continue_) {
                anyhow::bail!("--fork 不能与 --resume / --continue 同时使用");
            }
            let cfg = config::StitchConfig::load()?;
            let resume = match resume {
                Some(Some(id)) => Some(id),
                Some(None) => {
                    // `--resume` 无参：交互选择器
                    let work_dir = std::env::current_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| ".".into());
                    match pick_session(&work_dir)? {
                        Some(id) => Some(id),
                        None => {
                            println!("本工作区暂无保存的会话（运行 stitch chat 开始一个）");
                            None
                        }
                    }
                }
                None => None,
            };
            let fork = match fork {
                Some(Some(target)) => Some(target),
                Some(None) => {
                    // `--fork` 无参：交互选择器（同 --resume）
                    let work_dir = std::env::current_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| ".".into());
                    match pick_session(&work_dir)? {
                        Some(id) => Some(id),
                        None => {
                            println!("本工作区暂无保存的会话（运行 stitch chat 开始一个）");
                            None
                        }
                    }
                }
                None => None,
            };
            crate::upgrade::check_update_and_hint().await;
            repl::run_chat(
                cfg,
                resume,
                continue_,
                model,
                cli.add_dir.clone(),
                cli.budget,
                cli.max_turns,
                fork,
            )
            .await
        }
        Command::Sessions { action } => cmd_sessions(action),
        Command::Stats => cmd_stats(),
        Command::Suite { slug } => cmd_suite(slug).await,
        Command::Agent { slug } => cmd_agent(slug).await,
        Command::Login => cmd_login().await,
        Command::Logout => cmd_logout().await,
        Command::Config { action } => cmd_config(action).await,
        Command::Doctor => cmd_doctor().await,
        Command::Init => cmd_init().await,
        Command::Mcp { action } => cmd_mcp(action).await,
        Command::Upgrade => crate::upgrade::run().await,
        Command::Completions { shell } => cmd_completions(shell),
    }
}

/// `stitch completions <shell>`：输出补全脚本（clap_complete 生成）。
fn cmd_completions(shell: Option<clap_complete::Shell>) -> anyhow::Result<()> {
    let Some(shell) = shell else {
        println!("用法：stitch completions <bash|zsh|fish|powershell|elvish>");
        println!("示例：stitch completions bash > ~/.bash_completion.d/stitch");
        return Ok(());
    };
    let mut cmd = cli::Cli::command();
    clap_complete::generate(shell, &mut cmd, "stitch", &mut std::io::stdout());
    Ok(())
}

/// `--resume` 无参：交互选择要恢复的会话。
fn pick_session(work_dir: &str) -> anyhow::Result<Option<String>> {
    use dialoguer::Select;
    let sessions = repl::list_sessions(work_dir);
    if sessions.is_empty() {
        return Ok(None);
    }
    let items: Vec<String> = sessions
        .iter()
        .map(|s| {
            format!(
                "{} · {} 条 · {}",
                s.id,
                s.msg_count,
                if s.title.is_empty() {
                    "(无标题)"
                } else {
                    &s.title
                }
            )
        })
        .collect();
    let idx = Select::new()
        .with_prompt("选择要恢复的会话")
        .items(&items)
        .default(0)
        .interact_opt()?;
    Ok(idx.map(|i| sessions[i].id.clone()))
}

/// `stitch doctor`：环境诊断。
/// `stitch sessions [list|delete <id>|rename <id> <标题>]`。
fn cmd_sessions(action: Option<cli::SessionAction>) -> anyhow::Result<()> {
    let work_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    match action.unwrap_or(cli::SessionAction::List) {
        cli::SessionAction::List => {
            let sessions = repl::list_sessions(&work_dir);
            if sessions.is_empty() {
                println!("本工作区暂无保存的会话（运行 stitch chat 开始一个）");
            } else {
                for s in sessions {
                    println!(
                        "{}\t{}\t{} 条\t{}",
                        s.id, s.updated_at, s.msg_count, s.title
                    );
                }
            }
        }
        cli::SessionAction::Delete { id } => {
            let dir = agent::persist::session_dir(std::path::Path::new(&work_dir), &id)
                .ok_or_else(|| anyhow::anyhow!("非法会话 id：{id}"))?;
            if !dir.join("manifest.json").is_file() {
                anyhow::bail!("会话不存在：{id}（stitch sessions 查看）");
            }
            std::fs::remove_dir_all(&dir)?;
            println!("已删除会话：{id}");
        }
        cli::SessionAction::Rename { id, title } => {
            let dir = agent::persist::session_dir(std::path::Path::new(&work_dir), &id)
                .ok_or_else(|| anyhow::anyhow!("非法会话 id：{id}"))?;
            let path = dir.join("manifest.json");
            let text = std::fs::read_to_string(&path)
                .map_err(|_| anyhow::anyhow!("会话不存在：{id}（stitch sessions 查看）"))?;
            let mut manifest: agent::persist::Manifest = serde_json::from_str(&text)?;
            let title = title.trim().to_string();
            manifest.title = if title.is_empty() {
                None
            } else {
                Some(title.chars().take(60).collect())
            };
            agent::persist::write_manifest(&dir, &manifest)?;
            match &manifest.title {
                Some(t) => println!("已重命名会话 {id} → {t}"),
                None => println!("已清除 {id} 的自定义标题（恢复自动提取）"),
            }
        }
    }
    Ok(())
}

/// 会话统计（stitch stats）：会话数、总消息、总 tokens、时间跨度。
fn cmd_stats() -> anyhow::Result<()> {
    let work_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    let root = std::path::PathBuf::from(&work_dir)
        .join(".stitch")
        .join("sessions");
    let mut sessions: Vec<agent::persist::Manifest> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            if let Ok(text) = std::fs::read_to_string(entry.path().join("manifest.json"))
                && let Ok(m) = serde_json::from_str(&text)
            {
                sessions.push(m);
            }
        }
    }
    if sessions.is_empty() {
        println!("本工作区暂无会话数据（运行 stitch chat 开始一个会话）");
        return Ok(());
    }
    sessions.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let total_msgs: usize = sessions.iter().map(|m| m.msg_count).sum();
    let total_tokens: usize = sessions.iter().map(|m| m.estimated_tokens).sum();
    let first = &sessions[0];
    let last = sessions.last().unwrap();
    println!("会话统计（共 {} 个会话）：", sessions.len());
    println!(
        "  总消息数：{total_msgs}（平均 {} 条/会话）",
        total_msgs / sessions.len()
    );
    println!(
        "  总 tokens（估算）：{total_tokens}（平均 {} tokens/会话）",
        total_tokens / sessions.len()
    );
    println!(
        "  最早会话：{}（创建于 {}）",
        first.session_id, first.created_at
    );
    println!(
        "  最近会话：{}（更新于 {}）",
        last.session_id, last.updated_at
    );
    Ok(())
}

async fn cmd_doctor() -> anyhow::Result<()> {
    println!("── Stitch 环境诊断 ──");
    println!("版本：{}", env!("CARGO_PKG_VERSION"));

    // 1. 配置文件
    let cfg = config::StitchConfig::load()?;
    let cfg_path = config::config_dir().join("config.toml");
    if cfg_path.exists() {
        println!("配置：✓ {}", cfg_path.display());
    } else {
        println!("配置：✗ 未找到 {}（使用默认值）", cfg_path.display());
    }

    // 2. LLM key
    let key_ok = cfg.llm_api_key.is_some()
        || std::env::var("STITCH_LLM_API_KEY").is_ok()
        || std::env::var("OPENAI_API_KEY").is_ok();
    if key_ok {
        println!("LLM Key：✓ 已配置（{}）", cfg.llm_model);
    } else {
        println!("LLM Key：✗ 未配置（需 llm_api_key 或 STITCH_LLM_API_KEY）");
    }

    // 3. LLM API 连通性
    let base = cfg.llm_api_base.clone();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    match client.get(&base).send().await {
        Ok(_) => println!("LLM API：✓ {} 可达", base),
        Err(e) => println!("LLM API：✗ {} 不可达（{e}）", base),
    }

    // 4. 权限模式
    println!("权限模式：{}", permission::current().mode.as_str());

    // 5. 工作区记忆
    let wd = std::env::current_dir()?;
    for name in ["CLAUDE.md", "AGENTS.md"] {
        let p = wd.join(name);
        println!(
            "{}：{}",
            name,
            if p.exists() {
                "✓ 存在"
            } else {
                "— 未创建（stitch init 可生成模板）"
            }
        );
    }

    // 6. MCP 服务器逐个连通（复用 /mcp 健康视图逻辑）
    let servers = cfg.enabled_mcp_servers();
    if servers.is_empty() {
        println!("MCP：— 未配置（/mcp add 添加服务器）");
    } else {
        for p in servers {
            let url = p.url.as_deref().or(p.command.as_deref()).unwrap_or("");
            match crate::mcp_protocol::list_tools(p).await {
                Ok(tools) => println!("MCP：✓ {} — {url}（{} 工具）", p.label, tools.len()),
                Err(e) => println!("MCP：✗ {} — {url}（连接失败：{e}）", p.label),
            }
        }
    }

    // 7. 会话存储健康：manifest 完整性 + 可读写
    let sessions_root = wd.join(".stitch").join("sessions");
    let mut good = 0usize;
    let mut bad = 0usize;
    if let Ok(entries) = std::fs::read_dir(&sessions_root) {
        for entry in entries.flatten() {
            if entry.path().join("manifest.json").is_file() {
                good += 1;
            } else {
                bad += 1;
            }
        }
    }
    println!(
        "会话存储：✓ {} 个会话 manifest 完整{}",
        good,
        if bad > 0 {
            format!(" · {bad} 个缺 manifest（stitch sessions delete 可清）")
        } else {
            String::new()
        }
    );

    // 8. 工具注册
    let tools = tools::build_registry(&wd.display().to_string());
    println!("工具注册：{} 个", tools.definitions().len());

    // 9. 工作目录权限（写探针 + 读探测）
    let probe = wd.join(".stitch-write-probe");
    let write_ok = std::fs::write(&probe, "ok").is_ok() && std::fs::remove_file(&probe).is_ok();
    let read_ok = std::fs::read_dir(&wd).is_ok();
    match (read_ok, write_ok) {
        (true, true) => println!("工作目录：✓ 读写正常"),
        (true, false) => println!("工作目录：✗ 不可写（{} — 检查权限）", wd.display()),
        _ => println!("工作目录：✗ 不可读（{}）", wd.display()),
    }

    let count = repl::list_sessions(&wd.display().to_string()).len();
    println!("保存的会话：{count} 个");
    println!("── 诊断完成 ──");
    Ok(())
}

/// `stitch init`：生成工作区 CLAUDE.md 项目记忆模板（不覆盖已有文件）。
async fn cmd_init() -> anyhow::Result<()> {
    let wd = std::env::current_dir()?;
    let path = wd.join("CLAUDE.md");
    if path.exists() {
        anyhow::bail!("CLAUDE.md 已存在（{}），不覆盖", path.display());
    }
    let template = r#"# CLAUDE.md — 项目记忆

本文件由 `stitch init` 生成。它是 Stitch / Claude Code 的项目记忆：
每次会话开始时自动注入系统提示词。只写**长期稳定**的信息，不要写流水账。

## 项目

（一句话：这是什么项目）

## 常用命令

（构建 / 测试 / 部署命令，一行一条）

## 硬规则

（不可妥协的约定，如目录结构、命名、禁止事项）

## 技术栈

（语言 / 框架 / 关键依赖）
"#;
    std::fs::write(&path, template)?;
    println!("已生成 {}", path.display());
    println!(
        "（CLAUDE.md 会随工作区注入 agent 上下文；AGENTS.md 通用指令同理；\n  CLAUDE.local.md 为本地私有记忆——不进版本库的内容放那里，记得加入 .gitignore）"
    );
    Ok(())
}

/// `stitch mcp`：管理 MCP 服务器连接。
async fn cmd_mcp(action: Option<McpAction>) -> anyhow::Result<()> {
    use config::McpServerProfile;
    let mut cfg = config::StitchConfig::load()?;
    match action {
        None | Some(McpAction::List) => {
            if cfg.mcp_servers.is_empty() {
                println!("暂无 MCP 服务器配置");
                println!(
                    "添加：stitch mcp add <名称> --command \"npx -y 包名 参数\"  或  --url <端点>"
                );
            } else {
                println!("已配置的 MCP 服务器：");
                for s in &cfg.mcp_servers {
                    let target = match s.transport.as_str() {
                        "stdio" => s.command.clone().unwrap_or_default(),
                        _ => s.url.clone().unwrap_or_default(),
                    };
                    println!(
                        "  {}  [{}]  {}  {}",
                        s.id,
                        if s.enabled { "开" } else { "关" },
                        s.transport,
                        target
                    );
                }
            }
        }
        Some(McpAction::Add { name, command, url }) => {
            if cfg.mcp_servers.iter().any(|s| s.id == name) {
                anyhow::bail!("MCP 服务器 `{name}` 已存在");
            }
            let profile = if let Some(u) = url {
                if command.is_some() {
                    anyhow::bail!("--command 与 --url 只能二选一");
                }
                McpServerProfile {
                    id: name.clone(),
                    label: name.clone(),
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
                    url: Some(u),
                    headers: std::collections::HashMap::new(),
                }
            } else if let Some(c) = command {
                let mut parts = c.split_whitespace();
                let bin = parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--command 不能为空"))?;
                McpServerProfile {
                    id: name.clone(),
                    label: name.clone(),
                    transport: "stdio".into(),
                    enabled: true,
                    command: Some(bin.to_string()),
                    args: parts.map(str::to_string).collect(),
                    env: std::collections::HashMap::new(),
                    cwd: None,
                    url: None,
                    headers: std::collections::HashMap::new(),
                }
            } else {
                anyhow::bail!("需要 --command \"…\"（stdio）或 --url <端点>（http/sse）");
            };
            cfg.mcp_servers.push(profile);
            cfg.save()?;
            println!("[已添加] MCP 服务器 `{name}`（重启 stitch 后生效）");
        }
        Some(McpAction::Remove { name }) => {
            let before = cfg.mcp_servers.len();
            cfg.mcp_servers.retain(|s| s.id != name);
            if cfg.mcp_servers.len() == before {
                anyhow::bail!("MCP 服务器 `{name}` 不存在");
            }
            cfg.save()?;
            println!("[已移除] MCP 服务器 `{name}`");
        }
    }
    Ok(())
}

// -- Run --

/// 读取管道输入（`echo "…" | stitch run`）。stdin 为 TTY 时返回空。
async fn read_stdin_piped() -> anyhow::Result<String> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        return Ok(String::new());
    }
    use tokio::io::AsyncReadExt;
    let mut buf = String::new();
    tokio::io::stdin().read_to_string(&mut buf).await?;
    Ok(buf.trim().to_string())
}

/// 解析一行控制响应 JSON（`{"type":"control_response","id":..,"allow":..}`）。
/// 格式不符 / id 不匹配 / allow 非真 → 拒绝（安全默认）。
fn parse_control_response(line: &str, id: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let is_control = v.get("type").and_then(|x| x.as_str()) == Some("control_response");
    let vid = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
    let allow = v.get("allow").and_then(|x| x.as_bool()).unwrap_or(false);
    is_control && vid == id && allow
}

/// stream-json 确认门：读一行控制响应。500ms 超时 / EOF / 格式不符 → 拒绝。
async fn wait_control_response(
    stdin_lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::io::Stdin>>,
    id: &str,
) -> bool {
    match tokio::time::timeout(
        std::time::Duration::from_millis(500),
        stdin_lines.next_line(),
    )
    .await
    {
        Ok(Ok(Some(line))) => parse_control_response(&line, id),
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)] // 内部 helper：CLI flag 直传
async fn cmd_run(
    prompt: Vec<String>,
    model_override: Option<String>,
    skip_confirm: bool,
    json: bool,
    output_format: Option<cli::OutputFormat>,
    verbose: bool,
    add_dirs: Vec<String>,
    max_turns: Option<usize>,
) -> anyhow::Result<()> {
    let mut prompt = prompt.join(" ");
    if prompt.trim().is_empty() {
        prompt = read_stdin_piped().await?;
    }
    if prompt.trim().is_empty() {
        anyhow::bail!(
            "Please provide a task description. Example: stitch run fix-the-bug \
             (or pipe it via stdin: echo \"fix the bug\" | stitch run)"
        );
    }
    let prompt = prompt.trim().to_string();

    let cfg = config::StitchConfig::load()?;
    let api_key = cfg.require_llm_key()?;
    let work_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    let settings = workspace_settings::WorkspaceSettings::load(&work_dir);
    let model = workspace_settings::WorkspaceSettings::resolve_model(
        model_override.as_deref(),
        &settings,
        &cfg.llm_model,
    );
    // --max-turns 覆盖配置里的迭代上限（防跑飞）
    let max_iterations = max_turns.unwrap_or(cfg.max_iterations);

    let extra_roots = repl::resolve_add_dirs(&add_dirs)?;

    let mut tools = tools::build_registry_with_dirs(&work_dir, &extra_roots);
    let sub_ctx = tools::build_subagent_ctx(
        &cfg.llm_api_base,
        &model,
        api_key,
        max_iterations,
        Some(&work_dir),
        &tools,
        agents::load_agents(Some(&work_dir)),
    );
    tools::attach_subagents(&mut tools, &sub_ctx);
    let mut system_prompt = agent::prompt::build_system_prompt(&work_dir, &tools);
    agent::prompt::append_additional_dirs(&mut system_prompt, &extra_roots);
    let mut session = session::Session::new(system_prompt);
    session.add_user_message(&prompt);

    // --output-format 优先；--json 是快捷方式
    let format = output_format.unwrap_or(if json {
        cli::OutputFormat::Json
    } else {
        cli::OutputFormat::Text
    });

    let allow_rules = allow::AllowRules::load();
    let result = match format {
        cli::OutputFormat::Text => {
            eprintln!("---- stitch ({model}) ----");
            agent::run_react(
                &mut session,
                &cfg.llm_api_base,
                &model,
                api_key,
                &tools,
                max_iterations,
                skip_confirm,
                Some(&work_dir),
                Some(&allow_rules),
            )
            .await?
        }
        cli::OutputFormat::Json => {
            // 机器可读模式：静默渲染器（无终端输出），stdout 只出 JSON
            let confirm_pending = Arc::new(std::sync::Mutex::new(std::collections::HashMap::<
                String,
                tokio::sync::oneshot::Sender<bool>,
            >::new()));
            let allow_rules_arc = Arc::new(std::sync::Mutex::new(allow_rules.clone()));
            // 事件通道无人消费 → 渲染静默（stdout 只出 JSON）
            agent::run_react_streaming(
                &mut session,
                &cfg.llm_api_base,
                &model,
                api_key,
                &tools,
                max_iterations,
                confirm_pending,
                Some(&work_dir),
                allow_rules_arc,
                &tokio::sync::mpsc::unbounded_channel::<agent::AgentEvent>().0,
                &std::sync::atomic::AtomicBool::new(false),
                None,
            )
            .await?
        }
        cli::OutputFormat::StreamJson => {
            // NDJSON 事件流：每行一个 AgentEvent；确认走 control_request/response
            // 双向协议（500ms 无响应 → 拒绝）。
            let (event_tx, mut event_rx) =
                tokio::sync::mpsc::unbounded_channel::<agent::AgentEvent>();
            // 子代理事件（subagent_start/subagent_done + 内部工具事件）并入同一事件流
            sub_ctx.set_event_tx(event_tx.clone());
            // system init 事件（Claude Code 语义）：宿主做会话关联与工具枚举
            let session_id = format!(
                "{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default()
            );
            println!(
                "{}",
                serde_json::json!({
                    "type": "system",
                    "subtype": "init",
                    "session_id": session_id,
                    "model": model,
                    "work_dir": work_dir,
                    "tools": tools
                        .definitions()
                        .iter()
                        .map(|d| d.name.clone())
                        .collect::<Vec<_>>(),
                })
            );
            let confirm_pending = Arc::new(std::sync::Mutex::new(std::collections::HashMap::<
                String,
                tokio::sync::oneshot::Sender<bool>,
            >::new()));
            let allow_rules_arc = Arc::new(std::sync::Mutex::new(allow_rules.clone()));
            let cancel_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let handle = tokio::spawn({
                let tools = tools.clone();
                let api_key = api_key.to_string();
                let model = model.to_string();
                let work_dir = work_dir.clone();
                let confirm_pending = confirm_pending.clone();
                let allow_rules_arc = allow_rules_arc.clone();
                let cancel_flag = cancel_flag.clone();
                let cfg_base = cfg.llm_api_base.clone();
                async move {
                    let result = agent::run_react_streaming(
                        &mut session,
                        &cfg_base,
                        &model,
                        &api_key,
                        &tools,
                        max_iterations,
                        confirm_pending,
                        Some(&work_dir),
                        allow_rules_arc,
                        &event_tx,
                        &cancel_flag,
                        None,
                    )
                    .await;
                    (result, session)
                }
            });

            let mut stdin_lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
            loop {
                tokio::select! {
                    ev = event_rx.recv() => match ev {
                        Some(ev) => {
                            let line = serde_json::to_string(&ev)?;
                            println!("{line}");
                            if let agent::AgentEvent::ConfirmRequest { id, .. } = ev {
                                // 等待宿主控制响应；拒绝则跳过（事件已输出，
                                // 调用方可在取消语义上做文章）
                                let allow = wait_control_response(&mut stdin_lines, &id).await;
                                if let Some(tx) = confirm_pending
                                    .lock()
                                    .ok()
                                    .and_then(|mut m| m.remove(&id))
                                {
                                    let _ = tx.send(allow);
                                }
                            }
                        }
                        None => break,
                    }
                }
                if event_rx.is_empty() && handle.is_finished() {
                    break;
                }
            }
            let (turn_result, _returned_session) = handle
                .await
                .map_err(|e| anyhow::anyhow!("回合执行失败：{e}"))?;
            turn_result?
        }
    };

    // 成本仪表盘：回合成本 + 缓存命中率（Reasonix 式省钱可见；真实 usage 缺失时按估算）
    let cost = agent::tokens::estimate_cost(
        &agent::tokens::TokenUsage {
            input_tokens: result.input_tokens,
            output_tokens: result.output_tokens,
            cache_hit_tokens: result.cache_hit_tokens,
            cache_miss_tokens: result.cache_miss_tokens,
        },
        &model,
    );

    if format == cli::OutputFormat::Json {
        // 机器可读输出（--json / --output-format json）：stdout 只输出 JSON
        let out = serde_json::json!({
            "response": result.response,
            "iterations": result.iterations,
            "input_tokens": result.input_tokens,
            "output_tokens": result.output_tokens,
            "cache_hit_tokens": result.cache_hit_tokens,
            "cache_miss_tokens": result.cache_miss_tokens,
            "cost": cost,
            // 工具失败计数（非 0 → 退出码 1）——CI 判断任务是否干净完成
            "tool_errors": result.tool_errors,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return if result.tool_errors > 0 {
            std::process::exit(1)
        } else {
            Ok(())
        };
    }

    if format == cli::OutputFormat::StreamJson {
        // 事件流已逐行输出；末行补一条 result 汇总，宿主无需再解析事件。
        let out = serde_json::json!({
            "type": "result",
            "response": result.response,
            "iterations": result.iterations,
            "input_tokens": result.input_tokens,
            "output_tokens": result.output_tokens,
            "cache_hit_tokens": result.cache_hit_tokens,
            "cache_miss_tokens": result.cache_miss_tokens,
            "cost": cost,
            "tool_errors": result.tool_errors,
        });
        println!("{}", serde_json::to_string(&out)?);
        return if result.tool_errors > 0 {
            std::process::exit(1)
        } else {
            Ok(())
        };
    }

    eprintln!("---- done ({} iterations) ----", result.iterations);
    let hit_total = result.cache_hit_tokens + result.cache_miss_tokens;
    let hit_pct = result
        .cache_hit_tokens
        .saturating_mul(100)
        .checked_div(hit_total)
        .map(|pct| format!("{pct}%"))
        .unwrap_or_else(|| "—".to_string());
    eprintln!(
        "---- 成本 ¥{:.4} · 缓存命中 {} · 输入 {} tokens / 输出 {} tokens ----",
        cost, hit_pct, result.input_tokens, result.output_tokens,
    );
    if verbose {
        eprintln!(
            "---- verbose: model={model} · api={} · 上下文 {} / {} tokens（缓存命中 {} / 未命中 {}）----",
            cfg.llm_api_base,
            result.context_tokens,
            result.context_limit,
            result.cache_hit_tokens,
            result.cache_miss_tokens,
        );
    }
    if result.tool_errors > 0 {
        eprintln!(
            "---- 有 {n} 个工具调用失败（exit 1）——用 --json 看 tool_errors 明细 ----",
            n = result.tool_errors
        );
        std::process::exit(1);
    }
    Ok(())
}

// -- Suite / Agent --

async fn cmd_suite(slug: String) -> anyhow::Result<()> {
    let cfg = config::StitchConfig::load()?;
    let client = mcp::McpClient::new(cfg.api_base.clone(), cfg.api_token.clone());

    let suite = client.get_suite(&slug).await?;
    eprintln!("---- Suite: {} ----", suite.title);
    if let Some(ref desc) = suite.description {
        eprintln!("{desc}");
    }
    eprintln!(
        "Steps: {} | Tags: {}",
        suite.step_count,
        suite
            .tags
            .as_ref()
            .map(|t| t.join(", "))
            .unwrap_or_default(),
    );
    eprintln!();

    if suite.steps.is_empty() {
        anyhow::bail!("Suite has no steps to execute.");
    }

    let api_key = cfg.require_llm_key()?;
    let model = &cfg.llm_model;
    let work_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    let mut tools = tools::build_registry(&work_dir);
    let sub_ctx = tools::build_subagent_ctx(
        &cfg.llm_api_base,
        model,
        api_key,
        cfg.max_iterations,
        Some(&work_dir),
        &tools,
        agents::load_agents(Some(&work_dir)),
    );
    tools::attach_subagents(&mut tools, &sub_ctx);

    let mut total_iterations = 0usize;

    for step in &suite.steps {
        let label = format!(
            "Step {}/{}: {}",
            step.position, suite.step_count, step.step_title
        );
        eprintln!("---- {label} ----");

        let system_prompt = agent::prompt::build_system_prompt(&work_dir, &tools);
        let mut session = session::Session::new(system_prompt);

        // Include previous step context as a brief summary, then the current step content.
        let user_message = format!(
            "你正在执行任务套件「{}」的步骤 {}/{}。\n\n任务说明：{}\n\n请执行以下步骤，完成后简要报告结果：\n\n## {}\n\n{}",
            suite.title,
            step.position,
            suite.step_count,
            suite.description.as_deref().unwrap_or("无"),
            step.step_title,
            step.content,
        );
        session.add_user_message(&user_message);

        let result = agent::run_react(
            &mut session,
            &cfg.llm_api_base,
            model,
            api_key,
            &tools,
            cfg.max_iterations,
            true, // skip confirm for suite steps
            None,
            None,
        )
        .await?;

        total_iterations += result.iterations;

        let response_preview: String = result.response.chars().take(300).collect();
        eprintln!(
            "  -> {response_preview}{}",
            if result.response.chars().count() > 300 {
                "..."
            } else {
                ""
            }
        );
        eprintln!();
    }

    eprintln!(
        "---- Suite done ({} steps, {} total iterations) ----",
        suite.step_count, total_iterations
    );
    Ok(())
}

async fn cmd_agent(slug: String) -> anyhow::Result<()> {
    let cfg = config::StitchConfig::load()?;
    let client = mcp::McpClient::new(cfg.api_base.clone(), cfg.api_token.clone());

    let plan = client.run_agent_by_name(&slug, None).await?;

    let name = plan.get("name").and_then(|v| v.as_str()).unwrap_or(&slug);
    let suite_title = plan
        .get("task_suite_title")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    eprintln!("---- Agent: {name} (suite: {suite_title}) ----");

    let steps = plan
        .get("steps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Agent run plan missing 'steps' array"))?;

    if steps.is_empty() {
        anyhow::bail!("Agent has no steps to execute.");
    }

    let total_steps = steps.len();
    eprintln!("Steps: {total_steps}");

    // Print orchestration rules if available
    if let Some(rules) = plan.get("orchestration_rules").and_then(|v| v.as_array()) {
        eprintln!("\nOrchestration rules:");
        for (i, rule) in rules.iter().enumerate() {
            if let Some(r) = rule.as_str() {
                eprintln!("  {}. {r}", i + 1);
            }
        }
    }
    eprintln!();

    let api_key = cfg.require_llm_key()?;
    let model = &cfg.llm_model;
    let work_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    let mut tools = tools::build_registry(&work_dir);
    let sub_ctx = tools::build_subagent_ctx(
        &cfg.llm_api_base,
        model,
        api_key,
        cfg.max_iterations,
        Some(&work_dir),
        &tools,
        agents::load_agents(Some(&work_dir)),
    );
    tools::attach_subagents(&mut tools, &sub_ctx);

    // Build system prompt with agent orchestration rules
    let orch_rules_str = plan
        .get("orchestration_rules")
        .and_then(|v| v.as_array())
        .map(|rules| {
            rules
                .iter()
                .filter_map(|r| r.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    let completion_instruction = plan
        .get("completion")
        .and_then(|v| v.get("instruction"))
        .and_then(|v| v.as_str())
        .unwrap_or("执行完成后报告结果。");

    let system_prompt = format!(
        "{}\n\n## 编排规则\n{orch_rules_str}\n\n## 完成指引\n{completion_instruction}",
        agent::prompt::build_system_prompt(&work_dir, &tools),
    );

    let mut session = session::Session::new(system_prompt);

    // Feed all steps as a single user message
    let mut task_description = format!("执行智能体「{name}」的 {total_steps} 个步骤：\n\n");
    for step in steps {
        let pos = step.get("position").and_then(|v| v.as_i64()).unwrap_or(0);
        let title = step
            .get("step_title")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let content = step.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let preview = step
            .get("content_preview")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        task_description.push_str(&format!(
            "## 步骤 {pos}: {title}\n\n{}\n\n",
            if content.is_empty() { preview } else { content }
        ));
    }

    session.add_user_message(&task_description);

    eprintln!("---- Running agent with {} ----", model);
    let result = agent::run_react(
        &mut session,
        &cfg.llm_api_base,
        model,
        api_key,
        &tools,
        cfg.max_iterations,
        true, // skip confirm for agent steps
        None,
        None,
    )
    .await?;

    eprintln!("---- Agent done ({} iterations) ----", result.iterations);
    Ok(())
}

// -- Auth --

async fn cmd_login() -> anyhow::Result<()> {
    let mut cfg = config::StitchConfig::load()?;

    if std::env::var("STITCH_API_TOKEN").is_ok() {
        println!("Using STITCH_API_TOKEN from environment.");
        return Ok(());
    }

    let token = dialoguer::Input::<String>::new()
        .with_prompt("Paste your PromptStdio API token")
        .interact_text()?;

    cfg.api_token = Some(token);
    cfg.save()?;
    println!(
        "Logged in. Token saved to {}",
        config::config_path().display()
    );
    Ok(())
}

async fn cmd_logout() -> anyhow::Result<()> {
    auth::logout(&mut config::StitchConfig::load()?).await
}

// -- Config --

async fn cmd_config(action: Option<ConfigAction>) -> anyhow::Result<()> {
    let mut cfg = config::StitchConfig::load()?;

    match action {
        Some(ConfigAction::Set { key, value }) => {
            let display = if key == "llm_api_key" || key == "api_token" {
                "****"
            } else {
                &value
            };
            cfg.set(&key, &value)?;
            cfg.save()?;
            println!("{key} = {display}");
        }
        Some(ConfigAction::Get { key }) => {
            let value = cfg.get(&key)?;
            let display = if key == "llm_api_key" || key == "api_token" {
                if value.is_empty() || value == "(not set)" {
                    value
                } else {
                    "**** (set)".into()
                }
            } else {
                value
            };
            println!("{key} = {display}");
        }
        Some(ConfigAction::List) => {
            // 全部有效键；密钥掩码（与 get 一致）
            for key in [
                "api_base",
                "api_token",
                "llm_provider",
                "llm_api_base",
                "llm_api_key",
                "llm_model",
                "max_iterations",
                "local_vision_enabled",
                "local_vision_api_base",
                "local_vision_model",
                "work_dir",
                "sediment_visibility",
                "statusline",
                "permission_mode",
                "disallowed_tools",
            ] {
                let value = cfg.get(key)?;
                let display = if key == "llm_api_key" || key == "api_token" {
                    if value.is_empty() || value == "(not set)" {
                        value
                    } else {
                        "**** (set)".into()
                    }
                } else {
                    value
                };
                println!("{key} = {display}");
            }
        }
        Some(ConfigAction::Path) => {
            let path = config::config_path();
            if path.exists() {
                println!("Config: {}", path.display());
            } else {
                println!("Config: {} (not created yet)", path.display());
            }
        }
        None => {
            // 非交互终端（管道/CI）：退化为打印配置路径
            if !std::io::stdin().is_terminal() {
                let path = config::config_path();
                if path.exists() {
                    println!("Config: {}", path.display());
                } else {
                    println!("Config: {} (not created yet)", path.display());
                }
                return Ok(());
            }
            config_wizard(&mut cfg)?;
        }
    }
    Ok(())
}

/// `stitch config` 交互式便捷配置向导：模型 / API Base / API Key / 迭代上限。
/// 直接改内存配置并保存；Key 用掩码输入，留空表示不修改。
fn config_wizard(cfg: &mut config::StitchConfig) -> anyhow::Result<()> {
    use dialoguer::{Input, Password, Select, theme::ColorfulTheme};

    println!("配置向导（直接回车 = 保留当前值）");

    // 1. 模型
    let mut models = vec![
        "deepseek-v4-flash".to_string(),
        "deepseek-v4-pro".to_string(),
    ];
    let current_model = cfg.llm_model.clone();
    if !models.contains(&current_model) {
        models.push(current_model.clone());
    }
    models.push("自定义…".to_string());
    let default_idx = models.iter().position(|m| *m == current_model).unwrap_or(0);
    let chosen = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("模型")
        .default(default_idx)
        .items(&models)
        .interact()?;
    let model = if models[chosen] == "自定义…" {
        Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("模型名称")
            .default(current_model.clone())
            .interact_text()?
            .trim()
            .to_string()
    } else {
        models[chosen].clone()
    };

    // 2. API Base
    let api_base = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("API Base")
        .default(cfg.llm_api_base.clone())
        .interact_text()?
        .trim()
        .to_string();

    // 3. API Key（掩码；留空 = 不修改）
    let key_hint = if cfg.llm_api_key.as_deref().is_some_and(|k| !k.is_empty()) {
        "（已设置，留空保留）"
    } else {
        "（未设置）"
    };
    let key = Password::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("API Key {key_hint}"))
        .allow_empty_password(true)
        .interact()?;
    let key = key.trim().to_string();

    // 4. 迭代上限
    let iterations = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("单次任务最大迭代数")
        .default(cfg.max_iterations.to_string())
        .interact_text()?;
    let iterations = iterations
        .trim()
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("迭代数必须是数字"))?;

    if model != current_model {
        cfg.set("llm_model", &model)?;
    }
    if api_base != cfg.llm_api_base {
        cfg.set("llm_api_base", &api_base)?;
    }
    if !key.is_empty() {
        cfg.set("llm_api_key", &key)?;
    }
    if iterations != cfg.max_iterations {
        cfg.set("max_iterations", &iterations.to_string())?;
    }
    cfg.save()?;
    println!("已保存到 {}", config::config_path().display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_control_response;
    use crate::upgrade::{update_hint_text, version_newer};

    #[test]
    fn update_hint_only_when_newer() {
        assert!(update_hint_text("0.5.0", "0.4.1").is_some());
        assert!(update_hint_text("0.4.1", "0.4.1").is_none(), "同版本无提示");
        assert!(update_hint_text("0.4.0", "0.4.1").is_none(), "低版本无提示");
        assert!(update_hint_text("abc", "0.4.1").is_none(), "非法版本无提示");
        let text = update_hint_text("0.5.0", "0.4.1").unwrap();
        assert!(text.contains("0.5.0") && text.contains("upgrade"));
    }

    #[test]
    fn version_compare_guards_rollback() {
        assert!(version_newer("0.4.0", "0.3.0"));
        assert!(version_newer("0.3.1", "0.3.0"));
        assert!(version_newer("1.0.0", "0.9.9"));
        assert!(!version_newer("0.3.0", "0.3.0"), "同版本不升级");
        assert!(!version_newer("0.2.9", "0.3.0"), "低版本不降级");
        assert!(!version_newer("abc", "0.3.0"), "非法版本不升级");
        assert!(!version_newer("0.3.0", "abc"));
    }

    #[test]
    fn control_response_parsing() {
        // 匹配 → 放行
        assert!(parse_control_response(
            r#"{"type":"control_response","id":"confirm-call_1","allow":true}"#,
            "confirm-call_1"
        ));
        // id 不匹配 → 拒绝
        assert!(!parse_control_response(
            r#"{"type":"control_response","id":"confirm-other","allow":true}"#,
            "confirm-call_1"
        ));
        // allow=false → 拒绝
        assert!(!parse_control_response(
            r#"{"type":"control_response","id":"confirm-call_1","allow":false}"#,
            "confirm-call_1"
        ));
        // 缺 type / 非 JSON / 非对象 → 拒绝
        assert!(!parse_control_response(
            r#"{"id":"confirm-call_1","allow":true}"#,
            "confirm-call_1"
        ));
        assert!(!parse_control_response("not json", "confirm-call_1"));
        assert!(!parse_control_response("42", "confirm-call_1"));
    }
}
