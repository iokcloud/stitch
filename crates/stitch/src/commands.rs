//! 自定义 slash 命令（Claude Code 语义）：`.claude/commands/*.md` + `config_dir/commands/*.md`。
//!
//! 文件名（不含扩展名）= 命令名；frontmatter：`description` / `argument-hint`；
//! 正文 = 提示词正文，`$ARGUMENTS` 替换为用户输入（无占位符则参数追加）。
//! 全局与项目同名时项目覆盖（与 agents 一致）。
#![allow(clippy::disallowed_methods)] // json! 宏展开 + 测试临时目录操作，项目惯例

use std::path::PathBuf;

/// 自定义命令定义。
pub struct CommandDef {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
    pub body: String,
}

impl CommandDef {
    /// 渲染最终提示词：`$ARGUMENTS` 占位替换；无占位且参数非空则追加。
    pub fn render(&self, arguments: &str) -> String {
        if self.body.contains("$ARGUMENTS") {
            self.body.replace("$ARGUMENTS", arguments)
        } else if arguments.trim().is_empty() {
            self.body.clone()
        } else {
            format!("{}\n\n{}", self.body.trim_end(), arguments.trim())
        }
    }
}

/// 加载自定义命令：全局 config_dir/commands/*.md + 项目 .claude/commands/*.md（项目覆盖同名）。
pub fn load_commands(work_dir: Option<&str>) -> Vec<CommandDef> {
    let global = crate::config::config_dir().join("commands");
    let global_defs = read_commands_dir(&global);
    let proj_defs = match work_dir {
        Some(wd) => read_commands_dir(&PathBuf::from(wd).join(".claude").join("commands")),
        None => Vec::new(),
    };
    merge_commands(global_defs, proj_defs)
}

/// 合并全局与项目命令：项目覆盖同名。
pub fn merge_commands(global: Vec<CommandDef>, proj: Vec<CommandDef>) -> Vec<CommandDef> {
    let mut out = global;
    for def in proj {
        if let Some(slot) = out.iter_mut().find(|d| d.name == def.name) {
            *slot = def;
        } else {
            out.push(def);
        }
    }
    out
}

fn read_commands_dir(dir: &PathBuf) -> Vec<CommandDef> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .collect();
    files.sort_by_key(|e| e.file_name());
    let mut out = Vec::new();
    for entry in files {
        let path = entry.path();
        let name = entry
            .file_name()
            .to_string_lossy()
            .trim_end_matches(".md")
            .to_string();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (description, argument_hint, body) = parse_frontmatter(&text);
        out.push(CommandDef {
            name,
            description,
            argument_hint,
            body,
        });
    }
    out
}

/// 解析命令 frontmatter（`description` / `argument-hint`）与正文。无 frontmatter → 全文为正文。
fn parse_frontmatter(text: &str) -> (String, Option<String>, String) {
    let Some(rest) = text.strip_prefix("---") else {
        return (String::new(), None, text.trim().to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (String::new(), None, text.trim().to_string());
    };
    let block = &rest[..end];
    let body = rest[end + 4..].trim().to_string();
    let mut description = String::new();
    let mut hint = None;
    for raw in block.lines() {
        let line = raw.trim();
        if let Some(v) = line.strip_prefix("description:") {
            description = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("argument-hint:") {
            hint = Some(v.trim().to_string());
        }
    }
    (description, hint, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, body: &str) -> CommandDef {
        CommandDef {
            name: name.into(),
            description: String::new(),
            argument_hint: None,
            body: body.into(),
        }
    }

    #[test]
    fn render_replaces_arguments_placeholder() {
        let d = def("translate", "把 $ARGUMENTS 翻译成英文");
        assert_eq!(d.render("你好"), "把 你好 翻译成英文");
        // 占位符存在则原样替换（不 trim）：两侧空格 + 替换文本 = 4 空格
        assert_eq!(d.render("  "), "把    翻译成英文");
    }

    #[test]
    fn render_appends_arguments_without_placeholder() {
        let d = def("review", "审查当前改动");
        assert_eq!(d.render("重点看安全性"), "审查当前改动\n\n重点看安全性");
        assert_eq!(d.render(""), "审查当前改动");
    }

    #[test]
    fn frontmatter_parsed_and_body_extracted() {
        let (desc, hint, body) = parse_frontmatter(
            "---\ndescription: 翻译助手\nargument-hint: <文本>\n---\n把 $ARGUMENTS 翻译成英文",
        );
        assert_eq!(desc, "翻译助手");
        assert_eq!(hint.as_deref(), Some("<文本>"));
        assert_eq!(body, "把 $ARGUMENTS 翻译成英文");
    }

    #[test]
    fn no_frontmatter_treats_whole_text_as_body() {
        let (desc, hint, body) = parse_frontmatter("直接是正文");
        assert_eq!(desc, "");
        assert!(hint.is_none());
        assert_eq!(body, "直接是正文");
    }

    #[test]
    fn merge_project_overrides_global() {
        let global = vec![def("a", "global-a"), def("b", "global-b")];
        let proj = vec![def("b", "proj-b"), def("c", "proj-c")];
        let merged = merge_commands(global, proj);
        let names: Vec<_> = merged.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        assert_eq!(merged[1].body, "proj-b");
    }

    #[test]
    fn read_dir_skips_non_md_and_sorts() {
        let dir = std::env::temp_dir().join(format!("stitch-cmd-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.md"), "b 命令").unwrap();
        std::fs::write(dir.join("a.md"), "---\ndescription: A\n---\na 命令").unwrap();
        std::fs::write(dir.join("note.txt"), "忽略").unwrap();
        let defs = read_commands_dir(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].name, "a");
        assert_eq!(defs[0].description, "A");
        assert_eq!(defs[0].body, "a 命令");
        assert_eq!(defs[1].name, "b");
    }
}
