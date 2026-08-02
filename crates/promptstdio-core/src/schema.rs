//! 与 `docs/schemas/` 对齐的运行时 JSON Schema 校验（R7b · ADR-023）。

use std::sync::LazyLock;

use jsonschema::Validator;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{AppError, AppResult};

const PROMPT_WRITE_REQUEST: &str = include_str!("../schemas/prompt-write-request.json");
const USAGE_LOG_REQUEST: &str = include_str!("../schemas/usage-log-request.json");
const PROMPT_FEEDBACK_REQUEST: &str = include_str!("../schemas/prompt-feedback-request.json");
const API_ERROR_RESPONSE: &str = include_str!("../schemas/api-error.json");
const API_SUCCESS_RESPONSE: &str = include_str!("../schemas/api-success.json");
const SKILL_INSTALL_PAYLOAD: &str = include_str!("../schemas/skill-install-payload.json");
const SKILL_SUMMARY: &str = include_str!("../schemas/skill-summary.json");

static PROMPT_WRITE: LazyLock<Validator> =
    LazyLock::new(|| compile_schema(PROMPT_WRITE_REQUEST, "prompt-write-request"));
static USAGE_LOG: LazyLock<Validator> =
    LazyLock::new(|| compile_schema(USAGE_LOG_REQUEST, "usage-log-request"));
static PROMPT_FEEDBACK: LazyLock<Validator> =
    LazyLock::new(|| compile_schema(PROMPT_FEEDBACK_REQUEST, "prompt-feedback-request"));
static API_ERROR: LazyLock<Validator> =
    LazyLock::new(|| compile_schema(API_ERROR_RESPONSE, "api-error"));
static API_SUCCESS: LazyLock<Validator> =
    LazyLock::new(|| compile_schema(API_SUCCESS_RESPONSE, "api-success"));
static SKILL_INSTALL: LazyLock<Validator> =
    LazyLock::new(|| compile_schema(SKILL_INSTALL_PAYLOAD, "skill-install-payload"));
static SKILL_SUMMARY_SCHEMA: LazyLock<Validator> =
    LazyLock::new(|| compile_schema(SKILL_SUMMARY, "skill-summary"));

fn compile_schema(source: &str, label: &str) -> Validator {
    let schema: Value =
        serde_json::from_str(source).unwrap_or_else(|e| panic!("{label} schema JSON invalid: {e}"));
    jsonschema::validator_for(&schema)
        .unwrap_or_else(|e| panic!("{label} schema compile failed: {e}"))
}

fn run_validate(schema: &Validator, value: &Value) -> AppResult<()> {
    let errors: Vec<String> = schema
        .iter_errors(value)
        .take(3)
        .map(|e| e.to_string())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(errors.join("; ")))
    }
}

/// `POST/PATCH /api/v1/prompts` 与 domain `PromptInput` 写入校验。
pub fn validate_prompt_write(value: &Value) -> AppResult<()> {
    run_validate(&PROMPT_WRITE, value)
}

/// 兼容旧占位名。
pub fn validate_prompt_payload(value: &Value) -> AppResult<()> {
    validate_prompt_write(value)
}

/// `POST /api/v1/usage-logs/track` 请求体校验。
pub fn validate_usage_log_request(value: &Value) -> AppResult<()> {
    run_validate(&USAGE_LOG, value)
}

/// `POST …/prompts/:id/feedback` 请求体校验。
pub fn validate_prompt_feedback(value: &Value) -> AppResult<()> {
    run_validate(&PROMPT_FEEDBACK, value)
}

/// REST 错误响应信封（`promptstdio-api` `ApiError` 输出对照）。
pub fn validate_api_error_response(value: &Value) -> AppResult<()> {
    run_validate(&API_ERROR, value)
}

/// REST 成功响应信封（`api_success` helper 输出对照）。
pub fn validate_api_success_response(value: &Value) -> AppResult<()> {
    run_validate(&API_SUCCESS, value)
}

/// MCP `install_skill` / `sync_skill` 成功 data（`SkillCatalogService::install_payload`）。
pub fn validate_skill_install_payload(value: &Value) -> AppResult<()> {
    run_validate(&SKILL_INSTALL, value)
}

/// MCP `list_skills` 的 `skills[]` 元素 / Web Explore Skill 摘要。
pub fn validate_skill_summary(value: &Value) -> AppResult<()> {
    run_validate(&SKILL_SUMMARY_SCHEMA, value)
}

/// Schema 校验通过后反序列化（API 入口：`Json<Value>` → 强类型）。
pub fn validate_and_deserialize<T: DeserializeOwned>(
    value: Value,
    validate: fn(&Value) -> AppResult<()>,
) -> AppResult<T> {
    validate(&value)?;
    serde_json::from_value(value)
        .map_err(|e| AppError::validation(format!("invalid request body: {e}")))
}

