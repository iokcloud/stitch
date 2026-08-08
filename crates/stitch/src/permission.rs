//! 权限模式 + deny 规则（Claude Code 语义）。
//!
//! 四种权限模式：
//! - `default`：危险操作逐项确认（现状）
//! - `accept_edits`：write_file / edit_file 自动批准，其余照常确认
//! - `plan`：仅只读工具可用，其余直接拒绝
//! - `bypass`：跳过全部确认（`--dangerously-skip-permissions`）
//!
//! deny 规则（disallowedTools）与模式正交、**始终生效**：命中的工具直接
//! 拒绝，bypass 模式下也不放行（安全阀）。
//!
//! 配置来源（优先级）：`--permission-mode` CLI 参数 / `/permissions` slash
//! 命令（运行时）> config.json 的 `permission_mode` / `disallowed_tools`
//! （持久）> 默认 `default` / 空 deny 列表。
//!
//! 全局单例（与 undo.rs 的 UNDO_MANAGER 同模式）：CLI 启动时设置一次，
//! agent 层各挂点直接读取；desktop 不设置则保持 default。

use std::sync::Mutex;

/// 权限模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionMode {
    /// 危险操作逐项确认（现状）
    #[default]
    Default,
    /// 文件编辑自动批准（write_file / edit_file），其余确认
    AcceptEdits,
    /// 仅只读工具可用，其余拒绝
    Plan,
    /// 跳过全部确认（deny 规则除外）
    Bypass,
}

impl PermissionMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "default" => Some(Self::Default),
            "accept_edits" | "accept-edits" | "acceptedits" => Some(Self::AcceptEdits),
            "plan" => Some(Self::Plan),
            "bypass" | "bypass-permissions" | "dangerously-skip-permissions" => Some(Self::Bypass),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "accept_edits",
            Self::Plan => "plan",
            Self::Bypass => "bypass",
        }
    }
}

/// 会话级权限配置。
#[derive(Debug, Clone)]
pub struct PermissionConfig {
    pub mode: PermissionMode,
    /// deny 规则：工具名精确列表（始终生效，含 bypass）。
    pub deny_tools: Vec<String>,
    /// 白名单：非空时只允许列表内的工具（deny 仍优先）。
    pub allow_tools: Vec<String>,
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self {
            mode: PermissionMode::Default,
            deny_tools: Vec::new(),
            allow_tools: Vec::new(),
        }
    }
}

static PERMISSION: std::sync::LazyLock<Mutex<PermissionConfig>> =
    std::sync::LazyLock::new(|| Mutex::new(PermissionConfig::default()));

/// 设置会话级权限配置（CLI 启动 / slash 命令调用）。
pub fn set_config(cfg: PermissionConfig) {
    if let Ok(mut guard) = PERMISSION.lock() {
        *guard = cfg;
    }
}

/// 读取当前权限配置。
pub fn current() -> PermissionConfig {
    PERMISSION.lock().map(|g| g.clone()).unwrap_or_default()
}

/// 从 CLI 参数 + config 应用权限配置（CLI flag 优先于 config；无效值报错）。
/// `cli_deny`（--disallowed-tools）与 config 的 deny 列表取并集；
/// `cli_allow`（--allowed-tools）为白名单（非空即生效）。
pub fn apply_from_cli(
    flag: Option<&str>,
    cfg_mode: Option<&str>,
    cfg_deny: &[String],
    cli_deny: &[String],
    cli_allow: &[String],
) -> anyhow::Result<()> {
    let mode = match flag.or(cfg_mode) {
        Some(s) => PermissionMode::parse(s).ok_or_else(|| {
            anyhow::anyhow!("无效权限模式 `{s}`（可选：default / accept_edits / plan / bypass）")
        })?,
        None => PermissionMode::Default,
    };
    let mut deny_tools = cfg_deny.to_vec();
    for t in cli_deny {
        if !deny_tools.iter().any(|d| d == t) {
            deny_tools.push(t.clone());
        }
    }
    let allow_tools = cli_allow.to_vec();
    set_config(PermissionConfig {
        mode,
        deny_tools,
        allow_tools,
    });
    Ok(())
}

/// 一次工具调用的裁决结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 模式放行，直接执行
    Allow,
    /// 直接拒绝（deny 规则或 plan 模式写工具）
    Deny(String),
    /// 走现有确认流程
    Ask,
}

