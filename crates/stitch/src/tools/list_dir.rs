//! Directory listing tool.

use super::paths::{display_rel_under_work_dir, resolve_under_work_dir};
use super::{ToolDef, ToolResult};
use std::path::PathBuf;

/// Max entries before truncation.
const MAX_ENTRIES: usize = 100;

#[derive(Clone)]
pub struct ListDirectory {
    work_dir: PathBuf,
}

impl ListDirectory {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "list_directory".into(),
            description: "List files and directories in a given path. \
                 Shows file sizes and types. Use this to explore the project structure \
                 before reading or editing files. Dot-directories such as .agents are included."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to list, relative to working directory. Defaults to root."
                    }
                }
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        // Outside-workspace listing runs only with the gate-injected marker
        // (user-approved or matched allow rule); otherwise strictly under work_dir.
        let scoped = super::paths::scoped_allowed(&arguments);
        let target = match arguments["path"].as_str() {
            Some(p) if !p.trim().is_empty() => {
                if scoped {
                    super::paths::resolve_scoped(&self.work_dir, p)?
                } else {
                    resolve_under_work_dir(&self.work_dir, p)?
                }
            }
            _ => resolve_under_work_dir(&self.work_dir, ".")?,
        };

        let mut entries: Vec<String> = Vec::new();

        let mut read_dir = match tokio::fs::read_dir(&target).await {
            Ok(d) => d,
            Err(e) => {
                return Ok(ToolResult::fail(format!("Cannot list directory: {e}")));
            }
        };

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();

            // Skip only `.` / `..` — keep `.agents` / `.cursor` visible to the agent.
            if name == "." || name == ".." {
                continue;
            }

            let kind = if path.is_dir() { "dir" } else { "file" };
            let size = if path.is_file() {
                match tokio::fs::metadata(&path).await {
                    Ok(m) => format_size(m.len()),
                    Err(_) => String::new(),
                }
            } else {
                String::new()
            };

            // ASCII markers only (no emoji) — ADR-025 / desktop copy-tone
            entries.push(format!("[{kind}] {name} {size}"));
            if entries.len() >= MAX_ENTRIES {
                entries.push(format!("... truncated at {MAX_ENTRIES} entries"));
                break;
            }
        }

        // Sort: dirs first, then files
        entries.sort_by(|a, b| {
            let a_is_dir = a.starts_with("[dir]");
            let b_is_dir = b.starts_with("[dir]");
            b_is_dir.cmp(&a_is_dir).then_with(|| a.cmp(b))
        });

        let rel = display_rel_under_work_dir(&self.work_dir, &target);

        if entries.is_empty() {
            Ok(ToolResult::ok(format!("{rel}/ (empty)")))
        } else {
            Ok(ToolResult::ok(format!("{rel}/\n{}", entries.join("\n"))))
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("({bytes} B)")
    } else if bytes < 1024 * 1024 {
        format!("({:.1} KB)", bytes as f64 / 1024.0)
    } else {
        format!("({:.1} MB)", bytes as f64 / (1024.0 * 1024.0))
    }
}
