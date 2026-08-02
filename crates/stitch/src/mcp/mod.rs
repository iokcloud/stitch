//! PromptStdio REST client (cloud assets).
//!
//! Connects to the PromptStdio platform to fetch task suites,
//! agent configurations, and prompt assets via the REST API.
//!
//! This is **not** the Model Context Protocol. For protocol MCP servers
//! see [`crate::mcp_protocol`].

pub mod cache;
pub mod transport;

use serde::{Deserialize, Serialize};
use transport::ApiTransport;

// ---------------------------------------------------------------------------
// Response types — mirrors promptstdio-domain shapes
// ---------------------------------------------------------------------------

/// A step within a task suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteStep {
    pub position: i64,
    pub step_title: String,
    pub prompt_type: String,
    pub prompt_id: Option<String>,
    pub title_snapshot: String,
    pub content_preview: String,
    pub content: String,
}

/// Full detail for a single task suite, including steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteDetail {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub step_count: usize,
    pub steps: Vec<SuiteStep>,
}

/// Summary row for task suite list endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteSummary {
    #[serde(deserialize_with = "deserialize_id_string")]
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub step_count: i64,
    pub updated_at: Option<String>,
}

/// Summary of a Studio agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    #[serde(deserialize_with = "deserialize_id_string")]
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "deserialize_id_string")]
    pub task_suite_id: String,
    pub task_suite_title: Option<String>,
    pub trigger_mode: String,
    pub file_write_permission: String,
    pub step_strategy: String,
    pub failure_policy: String,
    pub updated_at: Option<String>,
}

/// Full detail for a single Studio agent, including its suite steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDetail {
    pub id: String,
    pub name: String,
    pub task_suite_id: String,
    pub task_suite_title: Option<String>,
    pub trigger_mode: String,
    pub step_strategy: String,
    pub updated_at: Option<String>,
    pub file_write_permission: String,
    pub failure_policy: String,
    #[serde(default)]
    pub advanced_settings: serde_json::Value,
    pub task_suite: Option<SuiteDetail>,
}

/// Summary of a user prompt (returned by list_prompts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptSummary {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub updated_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Client for PromptStdio's REST API.
pub struct McpClient {
    transport: ApiTransport,
}

impl McpClient {
    /// Create a new client.
    ///
    /// `api_base` is the PromptStdio server URL (e.g. `https://promptstdio.com`).
    /// `api_token` is the user's API token for authentication.
    pub fn new(api_base: String, api_token: Option<String>) -> Self {
        Self {
            transport: ApiTransport::new(api_base, api_token),
        }
    }

    /// Check whether the client has valid credentials.
    pub fn is_authenticated(&self) -> bool {
        self.transport
            .api_token
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    }

    /// List the current user's task suites.
    pub async fn list_suites(
        &self,
        page: Option<i64>,
        limit: Option<i64>,
    ) -> anyhow::Result<Vec<SuiteSummary>> {
        tracing::info!("mcp list_suites");
        let page = page.unwrap_or(1);
        let limit = limit.unwrap_or(50);
        self.transport
            .get::<Vec<SuiteSummary>>(&format!("/api/v1/task-suites?page={page}&limit={limit}"))
            .await
    }

    /// List the current user's Studio agents.
    pub async fn list_agents(
        &self,
        page: Option<i64>,
        limit: Option<i64>,
    ) -> anyhow::Result<Vec<AgentSummary>> {
        tracing::info!("mcp list_agents");
        let page = page.unwrap_or(1);
        let limit = limit.unwrap_or(50);
        self.transport
            .get::<Vec<AgentSummary>>(&format!("/api/v1/task-agents?page={page}&limit={limit}"))
            .await
    }

    /// Fetch a task suite by slug or numeric ID.
    pub async fn get_suite(&self, slug: &str) -> anyhow::Result<SuiteDetail> {
        tracing::info!(%slug, "mcp get_suite");
        self.transport
            .get::<SuiteDetail>(&format!("/api/v1/task-suites/{slug}"))
            .await
    }

    /// Fetch a Studio agent by slug/ID and return its full detail including suite steps.
    pub async fn get_agent(&self, slug: &str) -> anyhow::Result<AgentDetail> {
        tracing::info!(%slug, "mcp get_agent");
        self.transport
            .get::<AgentDetail>(&format!("/api/v1/task-agents/{slug}"))
            .await
    }

    /// Run an agent by name and return the execution plan.
    pub async fn run_agent_by_name(
        &self,
        name: &str,
        context: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        tracing::info!(%name, "mcp run_agent_by_name");
        let mut path = format!("/api/v1/task-agents/run?name={}", urlencoding(name));
        if let Some(ctx) = context {
            path.push_str(&format!("&context={}", urlencoding(ctx)));
        }
        self.transport.get::<serde_json::Value>(&path).await
    }

    /// List the user's prompts from PromptStdio.
    pub async fn list_prompts(
        &self,
        tag: Option<&str>,
        search: Option<&str>,
    ) -> anyhow::Result<Vec<PromptSummary>> {
        tracing::info!("mcp list_prompts");
        let mut path = "/api/v1/prompts".to_string();
        let mut params = Vec::new();
        if let Some(t) = tag {
            params.push(format!("tag={}", urlencoding(t)));
        }
        if let Some(s) = search {
            params.push(format!("search={}", urlencoding(s)));
        }
        if !params.is_empty() {
            path.push('?');
            path.push_str(&params.join("&"));
        }
        self.transport.get::<Vec<PromptSummary>>(&path).await
    }

