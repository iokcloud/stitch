#![allow(dead_code)]
#![allow(clippy::disallowed_methods)] // json! 宏展开内含 unwrap（项目惯例）

// Allow dead_code — many types are consumed only by stitch-desktop or external callers.
mod agent;
mod allow;
mod auth;
mod cli;
mod config;
mod llm;
mod mcp;
mod mcp_protocol;
mod render;
mod repl;
mod session;
mod tools;

use clap::Parser;
use cli::{Cli, Command, ConfigAction};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .without_time()
        // 日志走 stderr——stdout 留给程序输出（--json 等机器可读模式不被污染）
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async { run_command(cli).await })
}

async fn run_command(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Run {
            prompt,
            model,
            yes,
            json,
        } => cmd_run(prompt, model, yes, json).await,
        Command::Chat { resume, continue_ } => {
            let cfg = config::StitchConfig::load()?;
            repl::run_chat(cfg, resume, continue_).await
        }
        Command::Sessions => {
            let work_dir = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".into());
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
            Ok(())
        }
        Command::Suite { slug } => cmd_suite(slug).await,
        Command::Agent { slug } => cmd_agent(slug).await,
        Command::Login => cmd_login().await,
        Command::Logout => cmd_logout().await,
        Command::Config { action } => cmd_config(action).await,
        Command::Upgrade => cmd_upgrade().await,
    }
}

/// 自更新：从官网版本清单拉最新版，下载对应平台二进制并覆盖自身。
/// 版本清单：https://www.promptstdio.com/downloads/stitch-cli-version.json
/// （官网 /downloads 直链，国内可直连；GitHub Release 为国际镜像）
/// 完整性：清单带 sha256，下载后比对（不匹配即中止）；防回滚：仅接受
/// 高于当前版本的更新（语义版本比较）。
async fn cmd_upgrade() -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::Write;

    const VERSION_URL: &str = "https://www.promptstdio.com/downloads/stitch-cli-version.json";
    const BASE_URL: &str = "https://www.promptstdio.com/downloads/";

    let current = env!("CARGO_PKG_VERSION");
    let client = reqwest::Client::new();
    let manifest: serde_json::Value = client
        .get(VERSION_URL)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("无法连接更新服务：{e}（请检查网络）"))?
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("更新清单解析失败：{e}"))?;
    let latest = manifest
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if latest.is_empty() {
        anyhow::bail!("更新清单缺少 version 字段");
    }
    // 防回滚：仅允许升级到更高版本
    if !version_newer(&latest, current) {
        println!("已是最新版本 v{current}。");
        return Ok(());
    }
    println!("发现新版本 v{latest}（当前 v{current}），开始下载…");

    // 平台 → 文件名 + 清单 sha256 key
    let (file, hash_key): (&str, &str) = if cfg!(target_os = "windows") {
        ("stitch-x86_64-pc-windows-msvc.exe", "windows")
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            ("stitch-aarch64-apple-darwin", "macos-arm")
        } else {
            ("stitch-x86_64-apple-darwin", "macos-x64")
        }
    } else {
        ("stitch-x86_64-unknown-linux-musl", "linux")
    };
    let expected_sha = manifest
        .get("sha256")
        .and_then(|m| m.get(hash_key))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if expected_sha.is_empty() {
        anyhow::bail!("更新清单缺少 sha256.{hash_key} 字段");
    }

    let exe = std::env::current_exe().map_err(|e| anyhow::anyhow!("无法定位当前程序路径：{e}"))?;
    let tmp = exe.with_file_name(format!(
        "{}.upgrade",
        exe.file_name().and_then(|n| n.to_str()).unwrap_or("stitch")
    ));

    // 下载到临时文件，同时计算 sha256
    let url = format!("{BASE_URL}{file}");
    let mut resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("下载失败：{e}"))?;
    let mut out = std::fs::File::create(&tmp)
        .map_err(|e| anyhow::anyhow!("无法写入临时文件 {tmp:?}：{e}"))?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| anyhow::anyhow!("下载中断：{e}"))?
    {
        hasher.update(&chunk);
        out.write_all(&chunk)?;
    }
    drop(out);

    // 完整性校验：sha256 不匹配则丢弃并中止（防止损坏/被篡改的二进制）
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(&expected_sha) {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("下载文件校验失败（sha256 不匹配），已中止升级。");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }

    // 覆盖自身：Unix 直接 rename；Windows 运行中的 exe 被锁，提示手动替换
    match std::fs::rename(&tmp, &exe) {
        Ok(()) => {
            println!("已升级到 v{latest}。");
        }
        Err(_) if cfg!(windows) => {
            println!("下载完成（校验通过），但 Windows 正在运行的程序无法覆盖自身。");
            println!("请退出当前会话后在同目录执行：");
            println!(
                "  move /y \"{}\" \"{}\"",
                tmp.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("stitch.exe.upgrade"),
                exe.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("stitch.exe"),
            );
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!("升级失败：{e}");
        }
    }
    Ok(())
}

