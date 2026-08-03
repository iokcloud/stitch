#![allow(dead_code)]

//! PromptStdio Agent CLI library — shared core for CLI and desktop.
//!
//! Re-exports the agent engine, LLM client, tool system, and configuration
//! so both `stitch` (CLI) and `stitch-desktop` (Tauri) can reuse the same logic.
//!
//! Many types are consumed only by `stitch-desktop` or external consumers.
pub mod agent;
/// Persisted allow rules（记住此规则）— tool + scope prefix auto-approval.
pub mod allow;
pub mod config;
pub mod llm;
/// PromptStdio REST client (suites / agents / prompts) — not MCP protocol.
pub mod mcp;
/// Standard Model Context Protocol client (stdio / Streamable HTTP).
pub mod mcp_protocol;
pub mod render;
pub mod session;
pub mod tools;

// auth / cli are CLI-specific and not re-exported
