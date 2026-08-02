//! Save desktop automation workflow steps as a reusable Skill.

use super::{ToolDef, ToolResult};
use crate::agent::persist::JsonlRecord;
use crate::session::{Message, Role};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct DesktopStep {
    tool_name: String,
    arguments: String,
    success: bool,
    /// First ~120 chars of the tool result output (for readable SKILL.md).
    result_summary: Option<String>,
}

#[derive(Clone)]
pub struct SaveSkill {
    work_dir: PathBuf,
}

impl SaveSkill {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "save_skill".into(),
            description:
                "Save the current session's desktop automation steps as a reusable Skill. \
                 Only includes successful desktop_* tool calls from the latest session."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Skill slug (directory name), e.g. excel-report"
                    },
                    "title": {
                        "type": "string",
                        "description": "Display name, e.g. Excel report generation"
                    },
                    "description": {
                        "type": "string",
                        "description": "One-line description of what this skill does"
                    }
                },
                "required": ["name", "title", "description"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let name = arguments["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?
            .trim();
        let title = arguments["title"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'title' argument"))?
            .trim();
        let description = arguments["description"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'description' argument"))?
            .trim();

        if name.is_empty() || title.is_empty() || description.is_empty() {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: "参数 name、title、description 均不能为空".into(),
            });
        }

        let messages_path = match find_latest_messages_jsonl(&self.work_dir) {
            Ok(Some(p)) => p,
            Ok(None) => {
                return Ok(ToolResult { metrics: None,
                    success: false,
                    output: "未找到会话记录。请先完成一次对话，或确认工作区下存在 .stitch/sessions/ 目录。"
                        .into(),
                });
            }
            Err(e) => {
                return Ok(ToolResult {
                    metrics: None,
                    success: false,
                    output: format!("读取会话失败：{e}"),
                });
            }
        };

        let messages = match read_messages_jsonl(&messages_path) {
            Ok(msgs) => msgs,
            Err(e) => {
                return Ok(ToolResult {
                    metrics: None,
                    success: false,
                    output: format!("解析会话消息失败：{e}"),
                });
            }
        };

        let steps = extract_desktop_steps(&messages);
        let skill_dir = self.work_dir.join(".agents").join("skills").join(name);
        let skill_path = skill_dir.join("SKILL.md");
        let existed = skill_path.is_file();

        let content = render_skill_md(title, description, &steps);
        fs::create_dir_all(&skill_dir)?;
        fs::write(&skill_path, content)?;

        let mut output = format!("Skill 已保存到 .agents/skills/{name}/SKILL.md");
        if existed {
            output.push_str("（已覆盖同名 Skill）");
        }

        Ok(ToolResult {
            metrics: None,
            success: true,
            output,
        })
    }
}

fn sessions_root(work_dir: &Path) -> PathBuf {
    work_dir.join(".stitch").join("sessions")
}

fn find_latest_messages_jsonl(work_dir: &Path) -> anyhow::Result<Option<PathBuf>> {
    let root = sessions_root(work_dir);
    if !root.is_dir() {
        return Ok(None);
    }

    let mut latest: Option<(PathBuf, std::time::SystemTime)> = None;
    for ent in fs::read_dir(&root)? {
        let ent = ent?;
        if !ent.file_type()?.is_dir() {
            continue;
        }
        let msg_path = ent.path().join("messages.jsonl");
        if !msg_path.is_file() {
            continue;
        }
        let modified = fs::metadata(&msg_path)?.modified()?;
        if latest.as_ref().is_none_or(|(_, prev)| modified > *prev) {
            latest = Some((msg_path, modified));
        }
    }
    Ok(latest.map(|(p, _)| p))
}

fn read_messages_jsonl(path: &Path) -> anyhow::Result<Vec<Message>> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let rec: JsonlRecord = serde_json::from_str(line)?;
        out.push(rec.msg);
    }
    Ok(out)
}

fn tool_result_success(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|v| v.get("success").and_then(|s| s.as_bool()))
        .unwrap_or(true)
}

fn extract_desktop_steps(messages: &[Message]) -> Vec<DesktopStep> {
    // First pass: collect tool result success + output text by call_id.
    let mut results: HashMap<&str, (bool, Option<String>)> = HashMap::new();
    for msg in messages {
        if msg.role == Role::Tool
            && let Some(id) = msg.tool_call_id.as_deref()
        {
            let ok = tool_result_success(msg.content.text());
            // Extract the "output" field from the tool result JSON as summary.
            let summary = extract_output_field(msg.content.text(), 120);
            results.insert(id, (ok, summary));
        }
    }

    let mut steps = Vec::new();
    for msg in messages {
        if msg.role != Role::Assistant {
            continue;
        }
        let Some(tool_calls) = &msg.tool_calls else {
            continue;
        };
        for tc in tool_calls {
            let name = &tc.function.name;
            if !name.starts_with("desktop_") {
                continue;
            }
            let (success, summary) = results
                .get(tc.id.as_str())
                .cloned()
                .unwrap_or((false, None));
            if !success {
                continue;
            }
            steps.push(DesktopStep {
                tool_name: name.clone(),
                arguments: tc.function.arguments.clone(),
                success,
                result_summary: summary,
            });
        }
    }
    steps
}

/// Extract up to `max_chars` from the `"output"` field of a tool-result JSON string.
fn extract_output_field(raw: &str, max_chars: usize) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let output = v.get("output")?.as_str()?;
    if output.is_empty() {
        return None;
    }
    // Take first line or up to max_chars.
    let first_line = output.lines().next().unwrap_or(output);
    let trimmed: String = first_line.chars().take(max_chars).collect();
    if trimmed.is_empty() {
        None
    } else if trimmed.len() < first_line.len() {
        Some(format!("{trimmed}…"))
    } else {
        Some(trimmed)
    }
}

