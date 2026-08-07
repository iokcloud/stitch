//! Task 工具——子代理委派（Claude Code 语义）。
//!
//! 主代理调用 `Task(description, prompt, subagent_type?)` 把子任务委托给
//! 一个独立上下文（自己的 Session + 工具白名单 + 角色指令）的子代理，
//! 执行完毕把最终回复作为工具结果回收。
//!
//! 约束（简化决策）：
//! - 子代理不支持再委派：Task 工具不在子代理的工具集里（ctx.registry
//!   在注册 Task 前克隆，且克隆不包含 Task——同时避免 Arc 循环引用泄漏）。
//! - 子代理内工具不逐项确认（needs_confirmation=false）：deny 规则 /
//!   plan 模式等权限裁决在 execute_tool_with_renderer 里照常生效，
//!   Ask 分支自动放行。
//! - 子代理不响应桌面取消标志（运行中不可中断，回合结束自然终止）。

use crate::agent::{AgentEvent, ReactRenderer};
use crate::agents::SubAgentDef;
use crate::session::Session;
use crate::tools::{ToolDef, ToolRegistry, ToolResult};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc::UnboundedSender;

/// 子代理执行所需的运行时上下文（会话级，CLI / 桌面各自构造一次）。
pub struct SubagentCtx {
    pub api_base: String,
    pub model: String,
    /// Mutex：桌面端 api_key 回合内解析（resolve_llm_key），构建后注入。
    pub api_key: std::sync::Mutex<String>,
    pub max_iterations: usize,
    pub work_dir: Option<String>,
    /// 可用子代理定义（/agents 与 Task 的 subagent_type 共用）。
    pub agents: Vec<SubAgentDef>,
    /// 子代理嵌套深度计数器（防御：当前不支持再委派，恒 ≤1）。
    pub depth: AtomicUsize,
    /// 嵌套上限（保留给未来放开再委派用）。
    pub max_depth: usize,
    /// 事件转发出口：Some 时子代理内部事件（ToolStart/ToolDone/Token…）
    /// 原样转发给主渲染通道（桌面 / stream-json）。CLI 终端渲染为 None。
    pub event_tx: std::sync::Mutex<Option<UnboundedSender<AgentEvent>>>,
    /// 不含 Task 工具的工具注册表（子代理白名单过滤的基底）。
    pub registry: ToolRegistry,
}

impl SubagentCtx {
    pub fn set_event_tx(&self, tx: UnboundedSender<AgentEvent>) {
        if let Ok(mut guard) = self.event_tx.lock() {
            *guard = Some(tx);
        }
    }

    /// 桌面端回合内注入 api_key（构建时未知，resolve 后设置）。
    pub fn set_api_key(&self, key: &str) {
        if let Ok(mut guard) = self.api_key.lock() {
            *guard = key.to_string();
        }
    }
}

/// Task 工具实现。
#[derive(Clone)]
pub struct TaskSubagent {
    pub ctx: Arc<SubagentCtx>,
}

