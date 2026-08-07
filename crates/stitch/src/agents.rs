//! 子代理定义（Claude Code `.claude/agents/*.md` 语义）。
//!
//! 子代理 = Markdown 文件，frontmatter 声明元信息，正文为角色指令：
//!
//! ```md
//! ---
//! name: my-agent
//! description: 负责 X 的专职代理
//! tools: read_file, write_file
//! model: deepseek-v4-flash
//! ---
//! 这里是子代理的角色指令正文…
//! ```
//!
//! 加载优先级：项目 `<work_dir>/.claude/agents/*.md` 覆盖同名全局
//! `<config_dir>/agents/*.md`（Claude Code：`~/.claude/agents/`）。
//!
//! frontmatter 手写极简解析（无需 YAML 依赖）——只支持本项目需要的
//! 四字段；`tools` 兼容「逗号分隔」与「YAML 列表」两种写法。

use crate::tools::ToolRegistry;
use std::collections::HashMap;

/// 一个子代理定义。
#[derive(Debug, Clone)]
pub struct SubAgentDef {
    /// 子代理名（Task 工具的 `subagent_type` 引用它）。
    pub name: String,
    /// 一句话职责描述（模型选择子代理时参考）。
    pub description: String,
    /// 工具白名单（`tools` 字段省略 = 全部工具可用）。
    pub tools: Option<Vec<String>>,
    /// 模型覆盖（省略 = 继承主会话模型）。
    pub model: Option<String>,
    /// frontmatter 之后的正文 = 角色指令。
    pub instructions: String,
}

#[derive(Debug, Default)]
struct AgentFrontmatter {
    name: Option<String>,
    description: Option<String>,
    tools: Option<Vec<String>>,
    model: Option<String>,
}

/// 解析 frontmatter：首行 `---` 与下一行 `---` 之间的 `key: value` 块。
/// 无 frontmatter（不以 `---` 开头）→ None。
fn parse_frontmatter(text: &str) -> Option<AgentFrontmatter> {
    let rest = text.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    let block = &rest[..end];

    let mut fm = AgentFrontmatter::default();
    let mut collecting_list = false;
    let mut list: Vec<String> = Vec::new();

    for raw in block.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if collecting_list {
            if let Some(item) = line.strip_prefix("- ") {
                let item = item.trim();
                if !item.is_empty() {
                    list.push(item.to_string());
                }
                continue;
            }
            // 列表结束：落盘并回到普通解析
            if !list.is_empty() {
                fm.tools = Some(std::mem::take(&mut list));
            }
            collecting_list = false;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim().to_ascii_lowercase();
        let value = v.trim();
        match key.as_str() {
            "name" => fm.name = Some(value.to_string()),
            "description" => fm.description = Some(value.to_string()),
            "model" => fm.model = Some(value.to_string()),
            "tools" => {
                if value.is_empty() {
                    // YAML 列表形式：
                    // tools:
                    //   - read_file
                    collecting_list = true;
                    list = Vec::new();
                } else {
                    // 逗号分隔形式：tools: read_file, write_file
                    fm.tools = Some(
                        value
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect(),
                    );
                }
            }
            _ => {}
        }
    }
    if collecting_list && !list.is_empty() {
        fm.tools = Some(list);
    }
    Some(fm)
}

/// 从目录加载 `*.md` 子代理定义（跳过解析失败的条目）。
fn load_from_dir(dir: &std::path::Path, map: &mut HashMap<String, SubAgentDef>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(fm) = parse_frontmatter(&text) else {
            continue;
        };
        let Some(name) = fm.name else {
            continue;
        };
        let content = text
            .split_once("\n---")
            .map(|(_, after)| after)
            .unwrap_or(&text)
            .trim()
            .to_string();
        map.insert(
            name.clone(),
            SubAgentDef {
                name,
                description: fm.description.unwrap_or_default(),
                tools: fm.tools,
                model: fm.model,
                instructions: content,
            },
        );
    }
}

