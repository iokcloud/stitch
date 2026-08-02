//! Standard Model Context Protocol client (stdio + Streamable HTTP / SSE URL).
//!
//! Distinct from [`crate::mcp`] which is PromptStdio REST for suites / sediment.
//!
//! Config shape aligns with mainstream clients (Cursor / Claude Desktop): see [`import`].

mod import;

#[allow(unused_imports)] // re-export for stitch-desktop; binary also compiles this module
pub use import::{parse_mcp_servers_json, split_args_line};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use http::{HeaderName, HeaderValue};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, Tool as RmcpTool};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use serde_json::Value;

use crate::config::McpServerProfile;

fn hide_console(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Prepend common Node / package-manager bin dirs so GUI-launched Stitch finds `npx`.
fn enhance_path_env(command: &mut tokio::process::Command) {
    let mut prefixes: Vec<String> = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            let p = PathBuf::from(pf).join("nodejs");
            if p.is_dir() {
                prefixes.push(p.to_string_lossy().into_owned());
            }
        }
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
            let p = PathBuf::from(pf86).join("nodejs");
            if p.is_dir() {
                prefixes.push(p.to_string_lossy().into_owned());
            }
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            for rel in ["Programs\\nodejs", "fnm"] {
                let p = PathBuf::from(&local).join(rel);
                if p.is_dir() {
                    prefixes.push(p.to_string_lossy().into_owned());
                }
            }
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            let p = PathBuf::from(appdata).join("npm");
            if p.is_dir() {
                prefixes.push(p.to_string_lossy().into_owned());
            }
        }
        if let Ok(user) = std::env::var("USERPROFILE") {
            for rel in ["AppData\\Roaming\\npm", ".local\\bin", "scoop\\shims"] {
                let p = PathBuf::from(&user).join(rel);
                if p.is_dir() {
                    prefixes.push(p.to_string_lossy().into_owned());
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            for rel in [".local/bin", ".nvm/current/bin", ".fnm/current/bin"] {
                let p = PathBuf::from(&home).join(rel);
                if p.is_dir() {
                    prefixes.push(p.to_string_lossy().into_owned());
                }
            }
        }
        let usr = PathBuf::from("/usr/local/bin");
        if usr.is_dir() {
            prefixes.push(usr.to_string_lossy().into_owned());
        }
    }

    if prefixes.is_empty() {
        return;
    }
    #[cfg(windows)]
    let sep = ";";
    #[cfg(not(windows))]
    let sep = ":";
    let current = std::env::var("PATH").unwrap_or_default();
    let mut new_path = prefixes.join(sep);
    if !current.is_empty() {
        new_path.push_str(sep);
        new_path.push_str(&current);
    }
    command.env("PATH", new_path);
}

#[cfg(windows)]
fn is_shell_shim(cmd: &str) -> bool {
    matches!(
        cmd.to_ascii_lowercase().as_str(),
        "npx" | "npm" | "pnpm" | "yarn" | "node" | "uvx" | "uv" | "bun" | "deno"
    )
}

fn build_stdio_command(profile: &McpServerProfile) -> anyhow::Result<tokio::process::Command> {
    let cmd = profile
        .command
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("stdio 服务缺少命令"))?;

    let mut command = {
        #[cfg(windows)]
        {
            // GUI apps often miss interactive PATH; route shims through cmd.exe.
            if is_shell_shim(cmd) && !cmd.contains(['/', '\\']) {
                let mut c = tokio::process::Command::new("cmd.exe");
                c.arg("/D").arg("/C").arg(cmd);
                for a in &profile.args {
                    c.arg(a);
                }
                c
            } else {
                let mut c = tokio::process::Command::new(cmd);
                c.args(&profile.args);
                c
            }
        }
        #[cfg(not(windows))]
        {
            let mut c = tokio::process::Command::new(cmd);
            c.args(&profile.args);
            c
        }
    };

    for (k, v) in &profile.env {
        command.env(k, v);
    }
    if let Some(cwd) = profile
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        command.current_dir(cwd);
    }
    enhance_path_env(&mut command);
    hide_console(&mut command);
    Ok(command)
}

/// A tool discovered from one MCP server.
#[derive(Debug, Clone)]
pub struct DiscoveredMcpTool {
    pub server_id: String,
    pub remote_name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Qualified LLM tool name: `mcp__{server_id}__{remote_name}`.
pub fn qualify_tool_name(server_id: &str, remote_name: &str) -> String {
    let sid = sanitize_id(server_id);
    let name = sanitize_id(remote_name);
    format!("mcp__{sid}__{name}")
}

fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Parse `mcp__server__tool` → (server_id, remote_name). Remote may contain `__`.
pub fn parse_qualified_tool_name(qualified: &str) -> Option<(&str, &str)> {
    let rest = qualified.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server, tool))
}

fn tool_from_rmcp(server_id: &str, t: &RmcpTool) -> DiscoveredMcpTool {
    DiscoveredMcpTool {
        server_id: server_id.to_string(),
        remote_name: t.name.to_string(),
        description: t
            .description
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default(),
        input_schema: Value::Object(t.input_schema.as_ref().clone()),
    }
}

fn content_to_string(result: &rmcp::model::CallToolResult) -> String {
    if result.content.is_empty() {
        if let Some(err) = &result.is_error
            && *err
        {
            return "MCP tool reported an error (no content)".into();
        }
        return String::new();
    }
    let mut parts = Vec::new();
    for c in &result.content {
        if let Some(text) = c.as_text() {
            parts.push(text.text.clone());
        } else {
            parts.push(format!("{c:?}"));
        }
    }
    parts.join("\n")
}