fn render_skill_md(title: &str, description: &str, steps: &[DesktopStep]) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {title}\n"));
    out.push_str(&format!("description: {description}\n"));
    out.push_str("---\n\n");
    out.push_str(&format!("# {title}\n\n"));
    out.push_str(description);
    out.push_str("\n\n## 前置条件\n\n<!-- 运行此 Skill 前需要准备的环境或应用 -->\n\n");
    out.push_str("## 操作步骤\n\n");
    for (i, step) in steps.iter().enumerate() {
        out.push_str(&format!(
            "{}. `{}` — 参数: `{}`\n",
            i + 1,
            step.tool_name,
            step.arguments,
        ));
        if let Some(ref summary) = step.result_summary {
            out.push_str(&format!("   → {summary}\n"));
        }
    }
    out.push_str("\n## 预期结果\n\n<!-- 执行完成后应达成的状态 -->\n\n");
    out.push_str("## 使用方式\n\n");
    out.push_str(&format!(
        "在聊天中通过「+」菜单选择此 Skill，或直接说「用 {title} skill」。\n"
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::thread;
    use std::time::Duration;

    fn write_session_jsonl(work_dir: &Path, session_id: &str, lines: &[&str]) {
        let dir = work_dir.join(".stitch").join("sessions").join(session_id);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("messages.jsonl");
        let mut f = fs::File::create(path).unwrap();
        for line in lines {
            writeln!(f, "{line}").unwrap();
        }
    }

    #[tokio::test]
    async fn no_sessions_dir_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = SaveSkill::new(tmp.path());
        let result = tool
            .execute(serde_json::json!({
                "name": "test-skill",
                "title": "Test Skill",
                "description": "A test skill"
            }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.output.contains("未找到会话记录"));
    }

    #[tokio::test]
    async fn generates_skill_md_with_desktop_tools() {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path();

        write_session_jsonl(
            work,
            "sess-1",
            &[
                r#"{"ts":"2026-01-01T00:00:00Z","role":"assistant","content":"","tool_calls":[{"id":"call1","type":"function","function":{"name":"desktop_click","arguments":"{\"x\":100,\"y\":200}"}}]}"#,
                r#"{"ts":"2026-01-01T00:00:01Z","role":"tool","content":"{\"success\":true,\"output\":\"clicked\"}","tool_call_id":"call1"}"#,
                r#"{"ts":"2026-01-01T00:00:02Z","role":"assistant","content":"","tool_calls":[{"id":"call2","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"foo.txt\"}"}}]}"#,
                r#"{"ts":"2026-01-01T00:00:03Z","role":"tool","content":"{\"success\":true,\"output\":\"file\"}","tool_call_id":"call2"}"#,
                r#"{"ts":"2026-01-01T00:00:04Z","role":"assistant","content":"","tool_calls":[{"id":"call3","type":"function","function":{"name":"desktop_type","arguments":"{\"text\":\"hello\"}"}}]}"#,
                r#"{"ts":"2026-01-01T00:00:05Z","role":"tool","content":"{\"success\":false,\"output\":\"failed\"}","tool_call_id":"call3"}"#,
            ],
        );

        // Older session — should not be picked when a newer messages.jsonl exists.
        write_session_jsonl(
            work,
            "sess-old",
            &[r#"{"ts":"2020-01-01T00:00:00Z","role":"user","content":"old"}"#],
        );
        thread::sleep(Duration::from_millis(50));

        write_session_jsonl(
            work,
            "sess-2",
            &[
                r#"{"ts":"2026-01-01T00:00:00Z","role":"assistant","content":"","tool_calls":[{"id":"call1","type":"function","function":{"name":"desktop_click","arguments":"{\"x\":100,\"y\":200}"}}]}"#,
                r#"{"ts":"2026-01-01T00:00:01Z","role":"tool","content":"{\"success\":true,\"output\":\"clicked\"}","tool_call_id":"call1"}"#,
            ],
        );

        let tool = SaveSkill::new(work);
        let result = tool
            .execute(serde_json::json!({
                "name": "excel-report",
                "title": "Excel 报表生成",
                "description": "自动填写 Excel 报表"
            }))
            .await
            .unwrap();

        assert!(result.success, "{}", result.output);
        let skill_path = work
            .join(".agents")
            .join("skills")
            .join("excel-report")
            .join("SKILL.md");
        assert!(skill_path.is_file(), "SKILL.md should exist");

        let body = fs::read_to_string(skill_path).unwrap();
        assert!(body.contains("desktop_click"));
        assert!(!body.contains("read_file"));
        assert!(!body.contains("desktop_type"));
        assert!(body.contains("Excel 报表生成"));
        assert!(body.contains("自动填写 Excel 报表"));
    }

    #[tokio::test]
    async fn overwrites_existing_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path();

        write_session_jsonl(
            work,
            "sess-1",
            &[
                r#"{"ts":"2026-01-01T00:00:00Z","role":"assistant","content":"","tool_calls":[{"id":"c1","type":"function","function":{"name":"desktop_scroll","arguments":"{\"dy\":-3}"}}]}"#,
                r#"{"ts":"2026-01-01T00:00:01Z","role":"tool","content":"{\"success\":true,\"output\":\"ok\"}","tool_call_id":"c1"}"#,
            ],
        );

        let skill_dir = work.join(".agents").join("skills").join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "old content").unwrap();

        let tool = SaveSkill::new(work);
        let result = tool
            .execute(serde_json::json!({
                "name": "my-skill",
                "title": "My Skill",
                "description": "Updated"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("已覆盖"));
        let body = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert!(body.contains("desktop_scroll"));
        assert!(!body.contains("old content"));
    }
}
