//! File read/write tools.

use super::paths::resolve_under_roots;
use super::{ToolDef, ToolResult, extra_roots_impl};
use std::path::PathBuf;

/// Maximum file size to read (bytes). Larger files will be truncated.
const MAX_READ_BYTES: u64 = 50_000;

#[derive(Clone)]
pub struct ReadFile {
    work_dir: PathBuf,
    extra_roots: Vec<PathBuf>,
}

impl ReadFile {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
            extra_roots: Vec::new(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "read_file".into(),
            description:
                "Read the contents of a file. Returns the file content with line numbers. \
                 Use this before editing any file to understand its current state."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file, relative to the working directory"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let raw_path = arguments["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        // Outside-workspace reads run only with the gate-injected marker
        // (user-approved or matched allow rule); everything else stays
        // strictly under work_dir.
        let full_path = if super::paths::scoped_allowed(&arguments) {
            super::paths::resolve_scoped(&self.work_dir, raw_path)?
        } else {
            resolve_under_roots(&self.roots(), raw_path)?
        };

        // .stitchignore：工作区内被忽略的文件不可读（显式人工授权的外读不拦）
        if !super::paths::scoped_allowed(&arguments) {
            let rel = super::paths::display_rel_under_work_dir(&self.work_dir, &full_path);
            if super::ignore::IgnoreRules::load(&self.work_dir).is_ignored(&rel) {
                return Ok(ToolResult::fail(format!(
                    "Cannot read {raw_path}: 文件在 .stitchignore 中被忽略"
                )));
            }
        }

        let metadata = match tokio::fs::metadata(&full_path).await {
            Ok(m) => m,
            Err(e) => {
                return Ok(ToolResult::fail(format!("Cannot read {raw_path}: {e}")));
            }
        };

        if !metadata.is_file() {
            return Ok(ToolResult::fail(format!("{raw_path} is not a file")));
        }

        let size = metadata.len();
        let content = tokio::fs::read_to_string(&full_path).await?;

        let mut output = if size > MAX_READ_BYTES {
            // Truncate and note
            let truncated: String = content.chars().take(MAX_READ_BYTES as usize).collect();
            format!(
                "{truncated}\n\n[... file truncated at {MAX_READ_BYTES} bytes, total {size} bytes]"
            )
        } else {
            // Add line numbers
            add_line_numbers(&content)
        };

        // 子目录就近记忆：文件所在目录向上的 CLAUDE.md / AGENTS.md
        // 追加到结果里，让 agent 读取深层文件时带上目录级上下文。
        if let Some(mem) = crate::agent::rules::directory_memory(&full_path, &self.roots()) {
            output = format!(
                "{output}\n\n## 目录记忆（{parent} 及上级目录的 CLAUDE.md / AGENTS.md）\n\n{mem}",
                parent = full_path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            );
        }

        Ok(ToolResult {
            metrics: None,
            success: true,
            output,
        })
    }
}

fn add_line_numbers(content: &str) -> String {
    content
        .lines()
        .enumerate()
        .map(|(i, line)| format!("{:>6}\t{line}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone)]
pub struct WriteFile {
    work_dir: PathBuf,
    extra_roots: Vec<PathBuf>,
}

impl WriteFile {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
            extra_roots: Vec::new(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "write_file".into(),
            description: "Write content to a file under the working directory, creating parent \
                 directories if needed. Path must be relative (never absolute or outside the \
                 workdir). Existing files will be overwritten. Requires user confirmation."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path relative to the working directory (e.g. src/main.rs)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let content = arguments["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

        let full_path = resolve_under_roots(&self.roots(), path)?;

        // Snapshot for undo before overwriting
        super::undo::snapshot(&full_path);

        // Create parent directories
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&full_path, content).await?;

        let lines = content.lines().count();
        let bytes = content.len();
        Ok(ToolResult::ok(format!(
            "Wrote {lines} lines ({bytes} bytes) to {path}"
        )))
    }
}
extra_roots_impl!(ReadFile);
extra_roots_impl!(WriteFile);