async fn with_stdio_client<F, Fut, T>(profile: &McpServerProfile, f: F) -> anyhow::Result<T>
where
    F: FnOnce(rmcp::service::RunningService<rmcp::RoleClient, ()>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let command = build_stdio_command(profile)?;
    let transport = TokioChildProcess::new(command.configure(|_c| {})).map_err(|e| {
        anyhow::anyhow!("启动 MCP 进程失败（检查命令、参数、环境变量与 PATH）: {e}")
    })?;
    let client = ()
        .serve(transport)
        .await
        .map_err(|e| anyhow::anyhow!("连接 MCP(stdio) 失败: {e}"))?;
    let out = f(client).await?;
    Ok(out)
}

async fn with_http_client<F, Fut, T>(profile: &McpServerProfile, f: F) -> anyhow::Result<T>
where
    F: FnOnce(rmcp::service::RunningService<rmcp::RoleClient, ()>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let url = profile
        .url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("远程服务缺少地址"))?;
    let mut headers = HashMap::new();
    for (k, v) in &profile.headers {
        let name = HeaderName::try_from(k.as_str())
            .map_err(|e| anyhow::anyhow!("无效请求头名 {k}: {e}"))?;
        let value = HeaderValue::try_from(v.as_str())
            .map_err(|e| anyhow::anyhow!("无效请求头值 {k}: {e}"))?;
        headers.insert(name, value);
    }
    let config =
        StreamableHttpClientTransportConfig::with_uri(url.to_string()).custom_headers(headers);
    let transport = StreamableHttpClientTransport::from_config(config);
    let label = if profile.transport == "sse" {
        "SSE/HTTP"
    } else {
        "HTTP"
    };
    let client = ().serve(transport).await.map_err(|e| {
        anyhow::anyhow!(
            "连接 MCP({label}) 失败: {e}（须为 Streamable HTTP；纯旧版 SSE 端点可能不兼容）"
        )
    })?;
    let out = f(client).await?;
    Ok(out)
}

/// List tools from one server profile (connect → list → disconnect).
pub async fn list_tools(profile: &McpServerProfile) -> anyhow::Result<Vec<DiscoveredMcpTool>> {
    let server_id = profile.id.clone();
    match profile.transport.as_str() {
        "stdio" => {
            with_stdio_client(profile, |client| async move {
                let tools = client
                    .list_all_tools()
                    .await
                    .map_err(|e| anyhow::anyhow!("tools/list 失败: {e}"))?;
                let _ = client.cancel().await;
                Ok(tools
                    .iter()
                    .map(|t| tool_from_rmcp(&server_id, t))
                    .collect())
            })
            .await
        }
        "http" | "sse" => {
            with_http_client(profile, |client| async move {
                let tools = client
                    .list_all_tools()
                    .await
                    .map_err(|e| anyhow::anyhow!("tools/list 失败: {e}"))?;
                let _ = client.cancel().await;
                Ok(tools
                    .iter()
                    .map(|t| tool_from_rmcp(&server_id, t))
                    .collect())
            })
            .await
        }
        other => anyhow::bail!("不支持的传输方式：{other}"),
    }
}

/// Call one remote tool (connect → call → disconnect).
pub async fn call_tool(
    profile: &McpServerProfile,
    remote_name: &str,
    arguments: Value,
) -> anyhow::Result<String> {
    let name = remote_name.to_string();
    let args_obj = match arguments {
        Value::Object(m) => m,
        Value::Null => serde_json::Map::new(),
        other => {
            let mut m = serde_json::Map::new();
            m.insert("value".into(), other);
            m
        }
    };
    let params = CallToolRequestParams::new(name.clone()).with_arguments(args_obj);

    match profile.transport.as_str() {
        "stdio" => {
            with_stdio_client(profile, |client| async move {
                let result = client
                    .call_tool(params)
                    .await
                    .map_err(|e| anyhow::anyhow!("tools/call 失败: {e}"))?;
                let _ = client.cancel().await;
                if result.is_error == Some(true) {
                    anyhow::bail!("{}", content_to_string(&result));
                }
                Ok(content_to_string(&result))
            })
            .await
        }
        "http" | "sse" => {
            with_http_client(profile, |client| async move {
                let result = client
                    .call_tool(params)
                    .await
                    .map_err(|e| anyhow::anyhow!("tools/call 失败: {e}"))?;
                let _ = client.cancel().await;
                if result.is_error == Some(true) {
                    anyhow::bail!("{}", content_to_string(&result));
                }
                Ok(content_to_string(&result))
            })
            .await
        }
        other => anyhow::bail!("不支持的传输方式：{other}"),
    }
}

/// Discover tools from all enabled servers; skip failures with a warning.
pub async fn discover_enabled(servers: &[McpServerProfile]) -> Vec<DiscoveredMcpTool> {
    let mut out = Vec::new();
    for p in servers.iter().filter(|p| p.enabled) {
        match list_tools(p).await {
            Ok(tools) => out.extend(tools),
            Err(e) => {
                tracing::warn!(
                    server = %p.id,
                    error = %e,
                    "MCP server skipped (list_tools failed)"
                );
            }
        }
    }
    out
}

/// Shared handle stored on dynamic Tool entries.
#[derive(Clone)]
pub struct McpToolRuntime {
    pub profile: Arc<McpServerProfile>,
    pub remote_name: String,
}

impl McpToolRuntime {
    pub async fn execute(&self, arguments: Value) -> anyhow::Result<String> {
        call_tool(&self.profile, &self.remote_name, arguments).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualify_and_parse() {
        let q = qualify_tool_name("my-server", "list_files");
        assert_eq!(q, "mcp__my-server__list_files");
        let (s, t) = parse_qualified_tool_name(&q).unwrap();
        assert_eq!(s, "my-server");
        assert_eq!(t, "list_files");
        assert!(parse_qualified_tool_name("list_directory").is_none());
    }
}
