//! .stitchignore 忽略规则（gitignore 语法子集，对标 Claude Code 的 .claudeignore）。
//!
//! 工作区根放一个 `.stitchignore`：read_file / list_directory / find_path /
//! search_code 跳过被忽略的路径；系统提示注入规则摘要，让模型知道哪些文件不可读。
//!
//! 支持的语法子集：
//! - 空行、`#` 注释
//! - `!` 前缀 → 否定（取消忽略，最后匹配胜出）
//! - 末尾 `/` → 目录规则（目录本身及其下全部命中）
//! - 开头 `/` → 锚定工作区根（只匹配根级）
//! - 含 `/`（非末尾）→ 相对根的路径前缀规则
//! - 无 `/` → 匹配任意层级的同名段
//! - `*`（不跨 `/`）与 `?` 单段通配

use std::path::{Path, PathBuf};

/// 单条规则（解析后）。
struct Rule {
    /// 匹配段序列（/ 拆分，`*`/`?` 通配保留）。
    segments: Vec<String>,
    /// `!` 前缀：命中时取消忽略。
    negated: bool,
    /// 末尾 `/`：目录规则（命中前缀即忽略其下全部）。
    dir_only: bool,
    /// 开头 `/`：只匹配根级。
    anchored: bool,
}

impl Rule {
    fn matches(&self, segments: &[&str]) -> bool {
        if self.segments.len() == 1 && !self.anchored {
            // basename 规则：任意层级的同名段命中
            return segments
                .iter()
                .any(|s| super::find_path::glob_match(&self.segments[0], s));
        }
        // 路径规则（含锚定）：整路径或任一层前缀命中（glob 通配生效，
        // `src/*.tmp` 命中 `src/a.tmp`；`src/gen` 命中 `src/gen/x`）
        let pat = self.segments.join("/");
        let rel = segments.join("/");
        if super::find_path::glob_match(&pat, &rel) {
            return true;
        }
        segments
            .iter()
            .enumerate()
            .skip(1)
            .any(|(len, _)| super::find_path::glob_match(&pat, &segments[..len].join("/")))
    }
}

/// 工作区忽略规则集。
#[derive(Default)]
pub struct IgnoreRules {
    rules: Vec<Rule>,
    /// 规则文件路径（rg --ignore-file 用；parse 构造的规则没有文件）。
    path: Option<PathBuf>,
}

