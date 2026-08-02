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
        });

        if self.undo_stack.len() > MAX_HISTORY {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// Undo the most recent file operation.
    /// Returns a description of what was undone.
    pub fn undo(&mut self) -> anyhow::Result<String> {
        let snapshot = self
            .undo_stack
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Nothing to undo"))?;

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
        });

        // Restore the snapshot
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
            Ok(description) => Ok(ToolResult {
                metrics: None,
                success: true,
                output: description,
            }),
            Err(e) => Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!("Undo failed: {e}"),
            }),
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
            Ok(description) => Ok(ToolResult {
                metrics: None,
                success: true,
                output: description,
            }),
            Err(e) => Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!("Redo failed: {e}"),
            }),
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
}
