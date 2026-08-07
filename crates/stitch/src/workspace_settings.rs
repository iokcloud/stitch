//! 工作区级设置 `.stitch/settings.json`（对标 Claude Code `.claude/settings.json`）。
//!
//! 项目内覆盖全局 config.toml，优先级：CLI flag > .stitch/settings.json > config.toml。
//! 与 hooks.json 同目录（.stitch/ 已被 gitignore，本地私有、不随仓库分发；
//! 需要团队共享时自行 `git add -f`）。
//!
//! 支持字段：
//! - `permission_mode`：会话默认权限（default / accept_edits / plan / bypass）
//! - `disallowed_tools`：deny 工具列表（与全局取并集，始终生效）
//! - `append_system_prompt`：系统提示追加列表（与 CLI flag 合并）
//! - `statusline`：覆盖全局 statusline 命令
//! - `llm_model`：覆盖全局默认模型（CLI `--model` 仍优先）

use serde::Deserialize;

/// 工作区级设置（缺省即用全局 config）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkspaceSettings {
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
    #[serde(default)]
    pub append_system_prompt: Vec<String>,
    #[serde(default)]
    pub statusline: Option<String>,
    #[serde(default)]
    pub llm_model: Option<String>,
}

impl WorkspaceSettings {
    /// 从 `work_dir/.stitch/settings.json` 加载（缺失 / 解析失败 → 默认空）。
    pub fn load(work_dir: &str) -> Self {
        let path = std::path::Path::new(work_dir)
            .join(".stitch")
            .join("settings.json");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "bad .stitch/settings.json; using defaults");
                Self::default()
            }
        }
    }

    /// deny 工具并集：全局 config + 工作区 settings（保序去重）。
    pub fn merged_deny(cfg_deny: &[String], settings: &Self) -> Vec<String> {
        let mut out = cfg_deny.to_vec();
        for t in &settings.disallowed_tools {
            if !out.iter().any(|d| d == t) {
                out.push(t.clone());
            }
        }
        out
    }

    /// 模型解析：CLI `--model` > settings.llm_model > config.llm_model。
    pub fn resolve_model(model_override: Option<&str>, settings: &Self, cfg_model: &str) -> String {
        model_override
            .or(settings.llm_model.as_deref())
            .unwrap_or(cfg_model)
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("stitch-settings-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".stitch")).unwrap();
        d
    }

    #[test]
    fn missing_file_yields_defaults() {
        let r = WorkspaceSettings::load(tmp_dir("missing").to_str().unwrap());
        assert!(r.permission_mode.is_none());
        assert!(r.disallowed_tools.is_empty());
        assert!(r.statusline.is_none());
        assert!(r.llm_model.is_none());
    }

    #[test]
    fn parses_all_fields() {
        let d = tmp_dir("fields");
        std::fs::write(
            d.join(".stitch").join("settings.json"),
            r#"{
                "permission_mode": "plan",
                "disallowed_tools": ["run_command", "write_file"],
                "append_system_prompt": ["你是资深 Rust 工程师", "不要问确认"],
                "statusline": "echo hi",
                "llm_model": "deepseek-v4-flash"
            }"#,
        )
        .unwrap();
        let r = WorkspaceSettings::load(d.to_str().unwrap());
        assert_eq!(r.permission_mode.as_deref(), Some("plan"));
        assert_eq!(r.disallowed_tools, vec!["run_command", "write_file"]);
        assert_eq!(r.append_system_prompt.len(), 2);
        assert_eq!(r.statusline.as_deref(), Some("echo hi"));
        assert_eq!(r.llm_model.as_deref(), Some("deepseek-v4-flash"));
    }

    #[test]
    fn corrupt_json_yields_defaults() {
        let d = tmp_dir("corrupt");
        std::fs::write(d.join(".stitch").join("settings.json"), "not json").unwrap();
        let r = WorkspaceSettings::load(d.to_str().unwrap());
        assert!(r.permission_mode.is_none());
        assert!(r.disallowed_tools.is_empty());
    }

    #[test]
    fn merged_deny_dedups() {
        let s = WorkspaceSettings {
            disallowed_tools: vec!["write_file".into(), "run_command".into()],
            ..Default::default()
        };
        let merged = WorkspaceSettings::merged_deny(&["run_command".into()], &s);
        assert_eq!(merged, vec!["run_command", "write_file"]);
    }

    #[test]
    fn resolve_model_precedence() {
        let s = WorkspaceSettings {
            llm_model: Some("settings-model".into()),
            ..Default::default()
        };
        assert_eq!(
            WorkspaceSettings::resolve_model(None, &s, "cfg-model"),
            "settings-model"
        );
        assert_eq!(
            WorkspaceSettings::resolve_model(Some("flag-model"), &s, "cfg-model"),
            "flag-model"
        );
        assert_eq!(
            WorkspaceSettings::resolve_model(None, &WorkspaceSettings::default(), "cfg-model"),
            "cfg-model"
        );
    }
}