#[cfg(test)]
#[cfg(feature = "server")]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prompt_write_rejects_empty_title() {
        let body = json!({ "title": "", "content": "hello" });
        assert!(validate_prompt_write(&body).is_err());
    }

    #[test]
    fn prompt_write_rejects_oversized_content() {
        let body = json!({ "title": "t", "content": "x".repeat(5001) });
        assert!(validate_prompt_write(&body).is_err());
    }

    #[test]
    fn prompt_write_accepts_harvest_fields() {
        let body = json!({
            "title": "采集",
            "content": "正文",
            "harvest_meta": { "source": "clipboard" },
            "harvest_source": "import"
        });
        assert!(validate_prompt_write(&body).is_ok());
    }

    #[test]
    fn prompt_write_accepts_null_optional_arrays() {
        let body = json!({
            "title": "t",
            "content": "hello",
            "tags": null,
            "parameters": null
        });
        assert!(validate_prompt_write(&body).is_ok());
    }

    #[test]
    fn usage_log_rejects_unknown_action() {
        let body = json!({ "action": "mcp_call" });
        assert!(validate_usage_log_request(&body).is_err());
    }

    #[test]
    fn usage_log_accepts_string_prompt_id() {
        let body = json!({
            "prompt_id": "abc123",
            "action": "copy_prompt",
            "context": { "source": "detail" }
        });
        assert!(validate_usage_log_request(&body).is_ok());
    }

    #[test]
    fn usage_log_accepts_m1_actions() {
        for action in [
            "copy_member_pack",
            "save_suite_from_explore",
            "save_suite_from_member_zone",
            "view_explore",
            "view_member_pack",
            "view_pricing",
            "quota_hit",
            "stitch_chat_done",
            "stitch_scene_run",
            "stitch_suite_run",
            "stitch_suite_done",
            "stitch_agent_run",
            "stitch_sediment_copy",
            "stitch_sediment_save",
            "stitch_mature_gate_soft",
            "stitch_mature_gate_block",
            "stitch_mature_unlock",
        ] {
            let body = json!({
                "action": action,
                "context": {
                    "source": "ui-design-collab",
                    "step": "1",
                    "kind": "prompt",
                    "scene": "structure",
                    "outcome": "done",
                    "tools": "list_dir,run_command",
                    "tool_count": "2",
                    "client": "stitch-desktop"
                }
            });
            assert!(
                validate_usage_log_request(&body).is_ok(),
                "action {action} should be allowed"
            );
        }
    }

    #[test]
    fn feedback_rejects_invalid_rating() {
        let body = json!({ "rating": "maybe" });
        assert!(validate_prompt_feedback(&body).is_err());
    }

    #[test]
    fn feedback_accepts_up_with_comment() {
        let body = json!({ "rating": "up", "comment": "好用" });
        assert!(validate_prompt_feedback(&body).is_ok());
    }

    #[test]
    fn api_error_envelope_accepts_code_and_message() {
        let body = json!({
            "error": { "code": 422, "message": "validation failed" }
        });
        assert!(validate_api_error_response(&body).is_ok());
    }

    #[test]
    fn api_success_envelope_requires_message_ok() {
        let body = json!({ "data": { "id": "1" }, "message": "ok" });
        assert!(validate_api_success_response(&body).is_ok());
        let bad = json!({ "data": { "id": "1" } });
        assert!(validate_api_success_response(&bad).is_err());
    }

    #[test]
    fn skill_install_payload_requires_agent_path_selection() {
        let ok = json!({
            "slug": "pm-prd-demo",
            "title": "PM PRD 转 Demo",
            "version": "2.0.1",
            "status": "installed",
            "mode": "remote",
            "path_selection": "agent",
            "spec": "agentskills.io",
            "install_guide": "guide",
            "install_guide_file": "references/install-guide.md",
            "common_skill_paths": {
                ".cursor/skills": {
                    "path": ".cursor/skills/pm-prd-demo",
                    "label": "Cursor"
                }
            },
            "instruction": "write files",
            "files": [{ "path": "SKILL.md", "content": "---\nname: pm-prd-demo\n---\n" }]
        });
        assert!(validate_skill_install_payload(&ok).is_ok());
        let bad = json!({
            "slug": "pm-prd-demo",
            "version": "2.0.1",
            "status": "installed",
            "path_selection": "fixed",
            "spec": "agentskills.io",
            "common_skill_paths": {},
            "instruction": "x",
            "files": []
        });
        assert!(validate_skill_install_payload(&bad).is_err());
    }

    #[test]
    fn skill_summary_accepts_optional_teaser() {
        let body = json!({
            "slug": "pm-prd-demo",
            "title": "PM PRD 转 Demo",
            "description": "粘贴 PRD",
            "version": "2.0.1",
            "tag": "ide-skill",
            "install_phrase": "帮我安装",
            "sync_phrase": "帮我更新",
            "teaser_slug": "pm-prd-demo-teaser"
        });
        assert!(validate_skill_summary(&body).is_ok());
        // 无 teaser 时字段省略（不输出 null）
        let no_teaser = json!({
            "slug": "pm-prd-demo",
            "title": "PM PRD 转 Demo",
            "description": "粘贴 PRD",
            "version": "2.0.1",
            "tag": "ide-skill",
            "install_phrase": "帮我安装",
            "sync_phrase": "帮我更新"
        });
        assert!(validate_skill_summary(&no_teaser).is_ok());
        let null_teaser = json!({
            "slug": "pm-prd-demo",
            "title": "PM PRD 转 Demo",
            "description": "粘贴 PRD",
            "version": "2.0.1",
            "tag": "ide-skill",
            "install_phrase": "帮我安装",
            "sync_phrase": "帮我更新",
            "teaser_slug": null
        });
        assert!(validate_skill_summary(&null_teaser).is_err());
    }
}
