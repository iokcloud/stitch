//! Project analysis — auto-detect framework versions, build commands,
//! lint configuration, and project structure.
//!
//! This extends the basic `WorkspaceContext` with deeper analysis:
//! - Language/framework versions (rustc, node, python, etc.)
//! - Build commands (from Cargo.toml, package.json scripts, Makefile targets)
//! - Lint/test commands (from config files)
//! - Directory structure summary

use std::path::Path;

/// Detailed project analysis result.
#[derive(Debug, Clone, Default)]
pub struct ProjectInfo {
    /// Detected project types (e.g., "Rust", "Node.js")
    pub project_types: Vec<String>,
    /// Language/framework version strings
    pub versions: Vec<String>,
    /// Suggested build commands
    pub build_commands: Vec<String>,
    /// Suggested test commands
    pub test_commands: Vec<String>,
    /// Suggested lint commands
    pub lint_commands: Vec<String>,
    /// Key config files found
    pub config_files: Vec<String>,
    /// Notable directory structure
    pub structure: Vec<String>,
}

/// Analyze a project directory and return structured info.
pub fn analyze(work_dir: &str) -> ProjectInfo {
    let root = Path::new(work_dir);
    let mut info = ProjectInfo::default();

    // Detect project types and versions
    detect_rust(root, &mut info);
    detect_node(root, &mut info);
    detect_python(root, &mut info);
    detect_go(root, &mut info);
    detect_make(root, &mut info);

    // Structure
    detect_structure(root, &mut info);

    info
}

/// Format the project info as a human-readable string for the system prompt.
pub fn format_for_prompt(info: &ProjectInfo) -> String {
    let mut s = String::new();

    if !info.project_types.is_empty() {
        s.push_str(&format!(
            "- Project type: {}\n",
            info.project_types.join(", ")
        ));
    }

    if !info.versions.is_empty() {
        s.push_str(&format!("- Versions: {}\n", info.versions.join(", ")));
    }

    if !info.build_commands.is_empty() {
        s.push_str(&format!("- Build: {}\n", info.build_commands.join("; ")));
    }

    if !info.test_commands.is_empty() {
        s.push_str(&format!("- Test: {}\n", info.test_commands.join("; ")));
    }

    if !info.lint_commands.is_empty() {
        s.push_str(&format!("- Lint: {}\n", info.lint_commands.join("; ")));
    }

    if !info.config_files.is_empty() {
        s.push_str(&format!("- Configs: {}\n", info.config_files.join(", ")));
    }

    s.trim().to_string()
}

// ── Detectors ──────────────────────────────────────────────────────

fn detect_rust(root: &Path, info: &mut ProjectInfo) {
    if !root.join("Cargo.toml").exists() {
        return;
    }

    info.project_types.push("Rust".into());

    // Detect workspace vs single crate
    if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml"))
        && (content.contains("[workspace]") || content.contains("[workspace.dependencies]"))
    {
        info.project_types.push("Cargo Workspace".into());
    }

    // Rust version
    if let Some(v) = run_hidden("rustc", &["--version"]) {
        info.versions.push(v);
    }

    info.build_commands.push("cargo build".into());
    info.test_commands.push("cargo test".into());
    info.lint_commands.push("cargo clippy".into());

    if root.join("rustfmt.toml").exists() || root.join(".rustfmt.toml").exists() {
        info.config_files.push("rustfmt.toml".into());
    }
    if root.join("clippy.toml").exists() {
        info.config_files.push("clippy.toml".into());
    }
}

fn detect_node(root: &Path, info: &mut ProjectInfo) {
    if !root.join("package.json").exists() {
        return;
    }

    info.project_types.push("Node.js".into());

    // Node version
    if let Some(v) = run_hidden("node", &["--version"]) {
        info.versions.push(format!("Node {v}"));
    }

    // npm version
    if let Some(v) = run_hidden("npm", &["--version"]) {
        info.versions.push(format!("npm {v}"));
    }

    // Parse package.json for scripts
    if let Ok(content) = std::fs::read_to_string(root.join("package.json"))
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(scripts) = json.get("scripts").and_then(|s| s.as_object())
    {
        if scripts.contains_key("build") {
            info.build_commands.push("npm run build".into());
        }
        if scripts.contains_key("test") {
            info.test_commands.push("npm test".into());
        }
        if scripts.contains_key("lint") {
            info.lint_commands.push("npm run lint".into());
        }
    }

    // TypeScript
    if root.join("tsconfig.json").exists() {
        info.project_types.push("TypeScript".into());
        info.config_files.push("tsconfig.json".into());
    }

    // Lint configs
    for cfg in &[
        ".eslintrc.js",
        ".eslintrc.json",
        ".eslintrc.yaml",
        "eslint.config.js",
    ] {
        if root.join(cfg).exists() {
            info.config_files.push(cfg.to_string());
            break;
        }
    }
    if root.join(".prettierrc").exists() || root.join("prettier.config.js").exists() {
        info.config_files.push("prettier".into());
    }
}

