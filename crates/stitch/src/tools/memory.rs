//! 工作区记忆（文件系统式 · docs/LONG-HORIZON-CONTEXT-PLAN.md 记忆写闭环）。
//!
//! 模型自主调用 `save_memory` 把关键结论写入 `<work_dir>/.stitch-memory.md`，
//! 下次会话自动加载注入系统提示——跨会话学习。用户可见可编辑（Markdown），
//! 无专用存储后端（Anthropic 2026 结论：记忆 = 标准文件系统 + 自主写入）。

use super::{ToolDef, ToolResult};
use std::fs;
use std::path::{Path, PathBuf};

/// 记忆文件（工作区内）。
pub fn memory_file_path(work_dir: &str) -> PathBuf {
    Path::new(work_dir).join(".stitch-memory.md")
}

/// 读取记忆内容（会话加载用；文件不存在返回空）。
pub fn load_memory(work_dir: &str) -> String {
    fs::read_to_string(memory_file_path(work_dir)).unwrap_or_default()
}

#[derive(Clone)]
pub struct SaveMemory {
    work_dir: PathBuf,
}

impl SaveMemory {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "save_memory".into(),
            description:
                "Save a key fact or conclusion to the workspace memory (.stitch-memory.md). \
                 Loaded automatically at the start of future sessions in this workspace. \
                 Use for: project conventions, solved problems, important decisions, \
                 reusable insights. Same title overwrites the previous entry; \
                 keep entries short and self-contained."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Short title, e.g. build-command"
                    },
                    "content": {
                        "type": "string",
                        "description": "The fact/conclusion to remember (1-5 lines)"
                    }
                },
                "required": ["title", "content"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let title = arguments["title"].as_str().map(str::trim).unwrap_or("");
        let content = arguments["content"].as_str().map(str::trim).unwrap_or("");
        if title.is_empty() || content.is_empty() {
            return Ok(ToolResult::fail("参数 title、content 均不能为空"));
        }

        let work_dir = self.work_dir.to_string_lossy().to_string();
        let path = memory_file_path(&work_dir);
        let mut text = load_memory(&work_dir);
        let entry = format!("{title}\n\n{content}\n");

        // 同标题覆盖（保持记忆精简）：按 `## ` 块切分替换
        let mut replaced = false;
        let mut blocks: Vec<String> = text.split("## ").map(|s| s.to_string()).collect();
        for b in blocks.iter_mut() {
            if b.starts_with(&format!("{title}\n")) {
                *b = entry.clone();
                replaced = true;
                break;
            }
        }
        if replaced {
            text = blocks.join("## ");
        } else {
            text.push_str(&format!("## {entry}\n"));
        }
        text = text.trim_start_matches('\n').to_string();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&path, text).map_err(|e| anyhow::anyhow!("写入记忆文件失败：{e}"))?;
        Ok(ToolResult::ok(format!(
            "已写入工作区记忆：{title}（同标题条目已覆盖）"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("tempdir")
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = temp_dir();
        let tool = SaveMemory::new(dir.path());
        let args =
            serde_json::json!({"title": "build-command", "content": "用 cargo build -p stitch"});
        let result = tokio::runtime::Runtime::new()
            .expect("rt")
            .block_on(tool.execute(args))
            .expect("execute");
        assert!(result.success, "写入应成功: {:?}", result.output);

        let mem = load_memory(dir.path().to_string_lossy().as_ref());
        assert!(mem.contains("## build-command"));
        assert!(mem.contains("cargo build -p stitch"));
    }

    #[test]
    fn same_title_overwrites() {
        let dir = temp_dir();
        let tool = SaveMemory::new(dir.path());
        let a = serde_json::json!({"title": "t", "content": "第一版"});
        let b = serde_json::json!({"title": "t", "content": "第二版"});
        tokio::runtime::Runtime::new()
            .expect("rt")
            .block_on(tool.execute(a))
            .expect("a");
        tokio::runtime::Runtime::new()
            .expect("rt")
            .block_on(tool.execute(b))
            .expect("b");
        let mem = load_memory(dir.path().to_string_lossy().as_ref());
        assert!(mem.contains("第二版"));
        assert!(!mem.contains("第一版"), "同标题应覆盖：{mem}");
    }

    #[test]
    fn missing_args_fail() {
        let dir = temp_dir();
        let tool = SaveMemory::new(dir.path());
        let result = tokio::runtime::Runtime::new()
            .expect("rt")
            .block_on(tool.execute(serde_json::json!({"title": ""})))
            .expect("execute");
        assert!(!result.success);
    }
}
