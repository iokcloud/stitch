//! Undo/redo system for file operations.
//!
//! Maintains an in-memory snapshot stack per file so the agent
//! can undo destructive operations like write, edit, or delete.
//!
//! ## Usage
//!
//! - `undo_snapshot(path)` — save file content before modifying
//! - `undo_last_edit()` — revert most recent change (tool-callable)
//! - `redo_last_edit()` — reapply last undone change (tool-callable)
//!
//! The snapshot history is limited to prevent unbounded memory use.

use super::{ToolDef, ToolResult};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Maximum undo history per file.
const MAX_HISTORY: usize = 50;

/// A snapshot of file content at a point in time.
#[derive(Debug, Clone)]
struct Snapshot {
    /// Original content of the file.
    content: Option<String>, // None means the file didn't exist
    /// True if this was a deletion.
    was_deleted: bool,
    /// Absolute path for restoration.
    path: PathBuf,
    /// 回合边界标记（/rewind 用）：不参与普通 undo。
    marker: bool,
}

/// 回合标记的占位路径（仅内部使用，绝不作为真实文件路径匹配）。
fn marker_path() -> PathBuf {
    PathBuf::from("__stitch_turn_marker__")
}

/// Manages undo/redo stacks for file operations.
#[derive(Default)]
pub struct UndoManager {
    /// Per-file undo stack (most recent first).
    undo_stack: Vec<Snapshot>,
    /// Per-file redo stack.
    redo_stack: Vec<Snapshot>,
}

impl UndoManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Save a snapshot of a file before modifying it.
    pub fn snapshot(&mut self, path: &Path) {
        let content = if path.exists() && path.is_file() {
            std::fs::read_to_string(path).ok()
        } else {
            None
        };

        let was_deleted = false;

        self.undo_stack.push(Snapshot {
            content,
            was_deleted,
            path: path.to_path_buf(),
            marker: false,
        });

        // Limit history
        if self.undo_stack.len() > MAX_HISTORY {
            self.undo_stack.remove(0);
        }

        // Clear redo stack on new action
        self.redo_stack.clear();
    }

    /// Record that a file was deleted (for undo purposes).
    pub fn snapshot_delete(&mut self, path: &Path) {
        let content = std::fs::read_to_string(path).ok();

        self.undo_stack.push(Snapshot {
            content,
            was_deleted: true,
            path: path.to_path_buf(),
            marker: false,
        });

        if self.undo_stack.len() > MAX_HISTORY {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// 回合开始前压入边界标记（/rewind 用）。
    ///
    /// marker 不参与普通 undo；`undo_until_marker` 弹栈到最近的 marker
    /// 为止。受 MAX_HISTORY 约束（回合数远超 50 时最老的边界会被挤掉）。
    pub fn push_turn_marker(&mut self) {
        self.undo_stack.push(Snapshot {
            content: None,
            was_deleted: false,
            path: marker_path(),
            marker: true,
        });
        if self.undo_stack.len() > MAX_HISTORY {
            self.undo_stack.remove(0);
        }
        // 新回合即新动作：redo 栈作废
        self.redo_stack.clear();
    }

    /// 撤销到最近的回合边界（含边界本身），恢复该回合内所有文件改动。
    /// 返回每个恢复动作的描述。无边界时返回 Err。
    pub fn undo_until_marker(&mut self) -> anyhow::Result<Vec<String>> {
        let mut descriptions = Vec::new();
        while let Some(snapshot) = self.undo_stack.pop() {
            if snapshot.marker {
                return Ok(descriptions);
            }
            // 回合级回滚不进 redo 栈：redo 语义保持"单次工具操作"不变
            descriptions.push(restore_snapshot(&snapshot)?);
        }
        Err(anyhow::anyhow!("No turn boundary found"))
    }

    /// Undo the most recent file operation.
    /// Returns a description of what was undone.
    pub fn undo(&mut self) -> anyhow::Result<String> {
        // 跳过回合边界标记：工具级 undo 只回滚文件操作，不回滚回合
        let snapshot = loop {
            match self.undo_stack.pop() {
                Some(snap) if snap.marker => continue,
                Some(snap) => break snap,
                None => return Err(anyhow::anyhow!("Nothing to undo")),
            }
        };

        // Save current state for redo
        let current_content = if snapshot.path.exists() && snapshot.path.is_file() {
            std::fs::read_to_string(&snapshot.path).ok()
        } else {
            None
        };

        self.redo_stack.push(Snapshot {
            content: current_content,
            was_deleted: snapshot.was_deleted,
            path: snapshot.path.clone(),
            marker: false,
        });

        restore_snapshot(&snapshot)
    }

    /// Redo the most recently undone operation.
    pub fn redo(&mut self) -> anyhow::Result<String> {
        let snapshot = self
            .redo_stack
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Nothing to redo"))?;

        // Save current state for undo
        let current_content = if snapshot.path.exists() && snapshot.path.is_file() {
            std::fs::read_to_string(&snapshot.path).ok()
        } else {
            None
        };

        self.undo_stack.push(Snapshot {
            content: current_content,
            was_deleted: snapshot.was_deleted,
            path: snapshot.path.clone(),
            marker: false,
        });

        // Reapply the operation
        let description = match (&snapshot.content, snapshot.was_deleted) {
            (Some(content), _) => {
                if let Some(parent) = snapshot.path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&snapshot.path, content)?;
                format!("Redone changes to: {}", snapshot.path.display())
            }
            (None, _) => {
                if snapshot.path.is_dir() {
                    std::fs::remove_dir_all(&snapshot.path)?;
                } else if snapshot.path.is_file() {
                    std::fs::remove_file(&snapshot.path)?;
                }
                format!("Redone deletion of: {}", snapshot.path.display())
            }
        };

        Ok(description)
    }

    /// Check if there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Number of undoable operations.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }
}