fn detect_python(root: &Path, info: &mut ProjectInfo) {
    let has_python = root.join("requirements.txt").exists()
        || root.join("pyproject.toml").exists()
        || root.join("setup.py").exists()
        || root.join("setup.cfg").exists();

    if !has_python {
        return;
    }

    info.project_types.push("Python".into());

    if let Some(v) =
        run_hidden("python3", &["--version"]).or_else(|| run_hidden("python", &["--version"]))
    {
        info.versions.push(v);
    }

    if root.join("pyproject.toml").exists() {
        info.config_files.push("pyproject.toml".into());
        info.build_commands.push("pip install -e .".into());
    }

    if root.join("requirements.txt").exists() {
        info.config_files.push("requirements.txt".into());
    }

    info.test_commands.push("pytest".into());

    if root.join(".flake8").exists() || root.join("setup.cfg").exists() {
        info.lint_commands.push("flake8".into());
    }
    if root.join("pyproject.toml").exists() {
        // Check for ruff config in pyproject.toml
        if let Ok(content) = std::fs::read_to_string(root.join("pyproject.toml"))
            && content.contains("[tool.ruff]")
        {
            info.lint_commands.push("ruff check".into());
        }
    }
}

fn detect_go(root: &Path, info: &mut ProjectInfo) {
    if !root.join("go.mod").exists() {
        return;
    }

    info.project_types.push("Go".into());

    // Go version from go.mod
    if let Ok(content) = std::fs::read_to_string(root.join("go.mod")) {
        for line in content.lines() {
            if let Some(ver) = line.strip_prefix("go ") {
                info.versions.push(format!("Go {ver}"));
                break;
            }
        }
    }

    info.build_commands.push("go build ./...".into());
    info.test_commands.push("go test ./...".into());
    info.lint_commands.push("go vet ./...".into());
}

fn detect_make(root: &Path, info: &mut ProjectInfo) {
    if !root.join("Makefile").exists() {
        return;
    }

    info.config_files.push("Makefile".into());

    // Parse common targets
    if let Ok(content) = std::fs::read_to_string(root.join("Makefile")) {
        let has_target = |name: &str| -> bool {
            content
                .lines()
                .any(|l| l.trim().starts_with(&format!("{name}:")))
        };

        if has_target("build") {
            info.build_commands.push("make build".into());
        }
        if has_target("test") {
            info.test_commands.push("make test".into());
        }
        if has_target("lint") {
            info.lint_commands.push("make lint".into());
        }
    }
}

fn detect_structure(root: &Path, info: &mut ProjectInfo) {
    // Quick directory scan for interesting top-level dirs
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') && name != ".github" {
                continue;
            }
            // Only report notable directories
            match name {
                "src" | "lib" | "crates" | "packages" | "tests" | "docs" | "scripts"
                | "examples" | "benches" | "migrations" | "deploy" | "docker" | "ci"
                | ".github" => {
                    info.structure.push(name.to_string());
                }
                _ => {}
            }
        }
    }
}

fn run_hidden(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    crate::tools::process_win::hide_console_std(&mut cmd);
    let output = cmd.output().ok()?;
    if !output.status.success() && output.stdout.is_empty() {
        return None;
    }
    // Some tools (python --version) write to stderr.
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).trim().to_string()
    } else {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_empty_info() {
        let info = ProjectInfo::default();
        assert_eq!(format_for_prompt(&info), "");
    }

    #[test]
    fn format_rust_project() {
        let info = ProjectInfo {
            project_types: vec!["Rust".into(), "Cargo Workspace".into()],
            versions: vec!["rustc 1.85.0".into()],
            build_commands: vec!["cargo build".into()],
            test_commands: vec!["cargo test".into()],
            lint_commands: vec!["cargo clippy".into()],
            config_files: vec!["rustfmt.toml".into()],
            structure: vec!["src".into(), "docs".into()],
        };
        let formatted = format_for_prompt(&info);
        assert!(formatted.contains("Rust, Cargo Workspace"));
        assert!(formatted.contains("rustc 1.85.0"));
        assert!(formatted.contains("cargo build"));
        assert!(formatted.contains("cargo test"));
        assert!(formatted.contains("cargo clippy"));
    }
}
