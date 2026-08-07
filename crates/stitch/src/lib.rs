#![allow(dead_code)]

//! PromptStdio Agent CLI library — shared core for CLI and desktop.
//!
//! Re-exports the agent engine, LLM client, tool system, and configuration
//! so both `stitch` (CLI) and `stitch-desktop` (Tauri) can reuse the same logic.
//!
//! Many types are consumed only by `stitch-desktop` or external consumers.
pub mod agent;
/// 子代理定义（`.claude/agents/*.md` + `config_dir/agents/*.md`）。
pub mod agents;
/// Persisted allow rules（记住此规则）— tool + scope prefix auto-approval.
pub mod allow;
/// 自定义 slash 命令（`.claude/commands/*.md` + `config_dir/commands/*.md`）。
pub mod commands;
pub mod config;
/// Hooks 系统（Claude Code 语义最小集）— 6 事件 command 型 hook。
pub mod hooks;
pub mod llm;
/// PromptStdio REST client (suites / agents / prompts) — not MCP protocol.
pub mod mcp;
/// Standard Model Context Protocol client (stdio / Streamable HTTP).
pub mod mcp_protocol;
/// 权限模式 + deny 规则（Claude Code 语义）。
pub mod permission;
pub mod render;
pub mod session;
/// statusLine——每回合结束的自定义状态行（config.statusline 命令）。
pub mod statusline;
pub mod tools;
pub mod upgrade;

// auth / cli are CLI-specific and not re-exported