impl TaskSubagent {
    pub fn definition() -> ToolDef {
        ToolDef {
            name: "Task".into(),
            description: "将子任务委托给一个子代理。子代理拥有独立上下文、角色指令与工具白名单，\
                 执行完毕后返回其最终回复。适合：并行研究、大文件独立改写、\
                 与主任务无耦合的独立子任务。"
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "description": {
                        "type": "string",
                        "description": "子任务的一句话描述（显示用途，模型需能从描述判断何时需要委派）"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "委派给子代理的完整任务指令"
                    },
                    "subagent_type": {
                        "type": "string",
                        "description": "要使用的子代理名（.claude/agents/*.md 定义；省略 = 通用子代理，全部工具）"
                    }
                },
                "required": ["description", "prompt"]
            }),
        }
    }

    /// 执行子代理委派。无网络/无工具执行的路径可单测；完整链路依赖真实 LLM。
    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let description = arguments["description"].as_str().unwrap_or("").to_string();
        let prompt = arguments["prompt"].as_str().unwrap_or("").to_string();
        if prompt.is_empty() {
            return Ok(ToolResult::fail(
                "Task 工具缺少必填参数 `prompt`（子代理任务指令）",
            ));
        }
        let agent_name = arguments["subagent_type"].as_str().map(str::to_string);

        // 嵌套深度检查（防御）
        let depth = self.ctx.depth.fetch_add(1, Ordering::SeqCst) + 1;
        if depth > self.ctx.max_depth {
            self.ctx.depth.fetch_sub(1, Ordering::SeqCst);
            return Ok(ToolResult::fail(format!(
                "子代理嵌套深度已达上限 {depth}/{max_depth}（子代理不支持再委派 Task）",
                max_depth = self.ctx.max_depth
            )));
        }

        let fwd = self.ctx.event_tx.lock().ok().and_then(|g| g.clone());

        let result = self
            .run_subagent(&agent_name, &description, &prompt, fwd.as_ref())
            .await;

        self.ctx.depth.fetch_sub(1, Ordering::SeqCst);
        result
    }

    async fn run_subagent(
        &self,
        agent_name: &Option<String>,
        description: &str,
        prompt: &str,
        fwd: Option<&UnboundedSender<AgentEvent>>,
    ) -> anyhow::Result<ToolResult> {
        let ctx = &self.ctx;
        let api_key = ctx
            .api_key
            .lock()
            .ok()
            .map(|g| g.clone())
            .unwrap_or_default();

        // 子代理类型：名字匹配定义；省略 = 通用子代理（全部工具、无额外指令）。
        let def = agent_name
            .as_ref()
            .and_then(|n| ctx.agents.iter().find(|a| &a.name == n));
        let model = def
            .and_then(|d| d.model.as_deref())
            .unwrap_or(&ctx.model)
            .to_string();

        // 工具白名单过滤
        let sub_registry = match def {
            Some(d) => crate::agents::filter_registry(d, &ctx.registry),
            None => ctx.registry.clone(),
        };
        let sub_names: Vec<String> = sub_registry
            .definitions()
            .into_iter()
            .map(|d| d.name)
            .collect();

        // 系统提示：基础提示 + 子代理角色指令
        let base_prompt = crate::agent::prompt::build_system_prompt(
            ctx.work_dir.as_deref().unwrap_or("."),
            &sub_registry,
        );
        let system_prompt = match def {
            Some(d) => format!(
                "{base_prompt}\n\n## 子代理角色：{name}\n{desc}\n\n## 角色指令\n{instructions}",
                name = d.name,
                desc = if d.description.is_empty() {
                    "（无描述）"
                } else {
                    &d.description
                },
                instructions = if d.instructions.is_empty() {
                    "按基础规则执行委派任务。"
                } else {
                    &d.instructions
                },
            ),
            None => base_prompt,
        };

        if let Some(tx) = fwd {
            let _ = tx.send(AgentEvent::SubagentStart {
                name: def
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| "general".into()),
                description: description.to_string(),
                tools: sub_names,
            });
        }

        let mut sub_session = Session::new(system_prompt);
        sub_session.add_user_message(prompt);

        let sub_name = def
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "general".into());
        let hooks = crate::hooks::HookRegistry::load(ctx.work_dir.as_deref());

        let mut renderer = SubagentRenderer { fwd: fwd.cloned() };
        let outcome = crate::agent::run_react_core(
            &mut sub_session,
            &ctx.api_base,
            &model,
            &api_key,
            &sub_registry,
            ctx.max_iterations,
            ctx.work_dir.as_deref(),
            &mut renderer,
        )
        .await;

        match outcome {
            Ok(result) => {
                let summary = format!(
                    "子代理完成（{} 轮，{} tokens）：{}",
                    result.iterations, result.tokens_used, description
                );
                if let Some(tx) = fwd {
                    let _ = tx.send(AgentEvent::SubagentDone {
                        name: sub_name.clone(),
                        success: true,
                        summary: summary.clone(),
                    });
                }
                // SubagentStop hook：委派结束通知（Claude Code 语义）
                if hooks.has(crate::hooks::HookEvent::SubagentStop) {
                    let _ = hooks
                        .run(
                            crate::hooks::HookEvent::SubagentStop,
                            "subagent",
                            &serde_json::json!({
                                "subagent": sub_name,
                                "success": true,
                                "summary": summary,
                            }),
                            None,
                        )
                        .await;
                }
                Ok(ToolResult::ok(format!(
                    "子代理执行完成。\n\n返回结果：\n{}",
                    result.response
                )))
            }
            Err(e) => {
                if let Some(tx) = fwd {
                    let _ = tx.send(AgentEvent::SubagentDone {
                        name: sub_name.clone(),
                        success: false,
                        summary: e.to_string(),
                    });
                }
                if hooks.has(crate::hooks::HookEvent::SubagentStop) {
                    let _ = hooks
                        .run(
                            crate::hooks::HookEvent::SubagentStop,
                            "subagent",
                            &serde_json::json!({
                                "subagent": sub_name,
                                "success": false,
                                "summary": e.to_string(),
                            }),
                            None,
                        )
                        .await;
                }
                Ok(ToolResult::fail(format!("子代理执行失败：{e:#}")))
            }
        }
    }
}