/// 只读工具（plan 模式白名单）。
fn is_read_only(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read_file"
            | "list_directory"
            | "find_path"
            | "search_code"
            | "git_diff"
            | "git_status"
            | "web_fetch"
    )
}

/// 编辑类工具（accept_edits 自动批准）。
fn is_edit(tool_name: &str) -> bool {
    matches!(tool_name, "write_file" | "edit_file")
}

/// 裁决一次工具调用：deny 规则优先（始终生效），其次白名单，再模式。
pub fn adjudicate(tool_name: &str) -> Verdict {
    let cfg = current();
    if cfg.deny_tools.iter().any(|d| d == tool_name) {
        return Verdict::Deny(format!("tool `{tool_name}` is disallowed"));
    }
    // 白名单非空：列表外拒绝，列表内直接放行（最小权限执行环境）。
    if !cfg.allow_tools.is_empty() {
        if cfg.allow_tools.iter().any(|a| a == tool_name) {
            return Verdict::Allow;
        }
        return Verdict::Deny(format!(
            "tool `{tool_name}` 不在白名单（--allowed-tools 只允许 {}）",
            cfg.allow_tools.join(", ")
        ));
    }
    match cfg.mode {
        PermissionMode::Bypass => Verdict::Allow,
        PermissionMode::AcceptEdits if is_edit(tool_name) => Verdict::Allow,
        PermissionMode::Plan if !is_read_only(tool_name) => {
            Verdict::Deny(format!("plan 模式只允许只读工具（`{tool_name}` 被拒绝）"))
        }
        // 只读工具在 plan 下直接放行
        PermissionMode::Plan => Verdict::Allow,
        _ => Verdict::Ask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全局单例测试必须串行（并行测试会互相覆盖 set_config）。
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn parse_modes() {
        let _g = lock();

        assert_eq!(
            PermissionMode::parse("default"),
            Some(PermissionMode::Default)
        );
        assert_eq!(
            PermissionMode::parse("accept_edits"),
            Some(PermissionMode::AcceptEdits)
        );
        assert_eq!(
            PermissionMode::parse("acceptEdits"),
            Some(PermissionMode::AcceptEdits)
        );
        assert_eq!(PermissionMode::parse("plan"), Some(PermissionMode::Plan));
        assert_eq!(
            PermissionMode::parse("bypass"),
            Some(PermissionMode::Bypass)
        );
        assert_eq!(
            PermissionMode::parse("dangerously-skip-permissions"),
            Some(PermissionMode::Bypass)
        );
        assert_eq!(PermissionMode::parse("bogus"), None);
        assert_eq!(PermissionMode::parse(""), None);
    }

    fn with_cfg(mode: PermissionMode, deny: &[&str]) {
        set_config(PermissionConfig {
            mode,
            deny_tools: deny.iter().map(|s| s.to_string()).collect(),
            allow_tools: Vec::new(),
        });
    }

    #[test]
    fn default_mode_asks_everything() {
        let _g = lock();
        with_cfg(PermissionMode::Default, &[]);
        assert_eq!(adjudicate("read_file"), Verdict::Ask);
        assert_eq!(adjudicate("write_file"), Verdict::Ask);
    }

    #[test]
    fn accept_edits_auto_approves_edits_only() {
        let _g = lock();
        with_cfg(PermissionMode::AcceptEdits, &[]);
        assert_eq!(adjudicate("write_file"), Verdict::Allow);
        assert_eq!(adjudicate("edit_file"), Verdict::Allow);
        assert_eq!(adjudicate("read_file"), Verdict::Ask);
        assert_eq!(adjudicate("run_command"), Verdict::Ask);
        // delete 不在 accept_edits 白名单（保持确认）
        assert_eq!(adjudicate("delete_path"), Verdict::Ask);
    }

    #[test]
    fn plan_allows_reads_denies_writes() {
        let _g = lock();
        with_cfg(PermissionMode::Plan, &[]);
        assert_eq!(adjudicate("read_file"), Verdict::Allow);
        assert_eq!(adjudicate("git_diff"), Verdict::Allow);
        assert!(matches!(adjudicate("write_file"), Verdict::Deny(_)));
        assert!(matches!(adjudicate("run_command"), Verdict::Deny(_)));
    }

    #[test]
    fn bypass_allows_everything() {
        let _g = lock();
        with_cfg(PermissionMode::Bypass, &[]);
        assert_eq!(adjudicate("read_file"), Verdict::Allow);
        assert_eq!(adjudicate("run_command"), Verdict::Allow);
    }

    #[test]
    fn deny_wins_over_every_mode() {
        let _g = lock();
        // deny 命中在所有模式下都拒绝（含 bypass）
        for mode in [
            PermissionMode::Default,
            PermissionMode::AcceptEdits,
            PermissionMode::Plan,
            PermissionMode::Bypass,
        ] {
            with_cfg(mode, &["run_command", "delete_path"]);
            assert!(matches!(adjudicate("run_command"), Verdict::Deny(_)));
            assert!(
                matches!(adjudicate("delete_path"), Verdict::Deny(_)),
                "{mode:?}"
            );
        }
        // 未命中 deny 的走模式裁决
        with_cfg(PermissionMode::Bypass, &["run_command"]);
        assert_eq!(adjudicate("write_file"), Verdict::Allow);
        with_cfg(PermissionMode::Default, &["run_command"]);
        assert_eq!(adjudicate("read_file"), Verdict::Ask);
    }

    #[test]
    fn apply_from_cli_merges_cli_deny() {
        let _g = lock();
        // config deny + CLI --disallowed-tools 并集；CLI flag 覆盖模式
        apply_from_cli(
            Some("bypass"),
            Some("default"),
            &["run_command".to_string()],
            &["write_file".to_string(), "run_command".to_string()],
            &[],
        )
        .unwrap();
        let pc = current();
        assert_eq!(pc.mode, PermissionMode::Bypass);
        assert_eq!(pc.deny_tools, vec!["run_command", "write_file"]);
        // CLI deny 在 bypass 下仍生效（安全阀）
        assert!(matches!(adjudicate("run_command"), Verdict::Deny(_)));
        assert!(matches!(adjudicate("write_file"), Verdict::Deny(_)));
        assert_eq!(adjudicate("read_file"), Verdict::Allow);
    }

    #[test]
    fn apply_from_cli_rejects_bad_mode() {
        let _g = lock();
        let r = apply_from_cli(Some("bogus"), None, &[], &[], &[]);
        assert!(r.is_err());
        // flag 优先：config 有效但 flag 无效 → 报错
        let r = apply_from_cli(Some("bogus"), Some("plan"), &[], &[], &[]);
        assert!(r.is_err());
        // 无 flag：config 生效
        apply_from_cli(None, Some("plan"), &[], &[], &[]).unwrap();
        assert_eq!(current().mode, PermissionMode::Plan);
    }

    #[test]
    fn allowlist_denies_outside_tools() {
        let _g = lock();
        // 白名单非空：只允许列表内工具
        apply_from_cli(None, None, &[], &[], &["read_file".to_string()]).unwrap();
        assert!(matches!(adjudicate("read_file"), Verdict::Allow));
        assert!(matches!(adjudicate("write_file"), Verdict::Deny(_)));
        assert!(matches!(adjudicate("run_command"), Verdict::Deny(_)));
        // deny 仍优先于白名单：列表内但被 deny 的工具也拒绝
        apply_from_cli(
            None,
            None,
            &["read_file".to_string()],
            &[],
            &["read_file".to_string()],
        )
        .unwrap();
        assert!(matches!(adjudicate("read_file"), Verdict::Deny(_)));
        // 空白名单 = 不限制
        apply_from_cli(None, None, &[], &[], &[]).unwrap();
        assert_eq!(adjudicate("read_file"), Verdict::Ask);
    }

    #[test]
    fn deny_exact_name_only() {
        let _g = lock();
        with_cfg(PermissionMode::Bypass, &["write_file"]);
        assert!(matches!(adjudicate("write_file"), Verdict::Deny(_)));
        assert_eq!(adjudicate("edit_file"), Verdict::Allow);
    }

    #[test]
    fn default_cfg_is_safe() {
        let _g = lock();
        set_config(PermissionConfig::default());
        assert_eq!(adjudicate("write_file"), Verdict::Ask);
        assert_eq!(adjudicate("read_file"), Verdict::Ask);
    }
}
