//! File/directory deletion tool.
//!
//! Deletes files or directories recursively. Requires user confirmation.

use super::paths::resolve_under_work_dir;
use super::{ToolDef, ToolResult};
use std::path::PathBuf;

#[derive(Clone)]
pub struct DeletePath {
    work_dir: PathBuf,
}

impl Default for DeletePath {
    fn default() -> Self {
        Self::new(".")
    }
}

impl DeletePath {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "delete_path".into(),
            description: "Delete a file or directory (recursively). \
                 Directories are deleted with all contents. Requires user confirmation. \
                 Use with caution — this is irreversible."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file or directory to delete, relative to the working directory"
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

        let full_path = resolve_under_work_dir(&self.work_dir, raw_path)?;

        if !full_path.exists() {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!("Path does not exist: {raw_path}"),
            });
        }

        let is_dir = full_path.is_dir();
        let kind = if is_dir { "directory" } else { "file" };

        // Snapshot for undo before deletion
        if is_dir {
            super::undo::snapshot_delete(&full_path);
        } else {
            super::undo::snapshot(&full_path);
        }

        if is_dir {
            tokio::fs::remove_dir_all(&full_path).await?;
        } else {
            tokio::fs::remove_file(&full_path).await?;
        }

        Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!("Deleted {kind}: {raw_path}"),
        })
    }
}