/// 子代理渲染器：事件转发给主通道；确认门恒放行；无取消/无落盘。
pub struct SubagentRenderer {
    fwd: Option<UnboundedSender<AgentEvent>>,
}

impl ReactRenderer for SubagentRenderer {
    fn on_interim_text(&mut self, _text: &str) {}
    fn on_event(&mut self, ev: AgentEvent) {
        if let Some(tx) = &self.fwd {
            let _ = tx.send(ev);
        }
    }
    async fn confirm_tool(&mut self, _tool: &str, _call_id: &str, _message: &str) -> bool {
        true
    }
    fn is_cancelled(&self) -> bool {
        false
    }
    fn flush_turn(&mut self, _session: &Session) {}
    fn needs_confirmation(
        &self,
        _tool: &crate::tools::Tool,
        _args: &serde_json::Value,
        _work_dir: Option<&str>,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn ctx() -> Arc<SubagentCtx> {
        let base = crate::tools::build_registry(".");
        Arc::new(SubagentCtx {
            api_base: "http://127.0.0.1:9".into(), // 连接拒绝——网络路径不测试
            model: "test-model".into(),
            api_key: std::sync::Mutex::new("k".into()),
            max_iterations: 2,
            work_dir: Some(".".into()),
            agents: vec![SubAgentDef {
                name: "researcher".into(),
                description: "研究型子代理".into(),
                tools: Some(vec!["read_file".into()]),
                model: None,
                instructions: "你只做研究，不写代码。".into(),
            }],
            depth: AtomicUsize::new(0),
            max_depth: 2,
            event_tx: std::sync::Mutex::new(None),
            registry: base,
        })
    }

    #[tokio::test]
    async fn definition_schema_matches_claude_code() {
        let def = TaskSubagent::definition();
        assert_eq!(def.name, "Task");
        let props = def.parameters["properties"].as_object().unwrap();
        assert!(props.contains_key("description"));
        assert!(props.contains_key("prompt"));
        assert!(props.contains_key("subagent_type"));
        let required = def.parameters["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
        assert!(required.iter().any(|v| v == "description"));
        assert!(required.iter().any(|v| v == "prompt"));
    }

    #[tokio::test]
    async fn missing_prompt_fails_fast() {
        let t = TaskSubagent { ctx: ctx() };
        let r = t
            .execute(serde_json::json!({"description": "x"}))
            .await
            .unwrap();
        assert!(!r.success);
        assert!(r.output.contains("prompt"));
    }

    #[tokio::test]
    async fn depth_limit_blocks_nesting() {
        let c = ctx();
        c.depth.store(2, Ordering::SeqCst); // 已达上限
        let t = TaskSubagent { ctx: c };
        let r = t
            .execute(serde_json::json!({"description": "x", "prompt": "do it"}))
            .await
            .unwrap();
        assert!(!r.success);
        assert!(r.output.contains("嵌套深度"));
        // 失败后计数器归还（不会越减越小）
        assert_eq!(t.ctx.depth.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn unknown_agent_falls_back_to_general() {
        // 未知 subagent_type → 通用子代理（不报错，进网络路径）——
        // 这里验证参数层不因名字失败；连接拒绝会走 run_react_core 报错
        let t = TaskSubagent { ctx: ctx() };
        let r = t
            .execute(serde_json::json!({
                "description": "x",
                "prompt": "do it",
                "subagent_type": "ghost"
            }))
            .await
            .unwrap();
        // 网络错误（连接拒绝）→ fail，但错误信息是执行失败而非参数问题
        assert!(!r.success);
        assert!(r.output.contains("子代理执行失败"));
    }
}
