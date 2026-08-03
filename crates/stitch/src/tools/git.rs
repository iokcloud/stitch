//! Git information tools (`git_status`, `git_diff`).

use super::{ToolDef, ToolResult};
use std::path::PathBuf;

/// Cap unified diff text returned to the model (bytes, UTF-8 safe via chars take).
const MAX_DIFF_CHARS: usize = 40_000;

#[derive(Clone)]
pub struct GitStatus {
    work_dir: PathBuf,
}

impl GitStatus {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "git_status".into(),
            description: "Show the current git status including branch, staged/unstaged changes, \
                 and recent commits. Prefer this (then git_diff) before reading many files \
                 when reviewing changes."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let _ = arguments;

        let mut output = String::new();

        // Git branch
        if let Ok(branch) = run_git(&self.work_dir, &["branch", "--show-current"]).await {
            output.push_str(&format!("Branch: {branch}"));
        }

        // Git status (short)
        if let Ok(status) = run_git(&self.work_dir, &["status", "--short"]).await {
            if status.is_empty() {
                output.push_str("\nWorking tree clean");
            } else {
                output.push_str(&format!("\n\nChanges:\n{status}"));
            }
        }

        // Recent commits
        if let Ok(log) = run_git(&self.work_dir, &["log", "--oneline", "-5"]).await
            && !log.is_empty()
        {
            output.push_str(&format!("\n\nRecent commits:\n{log}"));
        }

        if output.is_empty() {
            output = "Not a git repository (or git not installed).".into();
        }

        Ok(ToolResult {
            metrics: None,
            success: true,
            output,
        })
    }
}

/// Show unstaged/staged diff (optionally scoped to one path).
#[derive(Clone)]
pub struct GitDiff {
    work_dir: PathBuf,
}

impl GitDiff {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "git_diff".into(),
            description: "Show git diff for the working tree (and optionally staged). \
                 Use after git_status when reviewing or explaining changes. \
                 Prefer this over reading many unrelated files."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Optional path relative to repo root to scope the diff"
                    },
                    "staged": {
                        "type": "boolean",
                        "description": "If true, show staged diff only (--cached). Default false."
                    }
                }
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let staged = arguments
            .get("staged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let path = arguments
            .get("path")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let mut args: Vec<&str> = vec!["diff"];
        if staged {
            args.push("--cached");
        }
        // Prefer a compact stat first so the model sees the file list even when
        // the full patch is truncated.
        let mut stat_args = args.clone();
        stat_args.push("--stat");
        if let Some(p) = path {
            stat_args.push("--");
            stat_args.push(p);
        }

        let mut patch_args = args.clone();
        if let Some(p) = path {
            patch_args.push("--");
            patch_args.push(p);
        }

        let stat = run_git(&self.work_dir, &stat_args)
            .await
            .unwrap_or_default();
        let patch = run_git(&self.work_dir, &patch_args)
            .await
            .unwrap_or_default();

        if stat.is_empty() && patch.is_empty() {
            return Ok(ToolResult::ok(if staged {
                "No staged changes."
            } else {
                "No unstaged changes (working tree matches HEAD for this scope). \
                     Try git_status; untracked files need to be read directly."
            }));
        }

        let mut output = String::new();
        if !stat.is_empty() {
            output.push_str("## Diff stat\n");
            output.push_str(&stat);
            output.push_str("\n\n");
        }
        if !patch.is_empty() {
            output.push_str("## Diff\n");
            output.push_str(&truncate_chars(&patch, MAX_DIFF_CHARS));
        }

        Ok(ToolResult {
            metrics: None,
            success: true,
            output,
        })
    }
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}\n\n... [diff truncated at {max_chars} chars, total {count}]")
    }
}

async fn run_git(work_dir: &PathBuf, args: &[&str]) -> Result<String, ()> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args).current_dir(work_dir);
    super::process_win::hide_console(&mut cmd);
    let output = cmd.output().await.map_err(|_| ())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn truncate_chars_preserves_cjk() {
        let s = "改动审查".repeat(20);
        let out = truncate_chars(&s, 10);
        assert!(out.contains("truncated"));
        assert!(out.is_char_boundary(out.find('\n').unwrap_or(out.len())));
    }
}
