//! Parse mainstream MCP client JSON (`mcpServers`) into [`McpServerProfile`].
//!
//! Shape matches Cursor / Claude Desktop / VS Code style configs so users can
//! paste the same JSON they already use elsewhere.

use std::collections::HashMap;

use serde_json::Value;

use crate::config::McpServerProfile;

/// Parse a Cursor/Claude-style MCP config blob into server profiles.
///
/// Accepts:
/// - `{ "mcpServers": { "id": { ... } } }`
/// - `{ "servers": { "id": { ... } } }` (VS Code-ish)
/// - a bare map `{ "id": { "command": ... } }` when every value looks like a server
pub fn parse_mcp_servers_json(raw: &str) -> anyhow::Result<Vec<McpServerProfile>> {
    let v: Value = serde_json::from_str(raw).map_err(|e| anyhow::anyhow!("JSON 无效：{e}"))?;
    let map = extract_server_map(&v)?;
    let mut out = Vec::new();
    for (id, entry) in map {
        out.push(profile_from_entry(id, entry)?);
    }
    if out.is_empty() {
        anyhow::bail!("未找到任何 MCP 服务");
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn extract_server_map(v: &Value) -> anyhow::Result<&serde_json::Map<String, Value>> {
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("根节点须为 JSON 对象"))?;
    if let Some(ms) = obj.get("mcpServers").and_then(|x| x.as_object()) {
        return Ok(ms);
    }
    if let Some(ms) = obj.get("servers").and_then(|x| x.as_object()) {
        return Ok(ms);
    }
    // Bare map of servers (every value is an object with command or url).
    if !obj.is_empty()
        && obj.values().all(|x| {
            x.as_object()
                .map(|o| o.contains_key("command") || o.contains_key("url"))
                .unwrap_or(false)
        })
    {
        return Ok(obj);
    }
    anyhow::bail!("需要 mcpServers（或 servers）对象，格式与 Cursor / Claude 一致")
}

fn profile_from_entry(id: &str, entry: &Value) -> anyhow::Result<McpServerProfile> {
    let id = id.trim();
    if id.is_empty() {
        anyhow::bail!("服务 id 不能为空");
    }
    let obj = entry
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("服务「{id}」须为对象"))?;

    let label = obj
        .get("name")
        .or_else(|| obj.get("label"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(id)
        .to_string();

    let enabled = obj.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true);

    let type_hint = obj
        .get("type")
        .or_else(|| obj.get("transport"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    let command = obj
        .get("command")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let url = obj
        .get("url")
        .or_else(|| obj.get("serverUrl"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let transport = if !type_hint.is_empty() {
        normalize_transport(&type_hint)?
    } else if command.is_some() {
        "stdio".to_string()
    } else if url.is_some() {
        "http".to_string()
    } else {
        anyhow::bail!("服务「{id}」须提供 command（stdio）或 url（HTTP）");
    };

    let args = match obj.get("args") {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        Some(Value::String(s)) => split_args_line(s),
        _ => Vec::new(),
    };

    let env = string_map(obj.get("env"));
    let mut headers = string_map(obj.get("headers"));
    // Some configs put a bare token under "env" only; Authorization may also be top-level.
    if let Some(tok) = obj
        .get("authorization")
        .or_else(|| obj.get("auth"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let value = if tok.to_ascii_lowercase().starts_with("bearer ") {
            tok.to_string()
        } else {
            format!("Bearer {tok}")
        };
        headers.entry("Authorization".into()).or_insert(value);
    }

    let cwd = obj
        .get("cwd")
        .or_else(|| obj.get("workingDirectory"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if transport == "stdio" && command.is_none() {
        anyhow::bail!("服务「{id}」为 stdio，须提供 command");
    }
    if (transport == "http" || transport == "sse") && url.is_none() {
        anyhow::bail!("服务「{id}」为远程传输，须提供 url");
    }

    Ok(McpServerProfile {
        id: id.to_string(),
        label,
        transport,
        enabled,
        command,
        args,
        env,
        cwd,
        url,
        headers,
    })
}

fn normalize_transport(raw: &str) -> anyhow::Result<String> {
    match raw {
        "stdio" | "std-io" => Ok("stdio".into()),
        "http" | "streamable-http" | "streamable_http" | "streamablehttp" => Ok("http".into()),
        "sse" | "http+sse" | "http_sse" => Ok("sse".into()),
        other => anyhow::bail!("不支持的传输方式：{other}（须为 stdio / http / sse）"),
    }
}

fn string_map(v: Option<&Value>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(Value::Object(obj)) = v else {
        return out;
    };
    for (k, val) in obj {
        if let Some(s) = val.as_str() {
            out.insert(k.clone(), s.to_string());
        } else if val.is_number() || val.is_boolean() {
            out.insert(k.clone(), val.to_string());
        }
    }
    out
}

/// Split a shell-ish args line; supports simple double quotes.
pub fn split_args_line(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in s.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cursor_style() {
        let raw = r#"{
          "mcpServers": {
            "github": {
              "command": "npx",
              "args": ["-y", "@modelcontextprotocol/server-github"],
              "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_x" }
            },
            "remote": {
              "url": "https://example.com/mcp",
              "headers": { "Authorization": "Bearer tok" }
            },
            "legacy": {
              "type": "sse",
              "url": "https://example.com/sse"
            }
          }
        }"#;
        let servers = parse_mcp_servers_json(raw).unwrap();
        assert_eq!(servers.len(), 3);
        let gh = servers.iter().find(|s| s.id == "github").unwrap();
        assert_eq!(gh.transport, "stdio");
        assert_eq!(gh.command.as_deref(), Some("npx"));
        assert_eq!(
            gh.env
                .get("GITHUB_PERSONAL_ACCESS_TOKEN")
                .map(String::as_str),
            Some("ghp_x")
        );
        let remote = servers.iter().find(|s| s.id == "remote").unwrap();
        assert_eq!(remote.transport, "http");
        let legacy = servers.iter().find(|s| s.id == "legacy").unwrap();
        assert_eq!(legacy.transport, "sse");
    }

    #[test]
    fn split_quoted_args() {
        assert_eq!(
            split_args_line(r#"-y "my package" ./path"#),
            vec!["-y", "my package", "./path"]
        );
    }
}
