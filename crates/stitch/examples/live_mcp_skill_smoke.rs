//! Live smoke: local Skill inventory + windows-mcp tools/list.
//!
//! ```bash
//! cd rust && cargo run -p stitch --example live_mcp_skill_smoke
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use stitch::config::McpServerProfile;
use stitch::mcp_protocol;

fn scan_local_skills(work_dir: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let root = PathBuf::from(work_dir).join(".agents").join("skills");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for ent in entries.flatten() {
        if !ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let slug = ent.file_name().to_string_lossy().to_string();
        let skill_md = ent.path().join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let body = std::fs::read_to_string(&skill_md).unwrap_or_default();
        let title = body
            .lines()
            .find_map(|l| l.strip_prefix("name:"))
            .map(|s| s.trim().trim_matches('"').to_string())
            .unwrap_or_else(|| slug.clone());
        out.push((
            slug,
            title,
            format!(".agents/skills/{}", ent.file_name().to_string_lossy()),
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[tokio::main]
async fn main() {
    let work_dir = std::env::var("STITCH_SMOKE_WORKDIR")
        .unwrap_or_else(|_| ".".into()); // 默认当前目录，可覆盖

    println!("== Skill inventory ==");
    println!("work_dir={work_dir}");
    let mut skills = scan_local_skills(&work_dir);
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = std::path::PathBuf::from(home);
        for base in [".agents/skills", ".cursor/skills"] {
            let root = home.join(base);
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            for ent in entries.flatten() {
                if !ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let slug = ent.file_name().to_string_lossy().to_string();
                let skill_md = ent.path().join("SKILL.md");
                if !skill_md.is_file() {
                    continue;
                }
                if skills.iter().any(|(s, _, _)| s == &slug) {
                    continue;
                }
                skills.push((slug.clone(), slug.clone(), format!("~/{base}/{slug}")));
            }
        }
    }
    if skills.is_empty() {
        println!("FAIL: no skills under workdir or ~/.agents|~/.cursor/skills");
        std::process::exit(1);
    }
    for (slug, title, rel) in &skills {
        println!("OK skill slug={slug} title={title} path={rel}");
    }
    println!("skill_count={}", skills.len());

    println!("== windows-mcp tools/list ==");
    let profile = McpServerProfile {
        id: "windows-mcp".into(),
        label: "windows-mcp".into(),
        transport: "stdio".into(),
        enabled: true,
        command: Some("uvx".into()),
        args: vec!["windows-mcp".into(), "serve".into()],
        env: HashMap::new(),
        cwd: None,
        url: None,
        headers: HashMap::new(),
    };
    match mcp_protocol::list_tools(&profile).await {
        Ok(tools) => {
            println!("OK windows-mcp connected tool_count={}", tools.len());
            for t in tools.iter().take(12) {
                println!("  - {}", t.remote_name);
            }
            if tools.len() > 12 {
                println!("  … {} more", tools.len() - 12);
            }
            if tools.is_empty() {
                println!("FAIL: connected but 0 tools");
                std::process::exit(2);
            }
        }
        Err(e) => {
            println!("FAIL windows-mcp: {e}");
            std::process::exit(3);
        }
    }
    println!("ALL PASS");
}