impl IgnoreRules {
    /// 空规则（无 .stitchignore 文件时）。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 从 `work_dir/.stitchignore` 加载（文件不存在 → 空规则）。
    pub fn load(work_dir: &Path) -> Self {
        let path = work_dir.join(".stitchignore");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Self::empty();
        };
        let mut rules = Self::parse(&content);
        rules.path = Some(path);
        rules
    }

    /// 解析规则文本（独立成函数便于测试）。
    pub fn parse(content: &str) -> Self {
        let mut rules = Vec::new();
        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut negated = false;
            let mut body = line;
            if let Some(rest) = body.strip_prefix('!') {
                negated = true;
                body = rest.trim();
                if body.is_empty() {
                    continue;
                }
            }
            // 尾部反斜杠转义则不是目录标记（`foo\/`）；子集不处理转义，直接判末尾 /
            let mut dir_only = false;
            let mut pat = body;
            if pat.ends_with('/') {
                dir_only = true;
                pat = &pat[..pat.len() - 1];
            }
            let mut anchored = false;
            if let Some(rest) = pat.strip_prefix('/') {
                anchored = true;
                pat = rest;
            }
            if pat.is_empty() {
                continue;
            }
            rules.push(Rule {
                segments: pat.split('/').map(str::to_string).collect(),
                negated,
                dir_only,
                anchored,
            });
        }
        Self { rules, path: None }
    }

    /// 规则集是否为空（无规则可匹配）。
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// 规则文件路径（rg `--ignore-file` 用；无文件时返回空串）。
    pub fn path_string(&self) -> String {
        self.path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }

    /// 判断相对工作区的 POSIX 路径（如 `src/gen/out.rs`）是否被忽略。
    /// 目录规则命中其祖先前缀即视为忽略（gitignore 语义）。
    pub fn is_ignored(&self, rel: &str) -> bool {
        let rel = rel.trim_start_matches("./").trim_start_matches('/');
        if rel.is_empty() {
            return false;
        }
        let segments: Vec<&str> = rel.split('/').collect();
        let mut ignored = false;
        // 前缀链逐级测试：自身 + 各级祖先（`node_modules/` 命中 `node_modules/pkg/x.js`）
        for len in 1..=segments.len() {
            let prefix = &segments[..len];
            for rule in &self.rules {
                if rule.matches(prefix) {
                    ignored = !rule.negated;
                }
            }
        }
        ignored
    }

    /// 非空时返回系统提示注入文本（模型知道哪些文件不可读）。
    pub fn summary(&self) -> Option<String> {
        if self.rules.is_empty() {
            return None;
        }
        let patterns: Vec<String> = self
            .rules
            .iter()
            .map(|r| {
                let mut s = if r.negated { "!" } else { "" }.to_string();
                s.push_str(&r.segments.join("/"));
                if r.dir_only {
                    s.push('/');
                }
                s
            })
            .collect();
        Some(format!(
            "工作区 .stitchignore 声明以下路径不可读、不可搜索（read_file / 搜索工具会拒绝）：\n{}",
            patterns.join(" · ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_rule_matches_any_level() {
        let r = IgnoreRules::parse("node_modules\n");
        assert!(r.is_ignored("node_modules/pkg/a.js"));
        assert!(r.is_ignored("node_modules"));
        assert!(!r.is_ignored("src/main.rs"));
    }

    #[test]
    fn path_rule_matches_prefix() {
        let r = IgnoreRules::parse("src/gen/\n");
        assert!(r.is_ignored("src/gen/out.rs"));
        assert!(r.is_ignored("src/gen"));
        assert!(!r.is_ignored("src/other.rs"));
        assert!(!r.is_ignored("gen/out.rs"));
    }

    #[test]
    fn anchored_rule_matches_root_only() {
        let r = IgnoreRules::parse("/notes.txt\n");
        assert!(r.is_ignored("notes.txt"));
        assert!(!r.is_ignored("sub/notes.txt"));
    }

    #[test]
    fn negation_wins_by_last_match() {
        let r = IgnoreRules::parse("node_modules\n!node_modules/keep.js\n");
        assert!(!r.is_ignored("node_modules/keep.js"));
        assert!(r.is_ignored("node_modules/drop.js"));
    }

    #[test]
    fn glob_and_question_mark() {
        let r = IgnoreRules::parse("*.log\nsrc/*.tmp\n");
        assert!(r.is_ignored("debug.log"));
        assert!(r.is_ignored("src/debug.log"));
        assert!(r.is_ignored("src/a.tmp"));
        assert!(!r.is_ignored("a.tmp"));
        assert!(!r.is_ignored("src/out.rs"));
    }

    #[test]
    fn comments_and_blanks_ignored() {
        let r = IgnoreRules::parse("# 注释\n\nnode_modules\n");
        assert_eq!(r.rules.len(), 1);
        assert!(r.is_ignored("node_modules/x"));
    }

    #[test]
    fn empty_when_no_file() {
        let dir = std::env::temp_dir().join(format!("stitch-ignore-none-{}", std::process::id()));
        let r = IgnoreRules::load(&dir);
        assert!(!r.is_ignored("anything"));
        assert!(r.summary().is_none());
    }

    #[test]
    fn summary_lists_patterns() {
        let r = IgnoreRules::parse("node_modules\n!src/keep.rs\nbuild/\n");
        let s = r.summary().unwrap();
        assert!(s.contains("node_modules"));
        assert!(s.contains("!src/keep.rs"));
        assert!(s.contains("build/"));
    }
}
