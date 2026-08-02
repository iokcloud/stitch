//! Persisted allow rules — the「记住此规则」behind a confirm dialog.
//!
//! A rule authorizes `tool` calls whose scope value starts with `value`
//! (path or command prefix). Rules live in `allow_rules.json` next to
//! `config.toml`, are shared (Arc<Mutex<..>>) between the agent loop and
//! `respond_confirmation`, and are matched prefix-wise with a separator /
//! space boundary so a remembered rule never silently widens to a sibling
//! path or command.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Scope kinds persisted on rules.
pub const SCOPE_PATH: &str = "path";
pub const SCOPE_COMMAND: &str = "command";

/// Internal marker the agent gate injects into authorized outside-workspace
/// reads. Never part of any tool schema; scrubbed from incoming args before
/// the gate decides, so the model cannot self-authorize.
pub const SCOPED_MARKER: &str = "__stitch_scoped";

/// One remembered rule: allow `tool` calls whose scope starts with `value`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AllowRule {
    pub tool: String,
    /// `"path"` or `"command"`.
    #[serde(default = "default_scope")]
    pub scope: String,
    pub value: String,
}

fn default_scope() -> String {
    SCOPE_PATH.into()
}

/// Loaded rule set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AllowRules {
    #[serde(default)]
    pub rules: Vec<AllowRule>,
}

impl AllowRules {
    /// Standard path: `<config dir>/allow_rules.json` (next to config.toml).
    pub fn rules_path() -> PathBuf {
        crate::config::config_path()
            .parent()
            .map(|d| d.join("allow_rules.json"))
            .unwrap_or_else(|| PathBuf::from("allow_rules.json"))
    }

    pub fn load() -> Self {
        Self::load_from(&Self::rules_path())
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&content).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::rules_path())
    }

    pub fn save_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        tracing::info!(
            path = %path.display(),
            rules = self.rules.len(),
            "allow rules saved"
        );
        Ok(())
    }

    /// Normalize a remembered rule before persisting:
    /// - path scope → the containing directory (so「记住此规则」means「此目录内
    ///   同类操作自动允许」), trailing separators trimmed;
    /// - command scope → kept verbatim (trimmed);
    /// - empty / unknown-scope rules are rejected.
    pub fn normalize(rule: AllowRule) -> Option<AllowRule> {
        let tool = rule.tool.trim().to_string();
        let value = rule.value.trim().to_string();
        if tool.is_empty() || value.is_empty() {
            return None;
        }
        if rule.scope == SCOPE_COMMAND {
            return Some(AllowRule {
                tool,
                scope: SCOPE_COMMAND.into(),
                value,
            });
        }
        if rule.scope != SCOPE_PATH {
            return None;
        }
        let p = std::path::Path::new(&value);
        let ends_with_sep = value.ends_with('/') || value.ends_with('\\');
        let parent = if ends_with_sep {
            p
        } else {
            p.parent().unwrap_or(p)
        };
        let v = parent
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_string();
        if v.is_empty() {
            return None;
        }
        Some(AllowRule {
            tool,
            scope: SCOPE_PATH.into(),
            value: v,
        })
    }

    /// Insert a rule; returns whether it was actually new (dedup by
    /// tool + scope + value).
    pub fn add(&mut self, rule: AllowRule) -> bool {
        if self
            .rules
            .iter()
            .any(|r| r.tool == rule.tool && r.scope == rule.scope && r.value == rule.value)
        {
            return false;
        }
        self.rules.push(rule);
        true
    }

    /// Remove the rule matching `tool + scope + value` exactly (storage-value
    /// match — what the UI shows is what gets removed). Returns whether a
    /// rule was removed; order of the remaining rules is preserved.
    pub fn remove(&mut self, tool: &str, scope: &str, value: &str) -> bool {
        let before = self.rules.len();
        self.rules
            .retain(|r| !(r.tool == tool && r.scope == scope && r.value == value));
        self.rules.len() != before
    }

    /// Remove all rules.
    pub fn clear(&mut self) {
        self.rules.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Prefix match with a boundary, so `C:\work\src` never matches
    /// `C:\work\src2\…` and `git status` never matches `git status2`.
    pub fn matches(&self, tool: &str, scope: &str, value: &str) -> bool {
        if self.rules.is_empty() {
            return false;
        }
        self.rules
            .iter()
            .any(|r| r.tool == tool && r.scope == scope && prefix_with_boundary(value, &r.value))
    }
}

