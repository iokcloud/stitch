//! 会话级启动参数（--setting / --model-config / --include）。
//!
//! 三类 Claude Code 语义的 CLI 能力，统一在启动时解析一次：
//! - `--setting <KEY>=<VALUE>`：快速配置覆盖（permission_mode /
//!   disallowed_tools / append_system_prompt / statusline / model），
//!   优先级高于 config 与 settings.json，不落盘，仅本次会话。
//! - `--model-config <FILE>`：JSON 模型参数（temperature / top_p /
//!   max_tokens），请求构造时合并进采样参数。
//! - `--include <PATH>`：附加文件内容注入系统提示末尾（模型始终可见）。

use std::sync::Mutex;

/// 模型采样参数覆盖（--model-config JSON）。
#[derive(Debug, Clone, Default)]
pub struct ModelOverrides {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<usize>,
}

impl ModelOverrides {
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("读取模型配置失败 {}: {e}", path.display()))?;
        #[derive(serde::Deserialize)]
        struct Raw {
            temperature: Option<f32>,
            top_p: Option<f32>,
            max_tokens: Option<usize>,
        }
        let raw: Raw = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("模型配置 JSON 解析失败 {}: {e}", path.display()))?;
        let over = Self {
            temperature: raw.temperature,
            top_p: raw.top_p,
            max_tokens: raw.max_tokens,
        };
        if over.temperature.is_none() && over.top_p.is_none() && over.max_tokens.is_none() {
            anyhow::bail!(
                "模型配置 {} 无可用键（支持 temperature / top_p / max_tokens）",
                path.display()
            );
        }
        if let Some(t) = over.temperature
            && !(0.0..=2.0).contains(&t)
        {
            anyhow::bail!("temperature 超出范围 0.0–2.0: {t}");
        }
        if let Some(p) = over.top_p
            && !(0.0..=1.0).contains(&p)
        {
            anyhow::bail!("top_p 超出范围 0.0–1.0: {p}");
        }
        Ok(over)
    }
}

static MODEL_OVERRIDES: Mutex<Option<ModelOverrides>> = Mutex::new(None);

/// 设置模型参数覆盖（--model-config）。
pub fn set_model_overrides(o: ModelOverrides) {
    if let Ok(mut guard) = MODEL_OVERRIDES.lock() {
        *guard = Some(o);
    }
}

/// 读取当前模型参数覆盖。
pub fn model_overrides() -> Option<ModelOverrides> {
    MODEL_OVERRIDES.lock().ok().and_then(|g| g.clone())
}

/// 解析 `KEY=VALUE` 列表为会话级设置（无效键报错）。
///
/// 返回 (permission_mode, 追加的 deny 工具, 追加的 system prompt, statusline, model)。
#[allow(clippy::type_complexity)]
pub fn parse_settings(
    kv_list: &[String],
) -> anyhow::Result<(
    Option<String>,
    Vec<String>,
    Vec<String>,
    Option<String>,
    Option<String>,
)> {
    let mut mode = None;
    let mut deny = Vec::new();
    let mut prompts = Vec::new();
    let mut statusline = None;
    let mut model = None;
    for kv in kv_list {
        let (key, value) = kv
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--setting 需要 KEY=VALUE 格式，得到 `{kv}`"))?;
        let key = key.trim();
        let value = value.trim();
        match key {
            "permission_mode" => {
                mode = Some(value.to_string());
            }
            "disallowed_tools" => {
                for t in value.split(',') {
                    let t = t.trim();
                    if !t.is_empty() && !deny.iter().any(|d| d == t) {
                        deny.push(t.to_string());
                    }
                }
            }
            "append_system_prompt" => {
                if !prompts.iter().any(|p| p == value) {
                    prompts.push(value.to_string());
                }
            }
            "statusline" => statusline = Some(value.to_string()),
            "model" => model = Some(value.to_string()),
            other => anyhow::bail!(
                "不支持的 --setting 键 `{other}`（支持 permission_mode / disallowed_tools / append_system_prompt / statusline / model）"
            ),
        }
    }
    Ok((mode, deny, prompts, statusline, model))
}

