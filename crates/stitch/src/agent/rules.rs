//! Project and user rules system.
//!
//! Stitch loads rules from two locations and injects them into the
//! system prompt:
//!
//! 1. `~/.stitchrules` — global user rules (applied to all projects)
//! 2. `<project>/.stitchrules` — project-specific rules
//!
//! Rules files are plain text. Both files are loaded and concatenated
//! into a single rules block appended to the system prompt.
//!
//! ## Format
//!
//! No special format is required. Write natural-language rules,
//! one per line or paragraph. Example:
//!
//! ```text
//! Always use async/await for I/O operations.
//! Prefer Result<T, Error> over panicking.
//! Run cargo fmt before committing.
//! ```

use std::path::PathBuf;

/// Maximum size of a rules file to load (to prevent loading huge files).
const MAX_RULES_SIZE: u64 = 64 * 1024; // 64 KB

/// Load and merge all applicable rules for the given working directory.
///
/// Returns `None` if no rules files exist or they're all empty.
pub fn load_rules(work_dir: &str) -> Option<String> {
    let mut combined = String::new();

    // 1. Global user rules
    if let Some(home_rules) = load_rules_file(&home_rules_path()) {
        combined.push_str("## Global Rules\n\n");
        combined.push_str(&home_rules);
        combined.push_str("\n\n");
    }

    // 2. Project-local rules
    let project_path = PathBuf::from(work_dir).join(".stitchrules");
    if let Some(project_rules) = load_rules_file(&project_path) {
        combined.push_str("## Project Rules\n\n");
        combined.push_str(&project_rules);
        combined.push('\n');
    }

    let trimmed = combined.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Resolve the path to the global rules file: `~/.stitchrules`.
fn home_rules_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        // On Windows, use USERPROFILE or HOMEDRIVE+HOMEPATH
        let home = std::env::var("USERPROFILE")
            .or_else(|_| {
                let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
                let path = std::env::var("HOMEPATH").unwrap_or_default();
                Ok::<String, std::env::VarError>(format!("{drive}{path}"))
            })
            .unwrap_or_default();
        PathBuf::from(home).join(".stitchrules")
    }

    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join(".stitchrules")
    }
}

/// Load a single rules file, returning its content if it exists and is valid.
fn load_rules_file(path: &PathBuf) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;

    // Skip directories and empty files
    if !meta.is_file() || meta.len() == 0 {
        return None;
    }

    // Reject overly large files
    if meta.len() > MAX_RULES_SIZE {
        tracing::warn!(
            path = %path.display(),
            size = meta.len(),
            max = MAX_RULES_SIZE,
            "rules file too large, skipping"
        );
        return None;
    }

    match std::fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                tracing::info!(path = %path.display(), "loaded rules file");
                Some(trimmed)
            }
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), %e, "failed to read rules file");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_rules_file(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".stitchrules");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        (dir, path)
    }

    #[test]
    fn load_valid_rules() {
        let (_dir, path) = temp_rules_file("Use tabs, not spaces.\nRun tests before commit.\n");
        let result = load_rules_file(&path);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Use tabs"));
    }

    #[test]
    fn skip_empty_file() {
        let (_dir, path) = temp_rules_file("");
        assert!(load_rules_file(&path).is_none());
    }

    #[test]
    fn skip_whitespace_only() {
        let (_dir, path) = temp_rules_file("   \n  \n  ");
        assert!(load_rules_file(&path).is_none());
    }

    #[test]
    fn skip_nonexistent_file() {
        let path = PathBuf::from("/nonexistent/path/.stitchrules");
        assert!(load_rules_file(&path).is_none());
    }

    #[test]
    fn load_rules_from_project() {
        let dir = tempfile::tempdir().unwrap();
        let rules_path = dir.path().join(".stitchrules");
        let mut f = std::fs::File::create(&rules_path).unwrap();
        f.write_all(b"Always use async/await.\n").unwrap();

        let result = load_rules(&dir.path().display().to_string());
        assert!(result.is_some());
        let rules = result.unwrap();
        assert!(rules.contains("Always use async/await"));
        assert!(rules.contains("## Project Rules"));
    }
}