/// Case-insensitive prefix (Windows paths are case-folded in practice), with
/// a `/`, `\` or space boundary after the prefix (or an exact match).
fn prefix_with_boundary(value: &str, prefix: &str) -> bool {
    let v = value.to_lowercase();
    let p = prefix.to_lowercase();
    if v.len() < p.len() {
        return false;
    }
    if v == p {
        return true;
    }
    if !v.starts_with(&p) {
        return false;
    }
    matches!(
        v.as_bytes().get(p.len()),
        Some(b'/') | Some(b'\\') | Some(b' ')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(tool: &str, scope: &str, value: &str) -> AllowRule {
        AllowRule {
            tool: tool.into(),
            scope: scope.into(),
            value: value.into(),
        }
    }

    #[test]
    fn normalize_path_uses_parent_dir() {
        let r = AllowRules::normalize(rule("read_file", "path", "C:/work/src/main.rs")).unwrap();
        assert_eq!(r.scope, "path");
        assert_eq!(r.value, "C:/work/src");
    }

    #[test]
    fn normalize_path_keeps_directory_and_trims_separators() {
        let r = AllowRules::normalize(rule("read_file", "path", "C:/work/src/")).unwrap();
        assert_eq!(r.value, "C:/work/src");
        let r = AllowRules::normalize(rule("list_directory", "path", "src\\")).unwrap();
        assert_eq!(r.value, "src");
    }

    #[test]
    fn normalize_rejects_empty_or_unknown_scope() {
        assert!(AllowRules::normalize(rule("", "path", "x")).is_none());
        assert!(AllowRules::normalize(rule("read_file", "path", "  ")).is_none());
        assert!(AllowRules::normalize(rule("read_file", "url", "x")).is_none());
    }

    #[test]
    fn normalize_keeps_command_verbatim() {
        let r = AllowRules::normalize(rule("run_command", "command", "  git status  ")).unwrap();
        assert_eq!(r.scope, "command");
        assert_eq!(r.value, "git status");
    }

    #[test]
    fn matches_prefix_with_boundary() {
        let rules = AllowRules {
            rules: vec![
                rule("read_file", "path", "C:/work/src"),
                rule("run_command", "command", "git status"),
            ],
        };
        assert!(rules.matches("read_file", "path", "C:/work/src/main.rs"));
        assert!(rules.matches("read_file", "path", "C:/WORK/SRC"));
        assert!(!rules.matches("read_file", "path", "C:/work/src2/main.rs"));
        assert!(rules.matches("run_command", "command", "git status --short"));
        assert!(!rules.matches("run_command", "command", "git status2"));
        assert!(!rules.matches("write_file", "path", "C:/work/src/main.rs"));
    }

    #[test]
    fn add_dedups() {
        let mut rules = AllowRules::default();
        assert!(rules.add(rule("read_file", "path", "C:/a")));
        assert!(!rules.add(rule("read_file", "path", "C:/a")));
        assert!(rules.add(rule("read_file", "path", "C:/b")));
        assert_eq!(rules.rules.len(), 2);
    }

    #[test]
    fn load_save_roundtrip() {
        let path = std::env::temp_dir().join(format!("stitch-allow-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut rules = AllowRules::default();
        rules.add(rule("run_command", "command", "npm run build"));
        rules.save_to(&path).unwrap();
        let loaded = AllowRules::load_from(&path);
        assert_eq!(loaded.rules.len(), 1);
        assert_eq!(loaded.rules[0].value, "npm run build");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_missing_or_broken_file_is_empty() {
        let path =
            std::env::temp_dir().join(format!("stitch-allow-missing-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(AllowRules::load_from(&path).is_empty());
        std::fs::write(&path, "{not json").unwrap();
        assert!(AllowRules::load_from(&path).is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn remove_matches_exact_triple_path_and_command() {
        let mut rules = AllowRules::default();
        rules.add(AllowRule {
            tool: "read_file".into(),
            scope: "path".into(),
            value: "C:\\work\\src".into(),
        });
        rules.add(AllowRule {
            tool: "run_command".into(),
            scope: "command".into(),
            value: "npm run build".into(),
        });
        assert!(rules.remove("read_file", "path", "C:\\work\\src"));
        assert_eq!(rules.rules.len(), 1);
        assert_eq!(rules.rules[0].tool, "run_command");
        assert!(rules.remove("run_command", "command", "npm run build"));
        assert!(rules.is_empty());
    }

    #[test]
    fn remove_missing_triple_returns_false_unchanged() {
        let mut rules = AllowRules::default();
        rules.add(AllowRule {
            tool: "read_file".into(),
            scope: "path".into(),
            value: "C:\\work\\src".into(),
        });
        assert!(!rules.remove("read_file", "path", "C:\\other"));
        assert!(!rules.remove("read_file", "command", "C:\\work\\src"));
        assert!(!rules.remove("write_file", "path", "C:\\work\\src"));
        assert_eq!(rules.rules.len(), 1);
    }

    #[test]
    fn remove_does_not_touch_same_value_other_tool() {
        let mut rules = AllowRules::default();
        rules.add(AllowRule {
            tool: "read_file".into(),
            scope: "path".into(),
            value: "C:\\work\\src".into(),
        });
        rules.add(AllowRule {
            tool: "write_file".into(),
            scope: "path".into(),
            value: "C:\\work\\src".into(),
        });
        assert!(rules.remove("read_file", "path", "C:\\work\\src"));
        assert_eq!(rules.rules.len(), 1);
        assert_eq!(rules.rules[0].tool, "write_file");
    }

    #[test]
    fn clear_empties_all() {
        let mut rules = AllowRules::default();
        rules.add(AllowRule {
            tool: "a".into(),
            scope: "path".into(),
            value: "x".into(),
        });
        rules.add(AllowRule {
            tool: "b".into(),
            scope: "command".into(),
            value: "y".into(),
        });
        rules.clear();
        assert!(rules.is_empty());
        assert_eq!(rules.rules.len(), 0);
    }
}
