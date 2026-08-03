//! Directory creation tool.
//!
//! Creates directories (including parents) in the working directory.

use super::paths::resolve_under_work_dir;
use super::{ToolDef, ToolResult};
use std::path::PathBuf;

#[derive(Clone)]
pub struct CreateDirectory {
    work_dir: PathBuf,
}

impl Default for CreateDirectory {
    fn default() -> Self {
        Self::new(".")
    }
}

impl CreateDirectory {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "create_directory".into(),
            description: "Create a new directory (and any missing parent directories). \
                 Returns success or error if the path already exists or creation fails."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the new directory, relative to the working directory"
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

        if full_path.exists() {
            return Ok(ToolResult::fail(format!("Path already exists: {raw_path}")));
        }

        tokio::fs::create_dir_all(&full_path).await?;

        Ok(ToolResult::ok(format!("Created directory: {raw_path}")))
    }
}