/// 把快照恢复到文件系统，返回描述。undo 与回合级回滚共用。
fn restore_snapshot(snapshot: &Snapshot) -> anyhow::Result<String> {
    let description = match (&snapshot.content, snapshot.was_deleted) {
        (Some(content), true) => {
            // File was deleted — restore it
            if let Some(parent) = snapshot.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&snapshot.path, content)?;
            format!("Restored deleted file: {}", snapshot.path.display())
        }
        (Some(content), false) => {
            // File was modified — restore original content
            if let Some(parent) = snapshot.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&snapshot.path, content)?;
            format!("Reverted changes to: {}", snapshot.path.display())
        }
        (None, _) => {
            // File didn't exist before — delete it
            if snapshot.path.is_dir() {
                std::fs::remove_dir_all(&snapshot.path)?;
            } else if snapshot.path.is_file() {
                std::fs::remove_file(&snapshot.path)?;
            }
            format!("Removed created file: {}", snapshot.path.display())
        }
    };
    Ok(description)
}

/// Global undo manager instance.
static UNDO_MANAGER: std::sync::LazyLock<Mutex<UndoManager>> =
    std::sync::LazyLock::new(|| Mutex::new(UndoManager::new()));

/// Save a snapshot of a file before modifying it.
/// Called by write_file, edit_file, and delete_path tools.
pub fn snapshot(path: &Path) {
    if let Ok(mut mgr) = UNDO_MANAGER.lock() {
        mgr.snapshot(path);
    }
}

/// Record a file deletion for potential undo.
pub fn snapshot_delete(path: &Path) {
    if let Ok(mut mgr) = UNDO_MANAGER.lock() {
        mgr.snapshot_delete(path);
    }
}

/// Undo the most recent file operation.
pub fn undo_last() -> anyhow::Result<String> {
    UNDO_MANAGER
        .lock()
        .map_err(|e| anyhow::anyhow!("Undo manager lock error: {e}"))?
        .undo()
}

/// 回合开始前压入边界标记（/rewind 用）。CLI 每回合开始调用一次。
pub fn push_turn_marker() {
    if let Ok(mut mgr) = UNDO_MANAGER.lock() {
        mgr.push_turn_marker();
    }
}

/// 撤销到最近的回合边界，返回恢复描述列表（/rewind 用）。
pub fn undo_until_marker() -> anyhow::Result<Vec<String>> {
    UNDO_MANAGER
        .lock()
        .map_err(|e| anyhow::anyhow!("Undo manager lock error: {e}"))?
        .undo_until_marker()
}

/// Redo the most recently undone operation.
pub fn redo_last() -> anyhow::Result<String> {
    UNDO_MANAGER
        .lock()
        .map_err(|e| anyhow::anyhow!("Undo manager lock error: {e}"))?
        .redo()
}

// ── Tool implementations ────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct UndoLastEdit;

impl UndoLastEdit {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "undo_last_edit".into(),
            description: "Undo the most recent file operation (write, edit, or delete). \
                 Restores the file to its previous state. Use this when you made a \
                 mistake and want to revert it."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    pub async fn execute(&self, _arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        match undo_last() {
            Ok(description) => Ok(ToolResult::ok(description)),
            Err(e) => Ok(ToolResult::fail(format!("Undo failed: {e}"))),
        }
    }
}

