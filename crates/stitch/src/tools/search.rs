//! Code search tool.
//!
//! Tries `rg` (ripgrep) first, falls back to `grep`, then to a built-in
//! basic file walker for cross-platform support.

use super::{ToolDef, ToolResult};
use std::path::PathBuf;
use std::process::Command;

/// Max output lines before truncation.
const MAX_OUTPUT_LINES: usize = 200;

/// Source file extensions to search (excludes binaries, large generated files).
const SOURCE_EXTS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "c", "cpp", "h", "hpp", "rb", "php",
    "swift", "kt", "scala", "sh", "bash", "toml", "yaml", "yml", "json", "md", "txt", "css",
    "html", "vue", "svelte", "sql", "env", "cfg", "ini", "csv",
];

#[derive(Clone)]
pub struct GrepSearch {
    work_dir: PathBuf,
}

impl GrepSearch {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "search_code".into(),
            description: "Search for a pattern in source files. \
                 Returns matching lines with file paths and line numbers. \
                 Use this to find function definitions, usages, or error patterns."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The substring or regex pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional subdirectory to search in, relative to working directory"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let pattern = arguments["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' argument"))?;
        // Outside-workspace search runs only with the gate-injected marker
        // (user-approved or matched allow rule); otherwise strictly under work_dir.
        let scoped = super::paths::scoped_allowed(&arguments);
        let search_dir = match arguments["path"].as_str() {
            Some(p) if !p.trim().is_empty() => {
                if scoped {
                    super::paths::resolve_scoped(&self.work_dir, p)?
                } else {
                    super::paths::resolve_under_work_dir(&self.work_dir, p)?
                }
            }
            _ => super::paths::resolve_under_work_dir(&self.work_dir, ".")?,
        };

        // Try ripgrep first (fastest)
        if let Ok(output) = run_rg(&search_dir, pattern) {
            return Ok(ToolResult {
                metrics: None,
                success: true,
                output,
            });
        }

        // Fall back to grep
        if let Ok(output) = run_grep(&search_dir, pattern) {
            return Ok(ToolResult {
                metrics: None,
                success: true,
                output,
            });
        }

        // Last resort: built-in basic search
        let output = basic_search(&search_dir, pattern);
        Ok(ToolResult {
            metrics: None,
            success: true,
            output,
        })
    }
}

fn run_rg(dir: &PathBuf, pattern: &str) -> Result<String, ()> {
    let mut cmd = Command::new("rg");
    cmd.args([
        "--line-number",
        "--no-heading",
        "--color=never",
        "-n",
        pattern,
    ])
    .arg(dir);
    super::process_win::hide_console_std(&mut cmd);
    let output = cmd.output().map_err(|_| ())?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(truncate_lines(&text, MAX_OUTPUT_LINES))
    } else {
        Err(())
    }
}

fn run_grep(dir: &PathBuf, pattern: &str) -> Result<String, ()> {
    let mut cmd = Command::new("grep");
    cmd.args(["-rn", "--color=never", pattern]).arg(dir);
    super::process_win::hide_console_std(&mut cmd);
    let output = cmd.output().map_err(|_| ())?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(truncate_lines(&text, MAX_OUTPUT_LINES))
    } else if output.status.code() == Some(1) {
        Ok("No matches found.".into())
    } else {
        Err(())
    }
}

/// Basic file search using only std — walks common source files, does substring matching.
fn basic_search(dir: &PathBuf, pattern: &str) -> String {
    let mut results: Vec<String> = Vec::new();
    walk_dir(dir, dir, pattern, &mut results, 0);
    if results.is_empty() {
        "No matches found.".into()
    } else {
        results.join("\n")
    }
}

fn walk_dir(base: &PathBuf, dir: &PathBuf, pattern: &str, results: &mut Vec<String>, depth: usize) {
    if depth > 15 || results.len() >= MAX_OUTPUT_LINES {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        if results.len() >= MAX_OUTPUT_LINES {
            return;
        }

        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip hidden files/dirs and common noise
        if file_name.starts_with('.')
            || file_name == "node_modules"
            || file_name == "target"
            || file_name == "__pycache__"
            || file_name == ".git"
        {
            continue;
        }

        if path.is_dir() {
            walk_dir(base, &path, pattern, results, depth + 1);
        } else if is_source_file(&path)
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            for (line_num, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    let rel = path.strip_prefix(base).unwrap_or(&path).display();
                    results.push(format!("{rel}:{}: {line}", line_num + 1));
                    if results.len() >= MAX_OUTPUT_LINES {
                        results.push(format!("... truncated at {MAX_OUTPUT_LINES} matches"));
                        return;
                    }
                }
            }
        }
    }
}

fn is_source_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| SOURCE_EXTS.contains(&ext))
}

fn truncate_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        text.to_string()
    } else {
        format!(
            "{}\n... truncated at {max_lines} lines ({} total)",
            lines[..max_lines].join("\n"),
            lines.len(),
        )
    }
}
