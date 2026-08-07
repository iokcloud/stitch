//! Resolve tool paths under the agent working directory.
//!
//! All generated / edited files must stay inside `work_dir`. Absolute paths
//! and `..` escapes are rejected.

use std::path::{Component, Path, PathBuf};

/// Resolve `user_path` to an absolute path that is guaranteed to be under `work_dir`.
pub fn resolve_under_work_dir(work_dir: &Path, user_path: &str) -> anyhow::Result<PathBuf> {
    resolve_under_roots(&[work_dir.to_path_buf()], user_path)
}

/// Resolve `user_path` under any of the given roots (`roots[0]` is the main
/// working directory; the rest are `--add-dir` additional directories).
/// Relative paths resolve against `roots[0]`; absolute paths are allowed only
/// if they land inside one of the roots. `..` escapes are rejected.
pub fn resolve_under_roots(roots: &[PathBuf], user_path: &str) -> anyhow::Result<PathBuf> {
    let trimmed = user_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Empty path");
    }
    let joined = if looks_absolute(trimmed) {
        normalize_lexically(Path::new(trimmed))
    } else {
        let work_abs = abs_work_dir(&roots[0])?;
        normalize_lexically(&work_abs.join(trimmed))
    };

    for root in roots {
        let root_abs = abs_work_dir(root)?;
        if is_under(&joined, &root_abs) {
            return Ok(joined);
        }
    }
    anyhow::bail!("Path traversal denied: {trimmed}")
}

/// Human-readable path relative to `work_dir` (no Windows `\\?\` prefix).
pub fn display_rel_under_work_dir(work_dir: &Path, absolute: &Path) -> String {
    let work = strip_verbatim(&abs_work_dir(work_dir).unwrap_or_else(|_| work_dir.to_path_buf()));
    let abs = strip_verbatim(absolute);
    if let Ok(rel) = abs.strip_prefix(&work) {
        let s = rel.to_string_lossy().replace('\\', "/");
        if s.is_empty() { ".".into() } else { s }
    } else {
        abs.to_string_lossy().replace('\\', "/")
    }
}

fn looks_absolute(path: &str) -> bool {
    let p = Path::new(path);
    if p.is_absolute() {
        return true;
    }
    // Windows drive paths: `C:\…` or `C:/…` or `C:foo`
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return true;
    }
    // UNC `\\server\share`
    path.starts_with("\\\\") || path.starts_with("//")
}

fn abs_work_dir(work_dir: &Path) -> anyhow::Result<PathBuf> {
    if work_dir.as_os_str().is_empty() {
        anyhow::bail!("Working directory is empty");
    }
    if work_dir.exists() {
        Ok(std::fs::canonicalize(work_dir).unwrap_or_else(|_| normalize_lexically(work_dir)))
    } else {
        Ok(normalize_lexically(work_dir))
    }
}

/// Lexical normalization (no filesystem access): collapse `.` / `..`.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                // Prefer popping a normal segment; if nothing to pop, keep `..`
                // so the subsequent under-check rejects escapes.
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !out.pop() {
                    out.push("..");
                }
            }
            Component::Normal(c) => out.push(c),
        }
    }
    out
}

fn is_under(path: &Path, root: &Path) -> bool {
    let path_s = strip_verbatim(path);
    let root_s = strip_verbatim(root);
    path_s.starts_with(&root_s)
}

/// Windows `\\?\` prefix breaks naive `starts_with`; strip when comparing / displaying.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else if let Some(rest) = s.strip_prefix(r"//?/") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// Whether `user_path` resolves inside any of the given roots (`roots[0]` is
/// the main working directory). Used by the confirm gate so `--add-dir`
/// additional directories are treated like workspace paths (reads need no
/// confirmation; writes follow the permission mode like workspace writes).
pub fn path_within_roots(user_path: &str, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .map(|r| r.to_string_lossy().to_string())
        .any(|r| path_within(user_path, Some(&r)))
}