/// 读取 --include 文件内容（相对工作目录解析），失败报错带路径。
pub fn load_includes(
    paths: &[std::path::PathBuf],
    cwd: &std::path::Path,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for p in paths {
        let full = if p.is_absolute() {
            p.clone()
        } else {
            cwd.join(p)
        };
        let content = std::fs::read_to_string(&full)
            .map_err(|e| anyhow::anyhow!("读取 --include 文件失败 {}: {e}", full.display()))?;
        let rel = full
            .strip_prefix(cwd)
            .map(|r| r.display().to_string())
            .unwrap_or_else(|_| full.display().to_string());
        out.push((rel, content));
    }
    Ok(out)
}

/// 把 --include 内容渲染为系统提示块（Claude Code 语义：始终可见）。
pub fn render_includes(includes: &[(String, String)]) -> String {
    if includes.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\n## Included Files（始终可见）\n");
    for (rel, content) in includes {
        s.push_str(&format!("\n### {rel}\n```\n{content}\n```\n"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_settings_full_keys() {
        let (mode, deny, prompts, statusline, model) = parse_settings(&[
            "permission_mode=accept_edits".into(),
            "disallowed_tools=run_command, write_file".into(),
            "append_system_prompt=你是资深工程师".into(),
            "statusline=git branch --show-current".into(),
            "model=deepseek-v4-flash".into(),
        ])
        .unwrap();
        assert_eq!(mode.as_deref(), Some("accept_edits"));
        assert_eq!(deny, vec!["run_command", "write_file"]);
        assert_eq!(prompts, vec!["你是资深工程师"]);
        assert_eq!(statusline.as_deref(), Some("git branch --show-current"));
        assert_eq!(model.as_deref(), Some("deepseek-v4-flash"));
    }

    #[test]
    fn parse_settings_requires_equals() {
        assert!(parse_settings(&["permission_mode".into()]).is_err());
    }

    #[test]
    fn parse_settings_rejects_unknown_key() {
        assert!(parse_settings(&["theme=dark".into()]).is_err());
    }

    #[test]
    fn parse_settings_dedups() {
        let (_, _, prompts, _, _) = parse_settings(&[
            "append_system_prompt=a".into(),
            "append_system_prompt=a".into(),
        ])
        .unwrap();
        assert_eq!(prompts, vec!["a"]);
    }

    #[test]
    fn model_config_validates_range() {
        let dir = std::env::temp_dir().join(format!("stitch-mc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("mc.json");
        std::fs::write(&f, r#"{"temperature": 0.7, "max_tokens": 2048}"#).unwrap();
        let o = ModelOverrides::from_file(&f).unwrap();
        assert_eq!(o.temperature, Some(0.7));
        assert_eq!(o.max_tokens, Some(2048));
        assert_eq!(o.top_p, None);
        // 越界拒绝
        std::fs::write(&f, r#"{"temperature": 3.0}"#).unwrap();
        assert!(ModelOverrides::from_file(&f).is_err());
        // 无可用键拒绝
        std::fs::write(&f, r#"{"frequency_penalty": 0.5}"#).unwrap();
        assert!(ModelOverrides::from_file(&f).is_err());
        // JSON 损坏拒绝
        std::fs::write(&f, "{broken").unwrap();
        assert!(ModelOverrides::from_file(&f).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn includes_render_and_load() {
        let dir = std::env::temp_dir().join(format!("stitch-inc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "hello").unwrap();
        let incs = load_includes(&["a.txt".into()], &dir).unwrap();
        assert_eq!(incs.len(), 1);
        assert_eq!(incs[0].1, "hello");
        let s = render_includes(&incs);
        assert!(s.contains("## Included Files"));
        assert!(s.contains("a.txt"));
        assert!(s.contains("hello"));
        // 缺失文件报错
        assert!(load_includes(&["missing.txt".into()], &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
