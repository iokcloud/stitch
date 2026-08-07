//! File-finding tool — glob-based path search.
//!
//! Walks the working directory matching files against a glob pattern.
//! Results are sorted alphabetically, paginated at 50 per page.

use super::{ToolDef, ToolResult};
use std::path::PathBuf;

/// Max number of results before truncation.
const MAX_RESULTS: usize = 200;

/// Directories to skip during traversal.
const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
    "dist",
    "build",
    "out",
];

#[derive(Clone)]
pub struct FindPath {
    work_dir: PathBuf,
}

impl Default for FindPath {
    fn default() -> Self {
        Self::new(".")
    }
}

impl FindPath {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "find_path".into(),
            description: "Find file paths matching a glob pattern. \
                 Returns sorted file paths (paginated, 50 per page). \
                 Use this to locate files by name patterns like **/*.rs or src/**/*.ts. \
                 Supports ** for recursive matching. \
                 Skips build artifacts and dependency directories automatically."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "glob": {
                        "type": "string",
                        "description": "A glob pattern to match against file paths. \
                            Supports ** for recursive matching. Examples: **/*.rs, src/**/*.ts, *.toml"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Optional starting position for paginated results (0-based). Default: 0",
                        "default": 0
                    }
                },
                "required": ["glob"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let pattern = arguments["glob"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'glob' argument"))?;
        let offset = arguments["offset"].as_u64().unwrap_or(0) as usize;

        let mut results: Vec<String> = Vec::new();
        // .stitchignore：被忽略的目录不递归、文件不收录
        let ignore = super::ignore::IgnoreRules::load(&self.work_dir);
        walk_and_match(
            &self.work_dir,
            &self.work_dir,
            pattern,
            &mut results,
            0,
            &ignore,
        );

        results.sort();

        let total = results.len();

        // Paginate: skip offset, take up to 50
        let page: Vec<&String> = results.iter().skip(offset).take(50).collect();

        if page.is_empty() && total > 0 {
            return Ok(ToolResult::ok(format!(
                "Offset {offset} exceeds total results ({total}). No more results."
            )));
        }

        let output_lines: Vec<&str> = page.iter().map(|s| s.as_str()).collect();
        let mut output = output_lines.join("\n");

        if total > offset + 50 {
            output.push_str(&format!(
                "\n\n... {}/{} results shown (use offset={} for next page)",
                (offset + 50).min(total),
                total,
                offset + 50
            ));
        } else if total > MAX_RESULTS {
            output.push_str(&format!("\n\n... truncated at {MAX_RESULTS} results"));
        }

        if output.is_empty() {
            output = format!("No files found matching \"{pattern}\"");
        }

        Ok(ToolResult {
            metrics: None,
            success: true,
            output,
        })
    }
}

fn walk_and_match(
    base: &PathBuf,
    dir: &PathBuf,
    pattern: &str,
    results: &mut Vec<String>,
    depth: usize,
    ignore: &super::ignore::IgnoreRules,
) {
    if depth > 20 || results.len() >= MAX_RESULTS {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        if results.len() >= MAX_RESULTS {
            return;
        }

        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip hidden and noise dirs
        if file_name.starts_with('.') || SKIP_DIRS.contains(&file_name) {
            continue;
        }

        let rel = path
            .strip_prefix(base)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        // .stitchignore：被忽略的目录不递归、文件不收录
        if ignore.is_ignored(&rel) {
            continue;
        }

        if path.is_dir() {
            walk_and_match(base, &path, pattern, results, depth + 1, ignore);
        } else if glob_match(pattern, &rel) {
            results.push(rel);
        }
    }
}

/// Simple glob matching — supports * (any chars except /), ** (any chars including /), ? (single char).
/// pub(crate)：.stitchignore 规则复用同一匹配器。
pub(crate) fn glob_match(pattern: &str, path: &str) -> bool {
    // Strip leading ./ if present
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);

    glob_match_inner(pattern, path)
}

fn glob_match_inner(pat: &str, s: &str) -> bool {
    if pat.is_empty() {
        return s.is_empty();
    }

    let p_bytes = pat.as_bytes();
    let s_bytes = s.as_bytes();

    // Handle **/
    if let Some(rest) = pat.strip_prefix("**/") {
        // **/ matches zero or more directories
        if glob_match_inner(rest, s) {
            return true;
        }
        // Try consuming one path segment
        if let Some(slash_pos) = s.find('/') {
            return glob_match_inner(pat, &s[slash_pos + 1..]);
        }
        // Last segment — match against rest
        return glob_match_inner(rest, s);
    }

    if s.is_empty() {
        return pat.is_empty() || pat == "**";
    }

    match p_bytes[0] {
        b'*' => {
            let rest = &pat[1..];
            // * can match zero or more of any character except /
            if !rest.is_empty() && rest.as_bytes()[0] == b'*' {
                // ** handled above
                return false;
            }
            for i in 0..=s.len() {
                if i == s.len() || s_bytes[i] != b'/' {
                    // Try matching rest starting at position i
                    if glob_match_inner(rest, &s[i..]) {
                        return true;
                    }
                }
                // Don't cross path separator for single *
                if i < s.len() && s_bytes[i] == b'/' {
                    break;
                }
            }
            false
        }
        b'?' => {
            // ? matches any single character except /
            s_bytes[0] != b'/' && glob_match_inner(&pat[1..], &s[1..])
        }
        _ => {
            // Literal character match
            if p_bytes[0] == s_bytes[0] {
                glob_match_inner(&pat[1..], &s[1..])
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match() {
        // Basic
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.py"));
        assert!(glob_match("src/**/*.rs", "src/tools/mod.rs"));
        assert!(glob_match("src/**/*.rs", "src/mod.rs"));
        assert!(!glob_match("src/**/*.rs", "tests/mod.rs"));
        // Single char
        assert!(glob_match("file.??", "file.rs"));
        assert!(!glob_match("file.??", "file.pyc"));
        // Mixed
        assert!(glob_match("**/test*.rs", "crates/stitch/tests/test_mod.rs"));
    }
}