/// Whether `user_path` (absolute, or relative to `work_dir`) resolves inside
/// `work_dir`. Used by the confirm gate to spot outside-workspace reads.
/// A missing/empty `work_dir` counts as outside (nothing to contain paths).
pub fn path_within(user_path: &str, work_dir: Option<&str>) -> bool {
    let Some(wd) = work_dir.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let wd_abs = strip_verbatim(&normalize_lexically(Path::new(wd)));
    let target = if looks_absolute(user_path.trim()) {
        strip_verbatim(&normalize_lexically(Path::new(user_path.trim())))
    } else {
        strip_verbatim(&normalize_lexically(&wd_abs.join(user_path.trim())))
    };
    path_starts_with(&target, &wd_abs)
}

/// Case-folded component prefix check — Windows paths are case-insensitive,
/// so `Path::starts_with` alone would false-negative on casing.
fn path_starts_with(path: &Path, root: &Path) -> bool {
    let p = path.to_string_lossy().to_lowercase();
    let r = root.to_string_lossy().to_lowercase();
    if p == r {
        return true;
    }
    if !p.starts_with(&r) {
        return false;
    }
    matches!(p.as_bytes().get(r.len()), Some(b'/') | Some(b'\\'))
}

/// Resolve an outside-workspace read after the gate authorized it (the
/// `__stitch_scoped` marker is set). Absolute paths are normalized lexically;
/// relative paths resolve against `work_dir`. No under-check: the scope was
/// user-approved (or matched an allow rule).
pub fn resolve_scoped(work_dir: &Path, user_path: &str) -> anyhow::Result<PathBuf> {
    let trimmed = user_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Empty path");
    }
    let resolved = if looks_absolute(trimmed) {
        normalize_lexically(Path::new(trimmed))
    } else {
        let work_abs = abs_work_dir(work_dir)?;
        normalize_lexically(&work_abs.join(trimmed))
    };
    Ok(strip_verbatim(&resolved))
}

