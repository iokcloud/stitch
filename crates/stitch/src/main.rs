// Allow dead_code — many types are consumed only by stitch-desktop or external callers.
#![allow(dead_code)]

mod agent;
mod allow;
mod auth;
mod cli;
mod config;
mod llm;
mod mcp;
mod mcp_protocol;
mod render;
mod session;
mod tools;

use clap::Parser;
use cli::{Cli, Command, ConfigAction};
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .without_time()
        .init();

    let cli = Cli::parse();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async { run_command(cli).await })
}

async fn run_command(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Run { prompt, model, yes } => cmd_run(prompt, model, yes).await,
        Command::Suite { slug } => cmd_suite(slug).await,
        Command::Agent { slug } => cmd_agent(slug).await,
        Command::Login => cmd_login().await,
        Command::Logout => cmd_logout().await,
        Command::Config { action } => cmd_config(action).await,
    }
}

// -- Run --

async fn cmd_run(
    prompt: Vec<String>,
    model_override: Option<String>,
    skip_confirm: bool,
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

    eprintln!("---- stitch ({model}) ----");

    let allow_rules = allow::AllowRules::load();
    let result = agent::run_react(
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
    .await?;

    eprintln!("---- done ({} iterations) ----", result.iterations);
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
