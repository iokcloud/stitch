//! Copy/move file or directory tool.
//!
//! Supports copying files and directories. Directories are copied recursively.

use super::paths::resolve_under_work_dir;
use super::{ToolDef, ToolResult};
use std::path::PathBuf;

#[derive(Clone)]
pub struct CopyPath {
    work_dir: PathBuf,
}

impl Default for CopyPath {
    fn default() -> Self {
        Self::new(".")
    }
}

impl CopyPath {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "copy_path".into(),
            description: "Copy a file or directory to a new location. \
                 Directory contents are copied recursively. \
                 If moving (source and destination are on the same filesystem), \
                 specify the move parameter."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "source_path": {
                        "type": "string",
                        "description": "Source file or directory path, relative to working directory"
                    },
                    "destination_path": {
                        "type": "string",
                        "description": "Destination path, relative to working directory"
                    }
                },
                "required": ["source_path", "destination_path"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let source = arguments["source_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'source_path' argument"))?;
        let dest = arguments["destination_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'destination_path' argument"))?;

        let src_full = resolve_under_work_dir(&self.work_dir, source)?;
        let dst_full = resolve_under_work_dir(&self.work_dir, dest)?;

        if !src_full.exists() {
            return Ok(ToolResult::fail(format!("Source does not exist: {source}")));
        }

        // Create parent directories for destination
        if let Some(parent) = dst_full.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let is_dir = src_full.is_dir();

        if is_dir {
            copy_dir(&src_full, &dst_full).await?;
            Ok(ToolResult::ok(format!(
                "Copied directory: {source} -> {dest}"
            )))
        } else {
            tokio::fs::copy(&src_full, &dst_full).await?;
            Ok(ToolResult::ok(format!("Copied file: {source} -> {dest}")))
        }
    }
}

async fn copy_dir(src: &PathBuf, dst: &PathBuf) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(dst).await?;

    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            Box::pin(copy_dir(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }

    Ok(())
}