/// 语义版本比较：`a` 是否高于 `b`（x.y.z 三段；解析失败视为不高于）。
fn version_newer(a: &str, b: &str) -> bool {
    fn parse(v: &str) -> Option<(u64, u64, u64)> {
        let mut parts = v.trim_start_matches('v').split('.');
        let x = parts.next()?.parse().ok()?;
        let y = parts.next().unwrap_or("0").parse().ok()?;
        let z = parts.next().unwrap_or("0").parse().ok()?;
        Some((x, y, z))
    }
    match (parse(a), parse(b)) {
        (Some(av), Some(bv)) => av > bv,
        _ => false,
    }
}

// -- Run --

async fn cmd_run(
    prompt: Vec<String>,
    model_override: Option<String>,
    skip_confirm: bool,
    json: bool,
) -> anyhow::Result<()> {
    let prompt = prompt.join(" ");
    if prompt.is_empty() {
        anyhow::bail!("Please provide a task description. Example: stitch run fix-the-bug");
    }

    let cfg = config::StitchConfig::load()?;
    let api_key = cfg.require_llm_key()?;
    let model = model_override.as_deref().unwrap_or(&cfg.llm_model);

    let work_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());

    let tools = tools::build_registry(&work_dir);
    let system_prompt = agent::prompt::build_system_prompt(&work_dir, &tools);
    let mut session = session::Session::new(system_prompt);
    session.add_user_message(&prompt);

    if !json {
        eprintln!("---- stitch ({model}) ----");
    }

    let allow_rules = allow::AllowRules::load();
    let result = if json {
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
            model,
            api_key,
            &tools,
            cfg.max_iterations,
            confirm_pending,
            Some(&work_dir),
            allow_rules_arc,
            &tokio::sync::mpsc::unbounded_channel::<agent::AgentEvent>().0,
            &std::sync::atomic::AtomicBool::new(false),
            None,
        )
        .await?
    } else {
        agent::run_react(
            &mut session,
            &cfg.llm_api_base,
            model,
            api_key,
            &tools,
            cfg.max_iterations,
            skip_confirm,
            Some(&work_dir),
            Some(&allow_rules),
        )
        .await?
    };

    // 成本仪表盘：回合成本 + 缓存命中率（Reasonix 式省钱可见；真实 usage 缺失时按估算）
    let cost = agent::tokens::estimate_cost(
        &agent::tokens::TokenUsage {
            input_tokens: result.input_tokens,
            output_tokens: result.output_tokens,
            cache_hit_tokens: result.cache_hit_tokens,
            cache_miss_tokens: result.cache_miss_tokens,
        },
        model,
    );

    if json {
        // 机器可读输出（--json）：stdout 只输出 JSON
        let out = serde_json::json!({
            "response": result.response,
            "iterations": result.iterations,
            "input_tokens": result.input_tokens,
            "output_tokens": result.output_tokens,
            "cache_hit_tokens": result.cache_hit_tokens,
            "cache_miss_tokens": result.cache_miss_tokens,
            "cost": cost,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
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
    let tools = tools::build_registry(&work_dir);

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
    let tools = tools::build_registry(&work_dir);

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
        Some(ConfigAction::Path) | None => {
            let path = config::config_path();
            if path.exists() {
                println!("Config: {}", path.display());
            } else {
                println!("Config: {} (not created yet)", path.display());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::version_newer;

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
}
