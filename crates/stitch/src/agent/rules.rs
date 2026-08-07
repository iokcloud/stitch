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

use std::path::{Path, PathBuf};

/// Maximum size of a rules file to load (to prevent loading huge files).
const MAX_RULES_SIZE: u64 = 64 * 1024; // 64 KB

/// Load and merge all applicable rules for the given working directory.
///
/// Returns `None` if no rules files exist or they're all empty.
pub fn load_rules(work_dir: &str) -> Option<String> {
    load_rules_with_paths(work_dir, &global_memory_path())
}

/// 带全局记忆路径参数的内部实现（测试注入临时路径用）。
fn load_rules_with_paths(work_dir: &str, global_memory: &PathBuf) -> Option<String> {
    let mut combined = String::new();

    // 0. Global memory（Claude Code 语义）：config_dir/CLAUDE.md 为用户级记忆，
    //    每次会话都注入；项目 CLAUDE.md 后加载，冲突时项目覆盖。
    if let Some(content) = load_rules_file(global_memory) {
        combined.push_str("## Global CLAUDE.md\n\n");
        combined.push_str(&content);
        combined.push_str("\n\n");
    }

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

    // 3. Project memory（Claude Code 语义）：工作区根 CLAUDE.md 为权威记忆，
    //    AGENTS.md 为通用代理指令（共存则都加载，CLAUDE.md 在前——更贴近
    //    用户表达，覆盖冲突时后加载者优先）。
    for name in ["CLAUDE.md", "AGENTS.md"] {
        let memory_path = PathBuf::from(work_dir).join(name);
        if let Some(content) = load_rules_file(&memory_path) {
            combined.push_str(&format!("## {name}\n\n"));
            combined.push_str(&content);
            combined.push_str("\n\n");
        }
    }

    // 4. Local memory（Claude Code 语义）：CLAUDE.local.md 为工作区本地
    //    私有记忆（应 .gitignore 不提交），最后加载——优先级最高，覆盖冲突。
    let local_memory = PathBuf::from(work_dir).join("CLAUDE.local.md");
    if let Some(content) = load_rules_file(&local_memory) {
        combined.push_str("## CLAUDE.local.md\n\n");
        combined.push_str(&content);
        combined.push_str("\n\n");
    }

    let trimmed = combined.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// 全局记忆路径：`config_dir()/CLAUDE.md`（与 hooks.json 同级，位置以
/// `stitch doctor` / `stitch config` 输出为准）。
fn global_memory_path() -> PathBuf {
    crate::config::config_dir().join("CLAUDE.md")
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

/// 子目录就近记忆：从 `file_path` 所在目录逐级向上，每层读
/// CLAUDE.md / AGENTS.md（最近目录在前、外层在后），供 ReadFile 注入
/// 结果——agent 读取深层文件时带上目录级上下文。
/// `roots[0]` 为主工作目录，其根记忆已由 `load_rules` 注入系统提示，
/// 此处不重复；附加根（`--add-dir`）的记忆在本函数中注入后停止向上。
pub fn directory_memory(file_path: &Path, roots: &[PathBuf]) -> Option<String> {
    let mut combined = String::new();
    let mut dir = file_path.parent();
    while let Some(d) = dir {
        let is_root = roots.iter().any(|r| same_path(r, d));
        if is_root && roots.first().is_some_and(|r| same_path(r, d)) {
            break; // 主根记忆已在系统提示，避免每次读文件重复注入
        }
        for name in ["CLAUDE.md", "AGENTS.md"] {
            if let Some(content) = load_rules_file(&d.join(name)) {
                combined.push_str(&format!("## [{}/{}]\n\n", d.display(), name));
                combined.push_str(&content);
                combined.push_str("\n\n");
            }
        }
        if is_root {
            break; // 附加根已读，不再向上
        }
        dir = d.parent();
    }
    let trimmed = combined.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// 路径比较（Windows 大小写不敏感 + 去掉 `\\?\` 前缀与尾部分隔符）。
fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        crate::tools::paths::strip_verbatim(p)
            .to_string_lossy()
            .to_lowercase()
            .trim_end_matches(['/', '\\'])
            .to_string()
    };
    norm(a) == norm(b)
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

    #[test]
    fn loads_claude_md_and_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "项目记忆：先跑测试再交差。").unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "# 通用指令\n不要动 docs/archive/。",
        )
        .unwrap();

        let result = load_rules(&dir.path().display().to_string());
        assert!(result.is_some());
        let rules = result.unwrap();
        assert!(rules.contains("## CLAUDE.md"));
        assert!(rules.contains("## AGENTS.md"));
        assert!(rules.contains("先跑测试再交差"));
        assert!(rules.contains("不要动 docs/archive/"));
        // CLAUDE.md 在 AGENTS.md 之前
        assert!(
            rules.find("## CLAUDE.md").unwrap() < rules.find("## AGENTS.md").unwrap(),
            "CLAUDE.md 须优先：{rules}"
        );
    }

    #[test]
    fn loads_global_claude_md_before_project() {
        let dir = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        let global_mem = global.path().join("CLAUDE.md");
        std::fs::write(&global_mem, "全局记忆：回答用中文。").unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "项目记忆：先跑测试再交差。").unwrap();

        let result = load_rules_with_paths(&dir.path().display().to_string(), &global_mem);
        assert!(result.is_some());
        let rules = result.unwrap();
        assert!(rules.contains("## Global CLAUDE.md"));
        assert!(rules.contains("全局记忆"));
        assert!(rules.contains("## CLAUDE.md"));
        // 全局记忆在项目记忆之前
        assert!(
            rules.find("## Global CLAUDE.md").unwrap() < rules.find("## CLAUDE.md").unwrap(),
            "全局记忆须优先：{rules}"
        );
    }

    #[test]
    fn directory_memory_nearest_first() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        let nested = work.join("pkg").join("mod");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(work.join("CLAUDE.md"), "工作区根记忆").unwrap();
        std::fs::write(work.join("pkg").join("CLAUDE.md"), "pkg 层记忆").unwrap();
        std::fs::write(nested.join("AGENTS.md"), "mod 层通用指令").unwrap();
        let file = nested.join("main.rs");
        std::fs::write(&file, "fn main() {}").unwrap();

        let mem = directory_memory(&file, std::slice::from_ref(&work)).unwrap();
        // 最近目录在前：mod 层先于 pkg 层
        assert!(
            mem.find("mod 层通用指令").unwrap() < mem.find("pkg 层记忆").unwrap(),
            "就近优先：{mem}"
        );
        // 主根记忆不重复注入（已在系统提示）
        assert!(!mem.contains("工作区根记忆"), "主根记忆应跳过：{mem}");
        assert!(mem.contains("AGENTS.md"));
    }

    #[test]
    fn directory_memory_no_memory_files() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(work.join("sub")).unwrap();
        let file = work.join("sub").join("a.rs");
        std::fs::write(&file, "x").unwrap();
        assert!(directory_memory(&file, &[work]).is_none());
    }

    #[test]
    fn directory_memory_additional_root_reads_then_stops() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        let extra = dir.path().join("extra");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&extra).unwrap();
        // 附加根自己的记忆应注入（系统提示未覆盖附加根）
        std::fs::write(extra.join("CLAUDE.md"), "附加根记忆").unwrap();
        let file = extra.join("notes.txt");
        std::fs::write(&file, "note").unwrap();

        let roots = vec![work.clone(), extra.clone()];
        let mem = directory_memory(&file, &roots).unwrap();
        assert!(mem.contains("附加根记忆"));
        // 到附加根为止，不再向上（不读附加根的上级目录）
        let above = dir.path().join("CLAUDE.md");
        std::fs::write(&above, "上级目录记忆").unwrap();
        let mem2 = directory_memory(&file, &roots).unwrap();
        assert!(!mem2.contains("上级目录记忆"));
    }

    #[test]
    fn loads_claude_local_md_last_with_highest_priority() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "项目记忆：先跑测试。").unwrap();
        std::fs::write(
            dir.path().join("CLAUDE.local.md"),
            "本地记忆：本机目录在 D:/work，覆盖冲突。",
        )
        .unwrap();

        let result = load_rules(&dir.path().display().to_string());
        assert!(result.is_some());
        let rules = result.unwrap();
        assert!(rules.contains("## CLAUDE.md"));
        assert!(rules.contains("## CLAUDE.local.md"));
        // 本地记忆最后加载 → 排在项目记忆之后（优先级最高）
        assert!(
            rules.find("## CLAUDE.local.md").unwrap() > rules.find("## CLAUDE.md").unwrap(),
            "CLAUDE.local.md 须在 CLAUDE.md 之后：{rules}"
        );
        assert!(rules.contains("D:/work"));
    }

    #[test]
    fn claude_md_without_other_rules_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("CLAUDE.md"),
            "只有项目记忆，没有 .stitchrules。",
        )
        .unwrap();
        let result = load_rules(&dir.path().display().to_string());
        assert!(result.is_some());
        assert!(result.unwrap().contains("只有项目记忆"));
    }
}