/// 加载全部子代理定义：全局 `config_dir/agents/` + 项目 `.claude/agents/`
/// （项目同名覆盖全局）。按名称排序返回。
pub fn load_agents(work_dir: Option<&str>) -> Vec<SubAgentDef> {
    let mut map: HashMap<String, SubAgentDef> = HashMap::new();
    load_from_dir(&crate::config::config_dir().join("agents"), &mut map);
    if let Some(wd) = work_dir {
        load_from_dir(
            &std::path::Path::new(wd).join(".claude").join("agents"),
            &mut map,
        );
    }
    let mut defs: Vec<SubAgentDef> = map.into_values().collect();
    defs.sort_by(|a, b| a.name.cmp(&b.name));
    defs
}

/// 按白名单过滤工具注册表（`tools` 省略 = 原样复用）。
pub fn filter_registry(def: &SubAgentDef, base: &ToolRegistry) -> ToolRegistry {
    let mut filtered = ToolRegistry::new();
    match &def.tools {
        Some(list) => {
            for name in list {
                if let Some(tool) = base.get(name) {
                    filtered.register(tool.clone());
                }
            }
        }
        None => {
            for d in base.definitions() {
                if let Some(tool) = base.get(&d.name) {
                    filtered.register(tool.clone());
                }
            }
        }
    }
    filtered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_frontmatter() {
        let text = "---\nname: my-agent\ndescription: 负责 X\nmodel: deepseek-v4-flash\n---\n\n按项目规范工作。";
        let fm = parse_frontmatter(text).expect("frontmatter");
        assert_eq!(fm.name.as_deref(), Some("my-agent"));
        assert_eq!(fm.description.as_deref(), Some("负责 X"));
        assert_eq!(fm.model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(fm.tools, None);
    }

    #[test]
    fn parses_comma_tools() {
        let text = "---\nname: a\ntools: read_file, write_file\n---\nbody";
        let fm = parse_frontmatter(text).expect("frontmatter");
        assert_eq!(
            fm.tools,
            Some(vec!["read_file".into(), "write_file".into()])
        );
    }

    #[test]
    fn parses_yaml_list_tools() {
        let text = "---\nname: a\ntools:\n  - read_file\n  - search_code\n---\nbody";
        let fm = parse_frontmatter(text).expect("frontmatter");
        assert_eq!(
            fm.tools,
            Some(vec!["read_file".into(), "search_code".into()])
        );
    }

    #[test]
    fn empty_tools_list_parses_as_none() {
        let text = "---\nname: a\ntools:\n---\nbody";
        let fm = parse_frontmatter(text).expect("frontmatter");
        // 空列表 = 无白名单（Claude Code 语义：tools 省略或空 = 全部工具）
        assert_eq!(fm.tools, None);
    }

    #[test]
    fn missing_frontmatter_returns_none() {
        assert!(parse_frontmatter("plain text, no frontmatter").is_none());
    }

    #[test]
    fn instructions_exclude_frontmatter() {
        let text = "---\nname: a\n---\n\n正文第一行\n正文第二行";
        let fm = parse_frontmatter(text).expect("frontmatter");
        // 正文 = frontmatter 之后（trim）
        let content = text.split_once("\n---").map(|(_, a)| a).unwrap_or(&text);
        assert_eq!(content.trim(), "正文第一行\n正文第二行");
        let _ = fm;
    }

    #[test]
    fn filter_registry_whitelist_and_full() {
        // 白名单：只保留命中的工具
        let mut base = ToolRegistry::new();
        base.register(crate::tools::Tool::ReadFile(
            crate::tools::file::ReadFile::new("."),
        ));
        base.register(crate::tools::Tool::WriteFile(
            crate::tools::file::WriteFile::new("."),
        ));
        let def = SubAgentDef {
            name: "x".into(),
            description: String::new(),
            tools: Some(vec!["read_file".into()]),
            model: None,
            instructions: String::new(),
        };
        let filtered = filter_registry(&def, &base);
        let names: Vec<String> = filtered.definitions().into_iter().map(|d| d.name).collect();
        assert_eq!(names, vec!["read_file"]);

        // tools 省略：全部工具
        let def_full = SubAgentDef {
            tools: None,
            ..def.clone()
        };
        let full = filter_registry(&def_full, &base);
        assert_eq!(full.definitions().len(), 2);
    }
}
