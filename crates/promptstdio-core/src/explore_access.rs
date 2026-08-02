//! Explore / 会员专区系统库访问级别。

/// Explore 公开列表与详情可见。
/// Explore「IDE 技能」Tab / Skill 目录：须带此 canonical tag。
/// 完整 Skill 经 MCP `install_skill` 下发；会员专区承载工具包套件，不是 Skill 安装入口。
pub const TAG_IDE_SKILL: &str = "ide-skill";

pub const ACCESS_LEVEL_PUBLIC: &str = "public";
/// 仅会员专区路由加载；Explore 列表/保存不可见。
pub const ACCESS_LEVEL_MEMBER_ONLY: &str = "member_only";
/// 系统提示词：运营后台可见，Explore 不可见。
pub const ACCESS_LEVEL_PRIVATE: &str = "private";

pub fn is_member_only(access_level: &str) -> bool {
    access_level == ACCESS_LEVEL_MEMBER_ONLY
}

/// 运营后台保存系统套件访问级别（未知值回落 public）。
pub fn normalize_suite_access_level(level: &str) -> &'static str {
    if is_member_only(level) {
        ACCESS_LEVEL_MEMBER_ONLY
    } else {
        ACCESS_LEVEL_PUBLIC
    }
}

/// 运营后台保存系统提示词访问级别。
pub fn normalize_prompt_access_level(level: &str) -> &'static str {
    match level {
        v if v == ACCESS_LEVEL_MEMBER_ONLY => ACCESS_LEVEL_MEMBER_ONLY,
        v if v == ACCESS_LEVEL_PRIVATE => ACCESS_LEVEL_PRIVATE,
        _ => ACCESS_LEVEL_PUBLIC,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_suite_access_level_values() {
        assert_eq!(
            normalize_suite_access_level("member_only"),
            ACCESS_LEVEL_MEMBER_ONLY
        );
        assert_eq!(normalize_suite_access_level("public"), ACCESS_LEVEL_PUBLIC);
        assert_eq!(normalize_suite_access_level("unknown"), ACCESS_LEVEL_PUBLIC);
    }

    #[test]
    fn normalize_prompt_access_level_values() {
        assert_eq!(
            normalize_prompt_access_level("member_only"),
            ACCESS_LEVEL_MEMBER_ONLY
        );
        assert_eq!(
            normalize_prompt_access_level("private"),
            ACCESS_LEVEL_PRIVATE
        );
        assert_eq!(normalize_prompt_access_level("public"), ACCESS_LEVEL_PUBLIC);
    }
}
