//! Targeted file editing tool.
//!
//! Supports precise text replacement in files — find `old_text` and
//! replace with `new_text`. Multiple edits can be applied atomically.

use super::paths::resolve_under_work_dir;
use super::{ToolDef, ToolResult};
use std::path::PathBuf;

/// Max number of edits in a single call to prevent abuse.
const MAX_EDITS_PER_CALL: usize = 20;

/// How many surrounding lines to show in the preview on success.
const CONTEXT_LINES: usize = 2;

#[derive(Clone)]
pub struct EditFile {
    work_dir: PathBuf,
}

impl Default for EditFile {
    fn default() -> Self {
        Self::new(".")
    }
}

impl EditFile {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "edit_file".into(),
            description: "Make targeted edits to a file by replacing specific text. \
                 Each edit specifies old_text (to find) and new_text (replacement). \
                 Prefer this over write_file for surgical changes — it's safer and \
                 preserves the rest of the file unchanged. Requires user confirmation."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file, relative to the working directory"
                    },
                    "edits": {
                        "type": "array",
                        "description": "List of edit operations to apply sequentially",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_text": {
                                    "type": "string",
                                    "description": "The exact text to find and replace"
                                },
                                "new_text": {
                                    "type": "string",
                                    "description": "The text to replace it with"
                                }
                            },
                            "required": ["old_text", "new_text"]
                        }
                    }
                },
                "required": ["path", "edits"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let raw_path = arguments["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let edits = arguments["edits"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing 'edits' array"))?;

        if edits.is_empty() {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: "No edits provided.".into(),
            });
        }

        if edits.len() > MAX_EDITS_PER_CALL {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!(
                    "Too many edits ({}) — maximum is {MAX_EDITS_PER_CALL} per call.",
                    edits.len()
                ),
            });
        }

        let full_path = resolve_under_work_dir(&self.work_dir, raw_path)?;

        // Read original content
        let original = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    metrics: None,
                    success: false,
                    output: format!("Cannot read {raw_path}: {e}"),
                });
            }
        };

        let mut content = original.clone();
        let mut applied = 0;
        let mut report_lines: Vec<String> = Vec::new();

        for (i, edit) in edits.iter().enumerate() {
            let old_text = edit["old_text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Edit {i}: missing 'old_text'"))?;
            let new_text = edit["new_text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Edit {i}: missing 'new_text'"))?;

            if old_text == new_text {
                report_lines.push(format!(
                    "  Edit {i}: skipped (old_text == new_text, no change)"
                ));
                continue;
            }

            // Count occurrences
            let count = content.matches(old_text).count();

            if count == 0 {
                return Ok(ToolResult {
                    metrics: None,
                    success: false,
                    output: format!(
                        "Edit {i} failed: 'old_text' not found in {raw_path}.\n\
                         The file may have changed since you last read it. \
                         Re-read the file and try again."
                    ),
                });
            }

            if count > 1 {
                return Ok(ToolResult {
                    metrics: None,
                    success: false,
                    output: format!(
                        "Edit {i} failed: 'old_text' matched {count} locations. \
                         Make old_text more specific (include more surrounding context) \
                         so it matches exactly one location."
                    ),
                });
            }

            // Apply the replacement
            content = content.replacen(old_text, new_text, 1);
            applied += 1;

            // Generate a brief preview
            let preview = preview_change(&original, old_text, new_text);
            report_lines.push(format!("  Edit {i}: {preview}"));
        }

        if applied == 0 {
            return Ok(ToolResult {
                metrics: None,
                success: true,
                output: format!("No changes applied to {raw_path} (all edits were no-ops)."),
            });
        }

        // Snapshot for undo before overwriting
        super::undo::snapshot(&full_path);

        // Write back
        tokio::fs::write(&full_path, &content).await?;

        let added = content.len() as i64 - original.len() as i64;
        let delta = if added >= 0 {
            format!("+{added}")
        } else {
            added.to_string()
        };

        Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!(
                "Applied {applied} edit(s) to {raw_path} ({delta} bytes):\n{}",
                report_lines.join("\n")
            ),
        })
    }
}

/// Generate a short preview of the change.
fn preview_change(_original: &str, old: &str, new: &str) -> String {
    let old_short = summarize_text(old, 40);
    let new_short = summarize_text(new, 40);

    if old_short == new_short {
        return format!("replaced \"{old_short}\" (no visible change)");
    }

    // Show what changed
    if old.is_empty() {
        format!("inserted \"{new_short}\"")
    } else if new.is_empty() {
        format!("deleted \"{old_short}\"")
    } else {
        // Show first differing character range
        let common_prefix = old
            .chars()
            .zip(new.chars())
            .take_while(|(a, b)| a == b)
            .count();
        let old_tail: String = old
            .chars()
            .skip(common_prefix)
            .take(CONTEXT_LINES * 10)
            .collect();
        let new_tail: String = new
            .chars()
            .skip(common_prefix)
            .take(CONTEXT_LINES * 10)
            .collect();
        format!(
            "\"{}\" → \"{}\"",
            truncate(&old_tail, 30),
            truncate(&new_tail, 30)
        )
    }
}

fn summarize_text(s: &str, max: usize) -> String {
    let s = s.replace('\n', "\\n").replace('\t', "\\t");
    truncate(&s, max)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}