    /// Fire-and-forget friendly usage track (requires Token).
    #[allow(clippy::disallowed_methods)] // serde_json::json! 宏展开内含 unwrap
    pub async fn track_usage(
        &self,
        action: &str,
        context: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        if !self.is_authenticated() {
            return Err(anyhow::anyhow!("not authenticated"));
        }
        let mut body = serde_json::json!({ "action": action });
        if let Some(ctx) = context {
            body["context"] = ctx;
        }
        self.transport
            .post_ok("/api/v1/usage-logs/track", &body)
            .await
    }

    /// Create a personal prompt (POST /api/v1/prompts).
    #[allow(clippy::disallowed_methods)] // serde_json::json! 宏展开内含 unwrap
    pub async fn create_prompt(
        &self,
        title: &str,
        content: &str,
        description: Option<&str>,
        tags: Option<Vec<String>>,
    ) -> anyhow::Result<PromptSummary> {
        tracing::info!(%title, "mcp create_prompt");
        let mut body = serde_json::json!({
            "title": title,
            "content": content,
            "harvest_source": "stitch",
        });
        if let Some(d) = description {
            body["description"] = serde_json::Value::String(d.to_string());
        }
        if let Some(t) = tags {
            body["tags"] =
                serde_json::Value::Array(t.into_iter().map(serde_json::Value::String).collect());
        }
        // API returns PromptDetail; map to summary fields we need.
        let detail: serde_json::Value = self.transport.post("/api/v1/prompts", &body).await?;
        Ok(PromptSummary {
            id: detail
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            title: detail
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or(title)
                .to_string(),
            description: detail
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            tags: detail
                .get("tags")
                .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok()),
            updated_at: detail
                .get("updated_at")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        })
    }

    /// Submit personal prompt for Explore review (POST …/submit-explore).
    pub async fn submit_explore(&self, prompt_id: &str) -> anyhow::Result<SubmitExploreSummary> {
        let id = prompt_id.trim().trim_start_matches("prompt:");
        tracing::info!(%id, "mcp submit_explore");
        let path = format!("/api/v1/prompts/{}/submit-explore", urlencoding(id));
        let detail: serde_json::Value = self.transport.post(&path, &serde_json::json!({})).await?;
        Ok(SubmitExploreSummary {
            system_prompt_id: detail
                .get("system_prompt_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            slug: detail
                .get("slug")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            status: detail
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            already_submitted: detail
                .get("already_submitted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
    }
}

/// Result of submit-explore (ADR-033).
#[derive(Debug, Clone)]
pub struct SubmitExploreSummary {
    pub system_prompt_id: String,
    pub slug: String,
    pub status: String,
    pub already_submitted: bool,
}

/// Accept Surreal Thing JSON as either a string or `{ tb, id }` object.
fn deserialize_id_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Object(map) => {
            let tb = map.get("tb").and_then(|v| v.as_str()).unwrap_or("record");
            let id = match map.get("id") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Number(n)) => n.to_string(),
                Some(other) => other.to_string().trim_matches('"').to_string(),
                None => return Err(serde::de::Error::custom("Thing missing id")),
            };
            Ok(format!("{tb}:{id}"))
        }
        other => Ok(other.to_string().trim_matches('"').to_string()),
    }
}

/// URL-encode a string slice (avoids pulling in a full encoding crate).
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".to_string(),
            _ => {
                let mut buf = [0u8; 4];
                let encoded = c.encode_utf8(&mut buf);
                encoded
                    .bytes()
                    .map(|b| format!("%{:02X}", b))
                    .collect::<Vec<_>>()
                    .join("")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding_plain_ascii() {
        assert_eq!(urlencoding("hello"), "hello");
        assert_eq!(urlencoding("foo-bar_baz.123"), "foo-bar_baz.123");
    }

    #[test]
    fn urlencoding_space() {
        assert_eq!(urlencoding("hello world"), "hello%20world");
    }

    #[test]
    fn urlencoding_special_chars() {
        let encoded = urlencoding("a@b.com");
        assert!(encoded.contains("%40"), "expected %40 in: {encoded}");
    }

    #[test]
    fn urlencoding_chinese() {
        let encoded = urlencoding("测试");
        assert!(!encoded.contains(' '), "spaces in: {encoded}");
        assert!(encoded.len() > 6, "too short: {encoded}");
    }

    #[test]
    fn urlencoding_empty() {
        assert_eq!(urlencoding(""), "");
    }

    #[test]
    fn mcp_client_is_authenticated() {
        let c = McpClient::new("https://example.com".into(), Some("tok".into()));
        assert!(c.is_authenticated());
    }

    #[test]
    fn mcp_client_not_authenticated() {
        let c = McpClient::new("https://example.com".into(), None);
        assert!(!c.is_authenticated());
    }

    #[test]
    fn mcp_client_empty_token() {
        let c = McpClient::new("https://example.com".into(), Some("".into()));
        assert!(!c.is_authenticated());
    }

    #[test]
    fn deserialize_suite_summary_id_string() {
        let raw = r#"{"id":"task_suite:abc","title":"T","step_count":1}"#;
        let s: SuiteSummary = serde_json::from_str(raw).expect("parse");
        assert_eq!(s.id, "task_suite:abc");
        assert_eq!(s.title, "T");
    }

    #[test]
    fn deserialize_suite_summary_id_object() {
        let raw = r#"{"id":{"tb":"task_suite","id":"abc"},"title":"T","step_count":2}"#;
        let s: SuiteSummary = serde_json::from_str(raw).expect("parse");
        assert_eq!(s.id, "task_suite:abc");
    }
}