/// Whether the agent gate authorized outside-workspace access for this call
/// (the internal marker — never part of any tool schema, injected only after
/// user approval or a matching allow rule).
pub fn scoped_allowed(arguments: &serde_json::Value) -> bool {
    arguments
        .get(crate::allow::SCOPED_MARKER)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn relative_ok() {
        // canonicalize 对齐 resolve_under_work_dir 的真实路径（GitHub Actions
        // runner 的 temp 目录可能是 junction——原始路径 starts_with 会失败）
        let dir = tempfile_dir("stitch-path-ok").canonicalize().unwrap();
        let p = resolve_under_work_dir(&dir, "src/main.rs").unwrap();
        assert!(p.starts_with(&dir) || is_under(&p, &dir));
        assert!(p.ends_with("main.rs"));
    }

    #[test]
    fn rejects_parent_escape() {
        let dir = tempfile_dir("stitch-path-escape");
        let err = resolve_under_work_dir(&dir, "../outside.txt").unwrap_err();
        assert!(err.to_string().contains("denied") || err.to_string().contains("relative"));
    }

    #[test]
    fn rejects_absolute() {
        let dir = tempfile_dir("stitch-path-abs");
        #[cfg(windows)]
        let abs = "C:/Windows/Temp/evil.txt";
        #[cfg(not(windows))]
        let abs = "/tmp/evil.txt";
        let err = resolve_under_work_dir(&dir, abs).unwrap_err();
        // 绝对路径不再一律拒绝：落在根内则允许（--add-dir 语义），
        // 根外绝对路径拒绝。
        assert!(err.to_string().contains("denied"));
    }

    #[test]
    fn nested_create_path_ok() {
        let dir = tempfile_dir("stitch-path-nest");
        let p = resolve_under_work_dir(&dir, "pkg/mod/hello.py").unwrap();
        assert!(is_under(
            &p,
            &std::fs::canonicalize(&dir).unwrap_or(dir.clone())
        ));
        assert!(p.to_string_lossy().contains("hello.py"));
    }

    #[test]
    fn display_rel_strips_verbatim() {
        let dir = tempfile_dir("stitch-path-disp");
        let nested = dir.join(".agents").join("skills");
        fs::create_dir_all(&nested).unwrap();
        let abs = resolve_under_work_dir(&dir, ".agents/skills").unwrap();
        let rel = display_rel_under_work_dir(&dir, &abs);
        assert!(!rel.contains(r"\\?\"));
        assert!(rel.replace('\\', "/").contains(".agents/skills"));
    }

    fn tempfile_dir(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn path_within_accepts_inside_and_rejects_outside() {
        let wd = "C:/work/project";
        assert!(path_within("src/main.rs", Some(wd)));
        assert!(path_within("C:/work/project/src/main.rs", Some(wd)));
        assert!(path_within("C:/WORK/PROJECT", Some(wd)));
        assert!(!path_within("C:/work/other/secret.txt", Some(wd)));
        assert!(!path_within("C:/work/project2/x.txt", Some(wd)));
        assert!(!path_within("C:/Windows/Temp/x.txt", Some(wd)));
    }

    #[test]
    fn path_within_rejects_parent_escape_and_empty_workdir() {
        let wd = "C:/work/project";
        assert!(!path_within("../outside.txt", Some(wd)));
        assert!(!path_within("C:/work/project/../outside.txt", Some(wd)));
        // No work dir → nothing is contained.
        assert!(!path_within("src/main.rs", None));
        assert!(!path_within("src/main.rs", Some("  ")));
    }

    #[test]
    fn path_within_matches_workdir_itself() {
        assert!(path_within("C:/work/project", Some("C:/work/project")));
        assert!(path_within(".", Some("C:/work/project")));
    }

    #[test]
    fn resolve_scoped_resolves_absolute_and_relative() {
        let wd = std::path::Path::new("C:/work/project");
        let abs = resolve_scoped(wd, "C:/Windows/Temp/a.txt").unwrap();
        assert_eq!(abs, std::path::Path::new("C:/Windows/Temp/a.txt"));
        let rel = resolve_scoped(wd, "sub/b.txt").unwrap();
        assert_eq!(rel, std::path::Path::new("C:/work/project/sub/b.txt"));
        // `..` escape is normalized, not rejected — the gate showed the
        // resolved target to the user before authorizing.
        let esc = resolve_scoped(wd, "../other.txt").unwrap();
        assert_eq!(esc, std::path::Path::new("C:/work/other.txt"));
    }

    #[test]
    fn scoped_allowed_reads_marker_only() {
        let plain = serde_json::json!({ "path": "a.txt" });
        assert!(!scoped_allowed(&plain));
        let marked = serde_json::json!({ "path": "a.txt", "__stitch_scoped": true });
        assert!(scoped_allowed(&marked));
        let spoofed = serde_json::json!({ "path": "a.txt", "__stitch_scoped": false });
        assert!(!scoped_allowed(&spoofed));
    }

    #[test]
    fn resolve_under_roots_allows_additional_dirs() {
        let work = tempfile_dir("stitch-roots-work").canonicalize().unwrap();
        let extra = tempfile_dir("stitch-roots-extra").canonicalize().unwrap();
        let roots = vec![work.clone(), extra.clone()];
        // 附加根内相对路径（按主目录解析后落入附加根）
        let rel_in_extra = resolve_under_roots(&roots, "../stitch-roots-extra/notes.txt");
        assert!(
            rel_in_extra.is_err(),
            "相对路径以主目录为基准，`..` 逃逸仍拒绝"
        );
        // 附加根内绝对路径
        let abs =
            resolve_under_roots(&roots, &extra.join("notes.txt").display().to_string()).unwrap();
        assert_eq!(abs, extra.join("notes.txt"));
        // 主根内相对路径照常
        let main = resolve_under_roots(&roots, "main.txt").unwrap();
        assert!(main.starts_with(&work));
        // 附加根之外的绝对路径拒绝
        let outside = tempfile_dir("stitch-roots-outside");
        let err = resolve_under_roots(&roots, &outside.display().to_string()).unwrap_err();
        assert!(err.to_string().contains("denied"));
    }

    #[test]
    fn path_within_roots_matches_any_root() {
        let work = PathBuf::from("C:/work/main");
        let extra = PathBuf::from("C:/work/extra");
        let roots = vec![work, extra];
        assert!(path_within_roots("C:/work/main/src/a.rs", &roots));
        assert!(path_within_roots("C:/work/extra/b.rs", &roots));
        assert!(!path_within_roots("C:/work/other/c.rs", &roots));
        assert!(!path_within_roots("../escape.txt", &roots));
        // `..` 规范化后落回附加根内 → 允许（解析结果仍安全）
        assert!(path_within_roots("C:/work/main/../extra/up.txt", &roots));
        assert!(!path_within_roots(
            "C:/work/main/../../elsewhere/x.rs",
            &roots
        ));
    }
}
