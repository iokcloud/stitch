//! Todo 工具——任务清单（Claude Code 语义）。
//!
//! 主代理用 `TodoWrite` 工具维护长任务的多步清单：add / update /
//! remove / clear + 状态（in_progress / completed / not_started）。
//! 状态存在会话级 `TodoStore`（Arc<Mutex<…>>）：CLI 的 `/todo` 命令与
//! 回合收尾的进度行共用同一实例；桌面端注册的是独立实例（工具可用，
//! 无专用 UI）。

use super::{ToolDef, ToolResult};
use std::sync::{Arc, Mutex};

/// 清单上限（防止模型无脑堆砌）。
const MAX_TODOS: usize = 50;

/// 单个待办项。
#[derive(Debug, Clone)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub done: bool,
    pub in_progress: bool,
}

/// 任务清单存储（会话级，可变共享）。
#[derive(Debug, Default)]
pub struct TodoStore {
    items: Vec<TodoItem>,
    next_id: u64,
}

impl TodoStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 新增任务，返回 id（数字字符串，从 1 自增）。
    pub fn add(&mut self, content: String) -> Option<String> {
        if content.trim().is_empty() || self.items.len() >= MAX_TODOS {
            return None;
        }
        self.next_id += 1;
        let id = self.next_id.to_string();
        self.items.push(TodoItem {
            id: id.clone(),
            content,
            done: false,
            in_progress: false,
        });
        Some(id)
    }

    /// 改任务描述。
    pub fn update(&mut self, id: &str, content: String) -> bool {
        match self.items.iter_mut().find(|i| i.id == id) {
            Some(item) if !content.trim().is_empty() => {
                item.content = content;
                true
            }
            _ => false,
        }
    }

    /// 设完成状态（done = true → completed；false → not_started 并清 in_progress）。
    pub fn set_done(&mut self, id: &str, done: bool) -> bool {
        match self.items.iter_mut().find(|i| i.id == id) {
            Some(item) => {
                item.done = done;
                if done {
                    item.in_progress = false;
                }
                true
            }
            None => false,
        }
    }

    /// 设进行中状态（互斥：进行中时 done 为 false）。
    pub fn set_in_progress(&mut self, id: &str, in_progress: bool) -> bool {
        match self.items.iter_mut().find(|i| i.id == id) {
            Some(item) => {
                item.in_progress = in_progress;
                if in_progress {
                    item.done = false;
                }
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        self.items.len() != before
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn list(&self) -> Vec<TodoItem> {
        self.items.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn done_count(&self) -> usize {
        self.items.iter().filter(|i| i.done).count()
    }
}

/// TodoWrite 工具实现。
#[derive(Clone)]
pub struct TodoWrite {
    store: Arc<Mutex<TodoStore>>,
}

impl Default for TodoWrite {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoWrite {
    pub fn new() -> Self {
        Self::with_store(Arc::new(Mutex::new(TodoStore::new())))
    }

    /// CLI 传入会话级共享存储（/todo 命令与回合进度行共用）。
    pub fn with_store(store: Arc<Mutex<TodoStore>>) -> Self {
        Self { store }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "TodoWrite".into(),
            description: "维护任务清单（长任务拆成多步时用，让用户看到整体进度）。\
                 operation：add（需 content，返回新任务 id）、update（需 id + content）、\
                 remove（需 id）、clear（清空）；status：completed / in_progress / \
                 not_started 更新指定 id 的状态。当前清单可用无 operation 的调用查看。"
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": {
                        "type": "string",
                        "enum": ["add", "update", "remove", "clear", "list"],
                        "description": "要执行的操作"
                    },
                    "id": {
                        "type": "string",
                        "description": "任务 id（add 返回；update/remove/status 用）"
                    },
                    "content": {
                        "type": "string",
                        "description": "任务描述（add 必填；update 改描述）"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["in_progress", "completed", "not_started"],
                        "description": "状态更新（对指定 id）"
                    }
                },
                "required": ["operation"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let op = arguments["operation"].as_str().unwrap_or("list");
        let mut store = match self.store.lock() {
            Ok(s) => s,
            Err(_) => return Ok(ToolResult::fail("todo 存储锁不可用")),
        };

        match op {
            "add" => {
                let content = arguments["content"].as_str().unwrap_or("").to_string();
                match store.add(content.clone()) {
                    Some(id) => Ok(ToolResult::ok(format!("已添加任务 {id}：{content}"))),
                    None => Ok(ToolResult::fail(
                        "添加失败：内容为空或清单已达上限（50 项）",
                    )),
                }
            }
            "update" => {
                let id = arguments["id"].as_str().unwrap_or("").to_string();
                let content = arguments["content"].as_str().unwrap_or("").to_string();
                if store.update(&id, content.clone()) {
                    Ok(ToolResult::ok(format!("已更新任务 {id}：{content}")))
                } else {
                    Ok(ToolResult::fail(format!(
                        "找不到任务 {id}（/todo 查看清单）"
                    )))
                }
            }
            "remove" => {
                let id = arguments["id"].as_str().unwrap_or("").to_string();
                if store.remove(&id) {
                    Ok(ToolResult::ok(format!("已删除任务 {id}")))
                } else {
                    Ok(ToolResult::fail(format!("找不到任务 {id}")))
                }
            }
            "clear" => {
                store.clear();
                Ok(ToolResult::ok("任务清单已清空"))
            }
            _ => {
                // list（或未知操作）：返回当前清单，让模型跟踪进度
                if store.is_empty() {
                    return Ok(ToolResult::ok("任务清单为空"));
                }
                let lines: Vec<String> = store
                    .list()
                    .iter()
                    .map(|i| {
                        let mark = if i.done {
                            "[x]"
                        } else if i.in_progress {
                            "[▶]"
                        } else {
                            "[ ]"
                        };
                        format!("{mark} {id}: {content}", id = i.id, content = i.content)
                    })
                    .collect();
                Ok(ToolResult::ok(format!(
                    "任务清单（{}/{} 完成）：\n{}",
                    store.done_count(),
                    store.list().len(),
                    lines.join("\n")
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_ops_lifecycle() {
        let mut s = TodoStore::new();
        let id1 = s.add("写 README".into()).unwrap();
        let id2 = s.add("跑测试".into()).unwrap();
        assert_eq!(id1, "1");
        assert_eq!(id2, "2");
        assert_eq!(s.list().len(), 2);
        // 状态互斥
        s.set_in_progress(&id1, true);
        assert!(s.list()[0].in_progress);
        s.set_done(&id1, true);
        assert!(s.list()[0].done);
        assert!(!s.list()[0].in_progress);
        // update / remove
        assert!(s.update(&id2, "跑全量测试".into()));
        assert_eq!(s.list()[1].content, "跑全量测试");
        assert!(s.remove(&id2));
        assert!(!s.remove(&id2)); // 幂等
        assert_eq!(s.done_count(), 1);
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn add_rejects_empty_and_caps() {
        let mut s = TodoStore::new();
        assert!(s.add("  ".into()).is_none());
        for i in 0..MAX_TODOS {
            assert!(s.add(format!("任务 {i}")).is_some());
        }
        assert!(s.add("超限".into()).is_none());
    }

    #[test]
    fn definition_schema() {
        let def = TodoWrite::new().definition();
        assert_eq!(def.name, "TodoWrite");
        let props = def.parameters["properties"].as_object().unwrap();
        assert!(props.contains_key("operation"));
        assert!(props.contains_key("id"));
        assert!(props.contains_key("content"));
        assert!(props.contains_key("status"));
    }

    #[tokio::test]
    async fn execute_add_and_list() {
        let t = TodoWrite::new();
        let r = t
            .execute(serde_json::json!({"operation": "add", "content": "第一步"}))
            .await
            .unwrap();
        assert!(r.success);
        assert!(r.output.contains("1"));
        let r2 = t
            .execute(serde_json::json!({"operation": "list"}))
            .await
            .unwrap();
        assert!(r2.output.contains("第一步"));
        assert!(r2.output.contains("0/1"));
    }

    #[tokio::test]
    async fn execute_missing_id_fails() {
        let t = TodoWrite::new();
        let r = t
            .execute(serde_json::json!({"operation": "update", "id": "9", "content": "x"}))
            .await
            .unwrap();
        assert!(!r.success);
        assert!(r.output.contains("找不到"));
    }
}