#[derive(Clone, Default)]
pub struct RedoLastEdit;

impl RedoLastEdit {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "redo_last_edit".into(),
            description: "Redo the most recently undone file operation. \
                 Reapplies the change that was undone by undo_last_edit."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    pub async fn execute(&self, _arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        match redo_last() {
            Ok(description) => Ok(ToolResult::ok(description)),
            Err(e) => Ok(ToolResult::fail(format!("Redo failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_restores_file_content() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        // Create initial file
        std::fs::write(&file_path, "original content").unwrap();

        // Snapshot before modification
        UNDO_MANAGER.lock().unwrap().snapshot(&file_path);

        // Modify
        std::fs::write(&file_path, "modified content").unwrap();
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "modified content"
        );

        // Undo
        let result = UNDO_MANAGER.lock().unwrap().undo().unwrap();
        assert!(result.contains("Reverted changes"));
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "original content"
        );
    }

    #[test]
    fn redo_reapplies_undone_change() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("redo_test.txt");

        std::fs::write(&file_path, "v1").unwrap();

        let mut mgr = UndoManager::new();
        mgr.snapshot(&file_path);
        std::fs::write(&file_path, "v2").unwrap();

        mgr.undo().unwrap();
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "v1");

        mgr.redo().unwrap();
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "v2");
    }

    #[test]
    fn nothing_to_undo_returns_error() {
        let mut mgr = UndoManager::new();
        assert!(!mgr.can_undo());
        assert!(mgr.undo().is_err());
    }

    #[test]
    fn nothing_to_redo_returns_error() {
        let mut mgr = UndoManager::new();
        assert!(mgr.redo().is_err());
    }

    #[test]
    fn new_action_clears_redo_stack() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("clear_test.txt");

        std::fs::write(&file_path, "v1").unwrap();
        let mut mgr = UndoManager::new();

        mgr.snapshot(&file_path);
        std::fs::write(&file_path, "v2").unwrap();

        mgr.undo().unwrap(); // Now redo stack has one entry
        assert!(mgr.can_redo());

        // New snapshot clears redo
        mgr.snapshot(&file_path);
        assert!(!mgr.can_redo());
    }

    #[test]
    fn turn_marker_bounds_rewind() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("t.txt");
        std::fs::write(&f, "v1").unwrap();

        let mut mgr = UndoManager::new();

        // 回合 1 开始：压边界（与 CLI 每回合前 push 一致），一次改动 v1 → v2
        mgr.push_turn_marker();
        mgr.snapshot(&f);
        std::fs::write(&f, "v2").unwrap();

        // 回合 2 开始：压边界，一次改动 v2 → v3
        mgr.push_turn_marker();
        mgr.snapshot(&f);
        std::fs::write(&f, "v3").unwrap();

        // /rewind 一次 → 回到回合 2 前（v2）
        let descs = mgr.undo_until_marker().unwrap();
        assert_eq!(descs.len(), 1);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "v2");

        // 再 rewind → 回到回合 1 前（v1）
        let descs = mgr.undo_until_marker().unwrap();
        assert_eq!(descs.len(), 1);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "v1");

        // 没有边界了
        assert!(mgr.undo_until_marker().is_err());
    }

    #[test]
    fn turn_marker_rewind_skips_redo() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("r.txt");
        std::fs::write(&f, "v1").unwrap();

        let mut mgr = UndoManager::new();
        mgr.snapshot(&f);
        std::fs::write(&f, "v2").unwrap();

        mgr.push_turn_marker();
        mgr.snapshot(&f);
        std::fs::write(&f, "v3").unwrap();

        // 回合级回滚不产生 redo 条目
        mgr.undo_until_marker().unwrap();
        assert!(!mgr.can_redo());
    }

    #[test]
    fn tool_undo_skips_turn_marker() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("u.txt");
        std::fs::write(&f, "v1").unwrap();

        let mut mgr = UndoManager::new();
        mgr.push_turn_marker();
        mgr.snapshot(&f);
        std::fs::write(&f, "v2").unwrap();

        // 工具级 undo 跳过 marker，恢复到 v1
        let d = mgr.undo().unwrap();
        assert!(d.contains("Reverted"));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "v1");
        // marker 保留在栈上，/rewind 仍可用
        assert!(mgr.undo_stack.last().unwrap().marker);
    }
}
