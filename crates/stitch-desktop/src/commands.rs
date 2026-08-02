//! Tauri IPC commands — bridge between the frontend UI and stitch engine.

use crate::platform;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, atomic::AtomicBool, atomic::Ordering};
use std::time::Duration;
use stitch::agent::persist::{self, Manifest};
use stitch::agent::{self, AgentEvent};
use stitch::config::{LlmProfile, McpProfile, McpServerProfile, StitchConfig};
use stitch::mcp::{self, AgentSummary, SuiteSummary};
use stitch::mcp_protocol;
use stitch::session::{Role, Session};
use stitch::tools;
use tauri::Emitter;
use tauri::Manager;
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::oneshot;

fn new_plan_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("plan-{ms}-{}", ms % 9973)
}

/// Build a readable suite failure summary (completed + failed + skipped).
fn format_suite_failure_summary(
    suite_title: &str,
    failed_idx: usize,
    total_steps: usize,
    failed_title: &str,
    error: &str,
    completed_body: &str,
    step_titles: &[(usize, &str)],
) -> String {
    let mut out = format!(
        "套件「{suite_title}」未全部完成：第 {}/{total_steps} 步失败（{failed_title}）。\n\n",
        failed_idx + 1
    );

    let completed = completed_body.trim();
    if !completed.is_empty() {
        out.push_str("## 已完成步骤\n\n");
        out.push_str(completed);
        out.push_str("\n\n");
    } else if failed_idx > 0 {
        out.push_str(&format!(
            "前 {failed_idx} 步已执行，但未留下可展示的报告。\n\n"
        ));
    }

    out.push_str("## 失败步骤\n\n");
    out.push_str(&format!(
        "### 步骤 {}/{total_steps} · {failed_title}\n\n原因：{error}\n\n",
        failed_idx + 1
    ));

    let skipped: Vec<_> = step_titles
        .iter()
        .copied()
        .filter(|(i, _)| *i > failed_idx)
        .collect();
    if !skipped.is_empty() {
        out.push_str("## 未执行步骤\n\n");
        for (i, title) in skipped {
            out.push_str(&format!("- 步骤 {}/{total_steps} · {title}\n", i + 1));
        }
    }
    out
}

/// Shared cancellation state for aborting in-flight agent generation.
#[derive(Default, Clone)]
pub struct CancelState {
    pub flag: Arc<AtomicBool>,
    pub notify: Arc<tokio::sync::Notify>,
    /// True while `send_message` / suite-agent run holds the generation slot.
    pub busy: Arc<AtomicBool>,
}

/// In-memory agent sessions keyed by frontend chat id.
///
/// Desktop UI history is text-only; without this cache, multi-turn / retry
/// rebuilds a Session that drops `tool_calls`/`tool` pairs and triggers
/// DeepSeek 400s after tool-heavy turns.
#[derive(Default, Clone)]
pub struct AgentSessionStore {
    inner: Arc<Mutex<HashMap<String, Session>>>,
}

impl AgentSessionStore {
    fn take(&self, id: &str) -> Option<Session> {
        self.inner.lock().ok()?.remove(id)
    }

    fn put(&self, id: &str, session: Session) {
        if let Ok(mut map) = self.inner.lock() {
            map.insert(id.to_string(), session);
        }
    }

    fn remove(&self, id: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(id);
        }
    }
}

struct BusyGuard(Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn try_acquire_generation(state: &CancelState) -> Result<BusyGuard, String> {
    if state
        .busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("已有生成任务在进行，请先停止或等待完成".into());
    }
    Ok(BusyGuard(state.busy.clone()))
}

fn emit_cancelled(app: &tauri::AppHandle) {
    let _ = app.emit(
        "agent-event",
        serde_json::json!({
            "type": "cancelled",
            "message": "Generation cancelled by user."
        }),
    );
}

/// Pending tool confirmation requests awaiting user response.
#[derive(Clone)]
pub struct ConfirmState {
    pub pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    /// Persisted allow rules（记住此规则）; shared with the agent loop so a
    /// rule remembered mid-turn applies to the next tool call.
    pub rules: Arc<Mutex<stitch::allow::AllowRules>>,
}

impl Default for ConfirmState {
    fn default() -> Self {
        Self {
            pending: Default::default(),
            rules: Arc::new(Mutex::new(stitch::allow::AllowRules::load())),
        }
    }
}

/// Pending plan approval requests awaiting user response.
#[derive(Default, Clone)]
pub struct PlanState {
    pub pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

/// Masked LLM profile for the settings UI (never returns raw keys).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProfileSnapshot {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub api_base: String,
    pub api_key_masked: String,
    pub api_key_set: bool,
    pub model: String,
    /// Whether the model accepts image input (gate for the paste entry).
    #[serde(default)]
    pub supports_images: bool,
}

/// Masked PromptStdio account profile for the settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpProfileSnapshot {
    pub id: String,
    pub label: String,
    pub api_base: String,
    pub api_token_masked: String,
    pub api_token_set: bool,
}

/// Masked MCP protocol server for the settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerSnapshot {
    pub id: String,
    pub label: String,
    pub transport: String,
    pub enabled: bool,
    pub command: Option<String>,
    pub args: Vec<String>,
    /// Environment variables (local desktop only; values shown for edit).
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub auth_set: bool,
    pub auth_masked: String,
}

/// Complete configuration snapshot sent to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub api_base: String,
    pub api_token_masked: String,
    pub api_token_set: bool,
    pub active_mcp_id: Option<String>,
    pub mcp_profiles: Vec<McpProfileSnapshot>,
    pub mcp_servers: Vec<McpServerSnapshot>,
    pub llm_provider: String,
    pub llm_api_base: String,
    pub llm_api_key_masked: String,
    pub llm_api_key_set: bool,
    pub llm_model: String,
    pub active_profile_id: Option<String>,
    pub llm_profiles: Vec<LlmProfileSnapshot>,
    pub max_iterations: usize,
    pub sediment_visibility: String,
    /// Local vision describe layer (settings UI + paste gate).
    pub local_vision: LocalVisionSnapshot,
}

/// Local vision describe layer snapshot (never returns secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalVisionSnapshot {
    pub enabled: bool,
    pub api_base: String,
    pub model: String,
    pub timeout_secs: u64,
}

/// Prior turn for multi-turn chat (user/assistant text only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
    /// Image data URLs on a user message (frontend keeps them in memory only;
    /// this field exists so a rebuild keeps the images if they arrive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

fn profile_to_snapshot(p: &LlmProfile) -> LlmProfileSnapshot {
    LlmProfileSnapshot {
        id: p.id.clone(),
        label: p.label.clone(),
        provider: p.provider.clone(),
        api_base: p.api_base.clone(),
        api_key_masked: mask_key(p.api_key.as_deref().unwrap_or("")),
        api_key_set: p.api_key.as_deref().map(|k| !k.is_empty()).unwrap_or(false),
        model: p.model.clone(),
        supports_images: stitch::agent::tokens::model_supports_vision(&p.model),
    }
}

fn mcp_profile_to_snapshot(p: &McpProfile) -> McpProfileSnapshot {
    McpProfileSnapshot {
        id: p.id.clone(),
        label: p.label.clone(),
        api_base: p.api_base.clone(),
        api_token_masked: mask_key(p.api_token.as_deref().unwrap_or("")),
        api_token_set: p
            .api_token
            .as_deref()
            .map(|t| !t.is_empty())
            .unwrap_or(false),
    }
}

fn auth_from_headers(headers: &std::collections::HashMap<String, String>) -> Option<&str> {
    headers
        .get("Authorization")
        .or_else(|| headers.get("authorization"))
        .map(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
}

fn mcp_server_to_snapshot(p: &McpServerProfile) -> McpServerSnapshot {
    let auth = auth_from_headers(&p.headers).unwrap_or("");
    McpServerSnapshot {
        id: p.id.clone(),
        label: p.label.clone(),
        transport: p.transport.clone(),
        enabled: p.enabled,
        command: p.command.clone(),
        args: p.args.clone(),
        env: p.env.clone(),
        cwd: p.cwd.clone(),
        url: p.url.clone(),
        auth_set: !auth.is_empty(),
        auth_masked: mask_key(auth),
    }
}

pub(crate) fn config_to_snapshot(cfg: &StitchConfig) -> ConfigSnapshot {
    let flat_set = cfg
        .api_token
        .as_deref()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);
    let active_set = cfg
        .active_mcp_id
        .as_deref()
        .and_then(|id| cfg.mcp_profiles.iter().find(|p| p.id == id))
        .or_else(|| cfg.mcp_profiles.first())
        .and_then(|p| p.api_token.as_deref())
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);
    let token_set = flat_set || active_set;
    let token_for_mask = cfg
        .api_token
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            cfg.active_mcp_id
                .as_deref()
                .and_then(|id| cfg.mcp_profiles.iter().find(|p| p.id == id))
                .or_else(|| cfg.mcp_profiles.first())
                .and_then(|p| p.api_token.as_deref())
        })
        .unwrap_or("");
    ConfigSnapshot {
        api_base: cfg.api_base.clone(),
        api_token_masked: mask_key(token_for_mask),
        api_token_set: token_set,
        active_mcp_id: cfg.active_mcp_id.clone(),
        mcp_profiles: cfg
            .mcp_profiles
            .iter()
            .map(mcp_profile_to_snapshot)
            .collect(),
        mcp_servers: cfg.mcp_servers.iter().map(mcp_server_to_snapshot).collect(),
        llm_provider: cfg.llm_provider.clone(),
        llm_api_base: cfg.llm_api_base.clone(),
        llm_api_key_masked: mask_key(cfg.llm_api_key.as_deref().unwrap_or("")),
        llm_api_key_set: cfg
            .llm_api_key
            .as_deref()
            .map(|k| !k.is_empty())
            .unwrap_or(false),
        llm_model: cfg.llm_model.clone(),
        active_profile_id: cfg.active_profile_id.clone(),
        llm_profiles: cfg.llm_profiles.iter().map(profile_to_snapshot).collect(),
        max_iterations: cfg.max_iterations,
        sediment_visibility: if cfg.sediment_submits_explore() {
            "explore".into()
        } else {
            "personal".into()
        },
        local_vision: LocalVisionSnapshot {
            enabled: cfg.local_vision.enabled,
            api_base: cfg.local_vision.api_base.clone(),
            model: cfg.local_vision.model.clone(),
            timeout_secs: cfg.local_vision.timeout_secs,
        },
    }
}

async fn build_agent_registry(work_dir: &str, cfg: &StitchConfig) -> tools::ToolRegistry {
    let mut registry = tools::build_registry(work_dir);
    let enabled: Vec<_> = cfg
        .mcp_servers
        .iter()
        .filter(|p| p.enabled)
        .cloned()
        .collect();
    if enabled.is_empty() {
        return registry;
    }
    let discovered = mcp_protocol::discover_enabled(&enabled).await;
    if !discovered.is_empty() {
        tracing::info!(
            count = discovered.len(),
            servers = enabled.len(),
            "attached MCP protocol tools"
        );
    }
    registry.attach_mcp_tools(&discovered, &enabled);
    registry
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "\u{2022}\u{2022}\u{2022}\u{2022}".into();
    }
    format!(
        "{}\u{2022}\u{2022}\u{2022}\u{2022}{}",
        &key[..4],
        &key[key.len() - 4..]
    )
}

fn apply_history(session: &mut Session, history: &[HistoryMessage]) {
    for msg in history {
        let role = msg.role.to_ascii_lowercase();
        let content = msg.content.trim();
        match role.as_str() {
            "user" => {
                if content.is_empty() && msg.images.as_deref().map(|i| i.is_empty()).unwrap_or(true)
                {
                    continue;
                }
                session.add_user_message(stitch::session::user_content_with_images(
                    content,
                    msg.images.as_deref().unwrap_or(&[]),
                ));
            }
            "assistant" if !content.is_empty() => {
                session.add_assistant_message(content);
            }
            _ => {}
        }
    }
}

/// First 32 chars of the first image url, if any (image fingerprint).
fn first_image_prefix(c: &stitch::session::Content) -> Option<String> {
    match c {
        stitch::session::Content::Parts(parts) => parts.iter().find_map(|p| match p {
            stitch::session::ContentPart::ImageUrl { image_url } => {
                Some(image_url.url.chars().take(32).collect())
            }
            _ => None,
        }),
        _ => None,
    }
}

/// Same-message check for the user turn: text match when text is non-empty;
/// image-only messages compare by image count + first-image signature so two
/// empty-text messages never compare equal just because both are "".
/// Same-message check for the user turn. Text-only messages compare by text;
/// any image involvement additionally requires matching image count and
/// first-image signature — same text with different images is a new message,
/// and two empty-text messages never compare equal just because both are "".
fn content_matches_user(c: &stitch::session::Content, text: &str, images: &[String]) -> bool {
    // Stored text may carry the local-vision describe block — strip it so
    // stop→resend / edit-resend still match the user's original text.
    let ct = stitch::llm::vision::strip_image_descriptions(c.text());
    let mt = text.trim();
    if c.image_count() == 0 && images.is_empty() {
        return ct.trim() == mt;
    }
    if ct.trim() != mt {
        return false;
    }
    let msg_prefix = first_image_prefix(c);
    let img_prefix = images
        .first()
        .map(|u| u.chars().take(32).collect::<String>());
    c.image_count() == images.len() && msg_prefix == img_prefix
}

/// Persist finished Agent session to `{work_dir}/.stitch/sessions/{id}/` (ADR-036).
fn persist_agent_session(work_dir: &str, chat_id: &str, session: &Session) {
    let wd = work_dir.trim();
    if wd.is_empty() {
        return;
    }
    let Some(dir) = persist::session_dir(std::path::Path::new(wd), chat_id) else {
        tracing::warn!(%chat_id, "skip persist: invalid session id");
        return;
    };
    let mut manifest = Manifest::new(chat_id, std::path::Path::new(wd));
    let man_path = dir.join("manifest.json");
    if man_path.is_file()
        && let Ok(raw) = std::fs::read_to_string(&man_path)
        && let Ok(prev) = serde_json::from_str::<Manifest>(&raw)
    {
        manifest.created_at = prev.created_at;
        manifest.committed_epoch = prev.committed_epoch;
    }

    if session.epoch > manifest.committed_epoch {
        let parent_epoch = session.epoch.saturating_sub(1);
        let summary = agent::context::condensed_summary_text(session).unwrap_or("");
        let cp = persist::checkpoint_from_compact(
            chat_id,
            session,
            parent_epoch,
            summary,
            "full",
            [0, session.messages.len().saturating_sub(1)],
        );
        if let Err(e) = persist::commit_checkpoint(&dir, &cp, &mut manifest) {
            tracing::warn!(%e, %chat_id, "checkpoint commit failed");
        }
    }

    if let Err(e) = persist::save_session(&dir, session, &mut manifest) {
        tracing::warn!(%e, %chat_id, "agent session persist failed");
    } else {
        tracing::info!(
            %chat_id,
            msgs = session.messages.len(),
            epoch = session.epoch,
            "agent session persisted"
        );
    }
}

/// User stop: restore disk to the pre-turn anchor so the aborted turn does
/// not resurface after an app restart (stop = discard this turn).
fn rollback_turn_disk(flusher: &Option<Arc<Mutex<persist::TurnFlusher>>>, before: &Session) {
    let Some(f) = flusher else { return };
    match f.lock() {
        Ok(mut g) => g.rollback(before),
        Err(_) => tracing::warn!("turn flusher lock poisoned; skip disk rollback"),
    }
}

/// Resolve session: memory → disk → UI text history (ADR-036 restore chain).
/// Snapshot before this turn's user message — used to restore memory on cancel
/// so the next send does not resume dangling tool_calls from an aborted turn.
fn session_before_turn(
    session: &Session,
    message: &str,
    images: &[String],
    resume: bool,
) -> Session {
    let mut s = session.clone();
    if !resume
        && s.messages.last().is_some_and(|m| {
            m.role == Role::User && content_matches_user(&m.content, message, images)
        })
    {
        s.messages.pop();
    }
    s
}

#[allow(clippy::too_many_arguments)]
fn resolve_agent_session(
    session_store: &AgentSessionStore,
    work_dir: &str,
    chat_id: Option<&str>,
    system_prompt: String,
    history: Option<&[HistoryMessage]>,
    message: &str,
    images: Option<&[String]>,
    resume: bool,
    rewind_to_user: Option<&str>,
    rewind_drop: bool,
) -> Session {
    let mut session = if let Some(id) = chat_id {
        if let Some(existing) = session_store.take(id) {
            tracing::info!(%id, "agent session from memory");
            existing
        } else if !work_dir.trim().is_empty()
            && let Some(dir) = persist::session_dir(std::path::Path::new(work_dir), id)
            && let Ok(Some((loaded, man))) = persist::load_session(&dir)
        {
            if man.restore_degraded.is_some() {
                tracing::warn!(%id, "agent session restored from checkpoint fallback");
            } else {
                tracing::info!(%id, epoch = loaded.epoch, "agent session from disk");
            }
            loaded
        } else {
            tracing::info!(%id, "agent session rebuilt from UI history (degraded)");
            let mut s = Session::new(system_prompt);
            if let Some(hist) = history {
                apply_history(&mut s, hist);
            }
            s
        }
    } else {
        let mut s = Session::new(system_prompt);
        if let Some(hist) = history {
            apply_history(&mut s, hist);
        }
        s
    };

    // Rewind (regenerate / edit-resend): drop tail turns back to the target
    // user message so the model never sees the discarded assistant turn.
    // Index 0 (system prompt) is never popped.
    if let Some(target) = rewind_to_user.map(str::trim).filter(|s| !s.is_empty()) {
        while session.messages.len() > 1 {
            let last = session.messages.last().expect("len > 1");
            if last.role == Role::User
                && stitch::llm::vision::strip_image_descriptions(last.content.text()).trim()
                    == target
            {
                break;
            }
            session.messages.pop();
        }
        if rewind_drop
            && session.messages.len() > 1
            && session.messages.last().is_some_and(|m| {
                m.role == Role::User
                    && stitch::llm::vision::strip_image_descriptions(m.content.text()).trim()
                        == target
            })
        {
            session.messages.pop();
        }
    }

    let last_is_same_user = session.messages.last().is_some_and(|m| {
        m.role == Role::User && content_matches_user(&m.content, message, images.unwrap_or(&[]))
    });
    if resume || last_is_same_user {
        tracing::info!(
            resume,
            "resuming agent session without re-appending user turn"
        );
    } else {
        if let Some(last) = session.messages.last()
            && last.role == Role::User
            && content_matches_user(&last.content, message, images.unwrap_or(&[]))
        {
            session.messages.pop();
        }
        session.add_user_message(stitch::session::user_content_with_images(
            message,
            images.unwrap_or(&[]),
        ));
    }

    agent::context::repair_message_sequence(&mut session.messages);
    session
}

fn mcp_client(cfg: &StitchConfig) -> Result<mcp::McpClient, String> {
    let resolved = cfg.resolve_mcp(None).map_err(|e| e.to_string())?;
    Ok(mcp::McpClient::new(
        resolved.api_base,
        Some(resolved.api_token),
    ))
}

/// Best-effort usage track — never fails the caller UI; skips when Token missing.
fn spawn_track(action: impl Into<String>, mut context: serde_json::Value) {
    let action = action.into();
    tokio::spawn(async move {
        let Ok(cfg) = StitchConfig::load() else {
            return;
        };
        let Ok(resolved) = cfg.resolve_mcp(None) else {
            return;
        };
        if let Some(obj) = context.as_object_mut() {
            obj.entry("client")
                .or_insert_with(|| serde_json::json!("stitch-desktop"));
        }
        let client = mcp::McpClient::new(resolved.api_base, Some(resolved.api_token));
        if let Err(e) = client.track_usage(&action, Some(context)).await {
            tracing::debug!(error = %e, %action, "usage track skipped/failed");
        }
    });
}

#[tauri::command]
pub async fn track_usage(action: String, context: Option<serde_json::Value>) -> Result<(), String> {
    let action = action.trim().to_string();
    if action.is_empty() {
        return Ok(());
    }
    let cfg = match StitchConfig::load() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    let Ok(resolved) = cfg.resolve_mcp(None) else {
        return Ok(());
    };
    let mut ctx = context.unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = ctx.as_object_mut() {
        obj.entry("client")
            .or_insert_with(|| serde_json::json!("stitch-desktop"));
    }
    let client = mcp::McpClient::new(resolved.api_base, Some(resolved.api_token));
    // Do not surface track errors to UI
    let _ = client.track_usage(&action, Some(ctx)).await;
    Ok(())
}

/// Membership probe for mature-scene soft/hard gates (never blocks chat tools).
#[derive(Debug, Clone, Serialize)]
pub struct MembershipSnapshot {
    pub token_set: bool,
    pub is_member: bool,
    pub status: String,
    pub plan: Option<String>,
    pub pricing_url: String,
}

fn pricing_url_for(api_base: &str) -> String {
    let base = api_base.trim().trim_end_matches('/');
    if base.is_empty() {
        return "https://www.promptstdio.com/pricing".into();
    }
    // Local/dev API often on :8090 — send users to production pricing page.
    if base.contains("127.0.0.1") || base.contains("localhost") {
        return "https://www.promptstdio.com/pricing".into();
    }
    format!("{base}/pricing")
}

#[tauri::command]
pub async fn get_membership() -> Result<MembershipSnapshot, String> {
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let resolved = match cfg.resolve_mcp(None) {
        Ok(r) => r,
        Err(_) => {
            let pricing_url = pricing_url_for(&cfg.api_base);
            return Ok(MembershipSnapshot {
                token_set: false,
                is_member: false,
                status: "none".into(),
                plan: None,
                pricing_url,
            });
        }
    };
    let pricing_url = pricing_url_for(&resolved.api_base);
    let base = resolved.api_base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Ok(MembershipSnapshot {
            token_set: true,
            is_member: false,
            status: "unknown".into(),
            plan: None,
            pricing_url,
        });
    }
    let url = format!("{base}/api/v1/payments/membership");
    let resp = reqwest::Client::new()
        .get(&url)
        .header("Authorization", format!("Bearer {}", resolved.api_token))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("会员状态查询失败: {e}"))?;
    if !resp.status().is_success() {
        // Conservative: treat as non-member for gate soft tips.
        return Ok(MembershipSnapshot {
            token_set: true,
            is_member: false,
            status: "unknown".into(),
            plan: None,
            pricing_url,
        });
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("会员状态解析失败: {e}"))?;
    let is_member = body
        .get("is_member")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or(if is_member { "active" } else { "none" })
        .to_string();
    let plan = body
        .get("plan")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Ok(MembershipSnapshot {
        token_set: true,
        is_member: is_member || status.eq_ignore_ascii_case("active"),
        status,
        plan,
        pricing_url,
    })
}

#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    let url = url.trim().to_string();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("仅支持 http(s) 链接".into());
    }
    open_http_url(&url)
}

pub(crate) fn open_http_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|e| format!("无法打开链接: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("无法打开链接: {e}"))?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("无法打开链接: {e}"))?;
        Ok(())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = url;
        Err("当前平台不支持打开外链".into())
    }
}

/// Strip Windows `\\?\` prefix from canonicalize() for readable UI paths.
fn display_path(path: &std::path::Path) -> String {
    let s = path.display().to_string();
    #[cfg(windows)]
    {
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    }
    #[cfg(not(windows))]
    {
        s
    }
}

fn resolve_initial_work_dir() -> String {
    if let Ok(cfg) = StitchConfig::load()
        && let Some(ref p) = cfg.work_dir
    {
        let pb = PathBuf::from(p);
        if pb.is_dir() {
            return pb
                .canonicalize()
                .map(|c| display_path(&c))
                .unwrap_or_else(|_| p.clone());
        }
    }
    std::env::current_dir()
        .map(|p| display_path(&p))
        .unwrap_or_else(|_| ".".into())
}

fn persist_work_dir(path: &str) {
    match StitchConfig::load() {
        Ok(mut cfg) => {
            cfg.work_dir = Some(path.to_string());
            if let Err(e) = cfg.save() {
                tracing::warn!(error = %e, "failed to persist work_dir");
            }
        }
        Err(e) => tracing::warn!(error = %e, "failed to load config for work_dir persist"),
    }
}

// ── Commands ────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_config() -> Result<ConfigSnapshot, String> {
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

#[tauri::command]
pub fn save_config(
    updates: std::collections::HashMap<String, String>,
) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    for (key, value) in &updates {
        cfg.set(key, value).map_err(|e| e.to_string())?;
    }
    let _ = cfg.ensure_llm_profiles_seeded();
    let _ = cfg.ensure_mcp_profiles_seeded();
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

/// Upsert payload for a named LLM profile.
#[derive(Debug, Deserialize)]
pub struct UpsertLlmProfileArgs {
    pub id: String,
    pub label: Option<String>,
    pub provider: String,
    pub api_base: String,
    /// Omit or empty to keep the previously stored key.
    pub api_key: Option<String>,
    pub model: String,
}

#[tauri::command]
pub fn upsert_llm_profile(args: UpsertLlmProfileArgs) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let _ = cfg.ensure_llm_profiles_seeded();
    let label = args
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(args.id.trim())
        .to_string();
    cfg.upsert_profile(LlmProfile {
        id: args.id,
        label,
        provider: args.provider,
        api_base: args.api_base,
        api_key: args.api_key,
        model: args.model,
    })
    .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

#[tauri::command]
pub fn delete_llm_profile(id: String) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    cfg.delete_profile(id.trim()).map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

#[tauri::command]
pub fn set_active_llm_profile(id: String) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let _ = cfg.ensure_llm_profiles_seeded();
    cfg.activate_profile(id.trim()).map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

/// Upsert payload for a named PromptStdio account profile.
#[derive(Debug, Deserialize)]
pub struct UpsertMcpProfileArgs {
    pub id: String,
    pub label: Option<String>,
    pub api_base: String,
    /// Omit or empty to keep the previously stored token.
    pub api_token: Option<String>,
}

#[tauri::command]
pub fn upsert_mcp_profile(args: UpsertMcpProfileArgs) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let _ = cfg.ensure_mcp_profiles_seeded();
    let label = args
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("PromptStdio")
        .to_string();
    cfg.upsert_mcp_profile(McpProfile {
        id: args.id,
        label,
        api_base: args.api_base,
        api_token: args.api_token,
    })
    .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

#[tauri::command]
pub fn delete_mcp_profile(id: String) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    cfg.delete_mcp_profile(id.trim())
        .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

#[tauri::command]
pub fn set_active_mcp_profile(id: String) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let _ = cfg.ensure_mcp_profiles_seeded();
    cfg.activate_mcp_profile(id.trim())
        .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

#[tauri::command]
pub fn clear_mcp_profile_token(id: String) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    cfg.clear_mcp_profile_token(id.trim())
        .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

/// Upsert payload for a protocol MCP server (Cursor/Claude-shaped fields).
#[derive(Debug, Deserialize)]
pub struct UpsertMcpServerArgs {
    pub id: String,
    pub label: Option<String>,
    pub transport: String,
    #[serde(default = "default_enabled_true")]
    pub enabled: bool,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// When set (including empty map), replaces stored env. When omitted, keep existing.
    pub env: Option<std::collections::HashMap<String, String>>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    /// Optional Bearer token / raw Authorization value for HTTP.
    pub auth_token: Option<String>,
    /// Extra HTTP headers (merged; Authorization from auth_token wins when set).
    pub headers: Option<std::collections::HashMap<String, String>>,
}

fn default_enabled_true() -> bool {
    true
}

#[tauri::command]
pub fn upsert_mcp_server(args: UpsertMcpServerArgs) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let id = args.id.trim().to_string();
    let existing = cfg.mcp_server(&id).cloned();
    let label = args
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(id.as_str())
        .to_string();
    let env = match args.env {
        Some(e) => e,
        None => existing.as_ref().map(|p| p.env.clone()).unwrap_or_default(),
    };
    let cwd = match &args.cwd {
        None => existing.as_ref().and_then(|p| p.cwd.clone()),
        Some(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
    };
    let mut headers = args.headers.unwrap_or_else(|| {
        existing
            .as_ref()
            .map(|p| p.headers.clone())
            .unwrap_or_default()
    });
    if let Some(tok) = args
        .auth_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let value = if tok.to_ascii_lowercase().starts_with("bearer ") {
            tok.to_string()
        } else {
            format!("Bearer {tok}")
        };
        headers.insert("Authorization".into(), value);
    }
    cfg.upsert_mcp_server(McpServerProfile {
        id,
        label,
        transport: args.transport,
        enabled: args.enabled,
        command: args.command,
        args: args.args,
        env,
        cwd,
        url: args.url,
        headers,
    })
    .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

/// Import Cursor/Claude Desktop `mcpServers` JSON. Merges by id (overwrites same id).
#[derive(Debug, Deserialize)]
pub struct ImportMcpServersArgs {
    pub json: String,
    /// When true, replace the entire list with parsed servers.
    #[serde(default)]
    pub replace: bool,
}

#[tauri::command]
pub fn import_mcp_servers(args: ImportMcpServersArgs) -> Result<ConfigSnapshot, String> {
    let parsed = mcp_protocol::parse_mcp_servers_json(&args.json).map_err(|e| e.to_string())?;
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    if args.replace {
        cfg.mcp_servers.clear();
    }
    for profile in parsed {
        cfg.upsert_mcp_server(profile).map_err(|e| e.to_string())?;
    }
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

#[tauri::command]
pub fn add_promptstdio_mcp_preset() -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    const ID: &str = "promptstdio";
    if cfg.mcp_server(ID).is_some() {
        return Ok(config_to_snapshot(&cfg));
    }
    let base = cfg.api_base.trim().trim_end_matches('/');
    let base = if base.is_empty() {
        "https://www.promptstdio.com"
    } else {
        base
    };
    let url = format!("{base}/mcp");
    let mut headers = std::collections::HashMap::new();
    if let Some(tok) = cfg
        .api_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let value = if tok.to_ascii_lowercase().starts_with("bearer ") {
            tok.to_string()
        } else {
            format!("Bearer {tok}")
        };
        headers.insert("Authorization".into(), value);
    }
    cfg.upsert_mcp_server(McpServerProfile {
        id: ID.into(),
        label: "PromptStdio".into(),
        transport: "http".into(),
        enabled: false,
        command: None,
        args: Vec::new(),
        env: Default::default(),
        cwd: None,
        url: Some(url),
        headers,
    })
    .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

#[tauri::command]
pub fn delete_mcp_server(id: String) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    cfg.delete_mcp_server(id.trim())
        .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

#[tauri::command]
pub fn set_mcp_server_enabled(id: String, enabled: bool) -> Result<ConfigSnapshot, String> {
    let mut cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    cfg.set_mcp_server_enabled(id.trim(), enabled)
        .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(config_to_snapshot(&cfg))
}

#[tauri::command]
pub async fn test_mcp_server(id: String) -> Result<String, String> {
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let profile = cfg
        .mcp_server(id.trim())
        .cloned()
        .ok_or_else(|| format!("找不到 MCP 服务：{}", id.trim()))?;
    let tools = mcp_protocol::list_tools(&profile)
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "MCP 连接成功 · {} · 可用工具 {} 个",
        profile.label,
        tools.len()
    ))
}

/// Optional form overrides so settings can probe a typed key without saving it first.
#[derive(Debug, Deserialize, Default)]
pub struct TestConnectionArgs {
    pub llm_api_key: Option<String>,
    pub llm_api_base: Option<String>,
    pub llm_model: Option<String>,
    pub profile_id: Option<String>,
}

#[tauri::command]
pub async fn test_connection(args: Option<TestConnectionArgs>) -> Result<bool, String> {
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let args = args.unwrap_or_default();
    let profile = args
        .profile_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|id| cfg.profile(id));
    let api_key = args
        .llm_api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            profile
                .and_then(|p| p.api_key.clone())
                .filter(|k| !k.trim().is_empty())
        })
        .or_else(|| {
            cfg.llm_api_key
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .ok_or_else(|| "请先填写 API Key".to_string())?;
    let api_base = args
        .llm_api_base
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| profile.map(|p| p.api_base.as_str()))
        .unwrap_or(cfg.llm_api_base.as_str())
        .trim_end_matches('/')
        .to_string();
    let model_raw = args
        .llm_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| profile.map(|p| p.model.as_str()))
        .unwrap_or(cfg.llm_model.as_str());
    let model = stitch::config::migrate_llm_model(model_raw).unwrap_or(model_raw);
    let client = reqwest::Client::new();
    let url = format!("{api_base}/chat/completions");
    let mut body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "say ok"}],
        "max_tokens": 5,
        "stream": false,
    });
    if api_base.to_ascii_lowercase().contains("deepseek.com") || model.starts_with("deepseek-") {
        body["thinking"] = serde_json::json!({ "type": "disabled" });
    }
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("模型连接失败: {e}"))?;
    if resp.status().is_success() {
        Ok(true)
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(friendly_llm_test_error(status.as_u16(), &body))
    }
}

/// Official Skill catalog row for the desktop library (mirrors domain SkillSummary).
#[derive(Debug, Clone, Serialize)]
pub struct SkillSummaryRow {
    pub slug: String,
    pub title: String,
    pub description: String,
    pub version: String,
    pub install_phrase: String,
    pub sync_phrase: String,
    pub price_display: Option<String>,
}

/// Embedded catalog — keep titles/phrases aligned with `promptstdio-domain::skills`.
fn official_skills() -> Vec<SkillSummaryRow> {
    vec![
        SkillSummaryRow {
            slug: "pm-prd-demo".into(),
            title: "PM PRD 转 Demo".into(),
            description: "粘贴 PRD，拆架构并产出可演示前端。".into(),
            version: "2.0.1".into(),
            install_phrase:
                "帮我安装 PromptStdio 的「PM PRD 转 Demo」Skill，装到你认为合适的 Skill 目录".into(),
            sync_phrase:
                "帮我更新 PromptStdio 的「PM PRD 转 Demo」Skill，写到你认为合适的 Skill 目录".into(),
            price_display: None,
        },
        SkillSummaryRow {
            slug: "html-deck".into(),
            title: "HTML Deck".into(),
            description: "公开课或技术分享用 HTML 课件，按页修订。".into(),
            version: "0.8.4".into(),
            install_phrase:
                "帮我安装 PromptStdio 的「HTML Deck」Skill，装到你认为合适的 Skill 目录".into(),
            sync_phrase: "帮我更新 PromptStdio 的「HTML Deck」Skill，写到你认为合适的 Skill 目录"
                .into(),
            price_display: None,
        },
        SkillSummaryRow {
            slug: "asset-sediment-demo".into(),
            title: "资产沉淀动效".into(),
            description: "瑞士风单线程演示：碎片到工作流的复利动效。".into(),
            version: "0.1.0".into(),
            install_phrase:
                "帮我安装 PromptStdio 的「资产沉淀动效」Skill，装到你认为合适的 Skill 目录".into(),
            sync_phrase:
                "帮我更新 PromptStdio 的「资产沉淀动效」Skill，写到你认为合适的 Skill 目录".into(),
            price_display: Some("¥1.00".into()),
        },
    ]
}

#[tauri::command]
pub fn list_skills() -> Vec<SkillSummaryRow> {
    official_skills()
}

/// One Skill from workspace or user-global inventory (Cursor-compatible paths).
#[derive(Debug, Clone, Serialize)]
pub struct LocalSkillRow {
    pub slug: String,
    pub title: String,
    pub description: String,
    /// Display path, e.g. `.agents/skills/foo` or `~/.agents/skills/foo`.
    pub rel_path: String,
    /// `"workspace"` (project) or `"user"` (home / 本机安装).
    pub scope: String,
}

fn parse_skill_md_meta(body: &str) -> (Option<String>, Option<String>) {
    let trimmed = body.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return (None, None);
    };
    let Some(end) = rest.find("\n---") else {
        return (None, None);
    };
    let fm = &rest[..end];
    let mut name = None;
    let mut description = None;
    for line in fm.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("name:") {
            let t = v.trim().trim_matches('"').trim_matches('\'').trim();
            if !t.is_empty() {
                name = Some(t.to_string());
            }
        } else if let Some(v) = line.strip_prefix("description:") {
            let t = v.trim().trim_matches('"').trim_matches('\'').trim();
            if !t.is_empty() {
                description = Some(t.to_string());
            }
        }
    }
    (name, description)
}

fn user_home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
}

fn collect_skills_under(
    root: &std::path::Path,
    rel_base: &str,
    scope: &str,
    out: &mut Vec<LocalSkillRow>,
) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    dirs.sort_by_key(|e| e.file_name());
    for ent in dirs {
        let Ok(ft) = ent.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let slug = ent.file_name().to_string_lossy().to_string();
        if slug.starts_with('.') {
            continue;
        }
        let skill_md = ent.path().join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let body = std::fs::read_to_string(&skill_md).unwrap_or_default();
        let (name, desc) = parse_skill_md_meta(&body);
        let title = name.unwrap_or_else(|| slug.clone());
        let description = desc.unwrap_or_default();
        let rel_path = format!("{rel_base}/{slug}").replace('\\', "/");
        out.push(LocalSkillRow {
            slug,
            title,
            description,
            rel_path,
            scope: scope.to_string(),
        });
    }
}

/// Discover Skills: work dir first, then user home (Cursor-compatible).
///
/// Paths: `.agents/skills` · `.cursor/skills` · `~/.agents/skills` · `~/.cursor/skills`.
/// Same slug: workspace wins over user; within a scope `.agents` is scanned before `.cursor`.
pub(crate) fn discover_local_skills(
    work_dir: Option<&std::path::Path>,
    home: Option<&std::path::Path>,
) -> Vec<LocalSkillRow> {
    let mut out = Vec::new();
    if let Some(base) = work_dir
        && !base.as_os_str().is_empty()
    {
        collect_skills_under(
            &base.join(".agents").join("skills"),
            ".agents/skills",
            "workspace",
            &mut out,
        );
        collect_skills_under(
            &base.join(".cursor").join("skills"),
            ".cursor/skills",
            "workspace",
            &mut out,
        );
    }
    if let Some(home) = home {
        collect_skills_under(
            &home.join(".agents").join("skills"),
            "~/.agents/skills",
            "user",
            &mut out,
        );
        collect_skills_under(
            &home.join(".cursor").join("skills"),
            "~/.cursor/skills",
            "user",
            &mut out,
        );
    }
    // Dedupe by slug (workspace entries were collected first).
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(s.slug.clone()));
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

/// List Skills from the work directory and user-global install dirs.
#[tauri::command]
pub fn list_local_skills(
    work_dir_state: tauri::State<'_, WorkDirState>,
) -> Result<Vec<LocalSkillRow>, String> {
    let wd = work_dir_state
        .path
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let wd = wd.trim();
    let work = if wd.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(wd))
    };
    Ok(discover_local_skills(
        work.as_deref(),
        user_home_dir().as_deref(),
    ))
}

#[derive(serde::Serialize)]
pub struct ExportSkillResult {
    /// 导出后的完整路径（如 `D:\backup\my-skill`）。
    pub path: String,
    /// 复制的文件数。
    pub files: usize,
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(dst)?;
    let mut count = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            count += copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
            count += 1;
        }
    }
    Ok(count)
}

/// 导出 Skill 到用户选择的位置（资产主权：用户可随时带走自己的 Skill）。
#[tauri::command]
pub fn export_skill(
    slug: String,
    work_dir_state: tauri::State<'_, WorkDirState>,
) -> Result<ExportSkillResult, String> {
    let wd = work_dir_state
        .path
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let wd = wd.trim();
    let home = user_home_dir();

    // 与 list_local_skills 相同的四个候选位置，取第一个存在的目录。
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if !wd.is_empty() {
        let base = std::path::PathBuf::from(wd);
        candidates.push(base.join(".agents").join("skills").join(&slug));
        candidates.push(base.join(".cursor").join("skills").join(&slug));
    }
    if let Some(h) = home.as_deref() {
        candidates.push(h.join(".agents").join("skills").join(&slug));
        candidates.push(h.join(".cursor").join("skills").join(&slug));
    }
    let src = candidates
        .into_iter()
        .find(|p| p.is_dir())
        .ok_or_else(|| format!("未找到 Skill：{slug}"))?;

    let dir = rfd::FileDialog::new()
        .set_title("选择导出位置")
        .pick_folder()
        .ok_or_else(|| "已取消".to_string())?;
    let dest = dir.join(&slug);
    if dest.exists() {
        return Err(format!("目标位置已存在同名目录：{}", dest.display()));
    }
    let files = copy_dir_recursive(&src, &dest).map_err(|e| format!("导出失败：{e}"))?;
    Ok(ExportSkillResult {
        path: dest.display().to_string(),
        files,
    })
}

/// Short L1 errors for settings UI (avoid dumping raw JSON to users).
fn friendly_llm_test_error(status: u16, body: &str) -> String {
    let lower = body.to_ascii_lowercase();
    if status == 401
        || lower.contains("authentication fails")
        || lower.contains("invalid api key")
        || lower.contains("incorrect api key")
        || lower.contains("unauthorized")
    {
        return "API Key 无效，请检查后重试".into();
    }
    if lower.contains("model")
        && (lower.contains("not found")
            || lower.contains("does not exist")
            || lower.contains("invalid_model"))
    {
        return "模型名称不可用，请更换后重试".into();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body)
        && let Some(msg) = v
            .pointer("/error/message")
            .and_then(|x| x.as_str())
            .or_else(|| v.get("message").and_then(|x| x.as_str()))
    {
        let m = msg.to_ascii_lowercase();
        if m.contains("authentication") || m.contains("api key") {
            return "API Key 无效，请检查后重试".into();
        }
    }
    if body.chars().count() > 160 {
        return format!("模型连接失败（HTTP {status}）");
    }
    if body.trim().is_empty() {
        return format!("模型连接失败（HTTP {status}）");
    }
    format!("模型连接失败: {}", body.trim())
}

/// Probe PromptStdio REST with saved or form-override credentials (list suites page 1).
#[derive(Debug, Deserialize, Default)]
pub struct TestPromptstdioArgs {
    pub profile_id: Option<String>,
    pub api_token: Option<String>,
    pub api_base: Option<String>,
}

#[tauri::command]
pub async fn test_promptstdio(args: Option<TestPromptstdioArgs>) -> Result<String, String> {
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let args = args.unwrap_or_default();
    let override_token = args
        .api_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let (api_base, api_token) = if let Some(token) = override_token {
        let base = args
            .api_base
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_end_matches('/').to_string())
            .or_else(|| {
                args.profile_id
                    .as_deref()
                    .and_then(|id| cfg.mcp_profile(id))
                    .map(|p| p.api_base.clone())
            })
            .unwrap_or_else(|| {
                if cfg.api_base.trim().is_empty() {
                    "https://www.promptstdio.com".into()
                } else {
                    cfg.api_base.clone()
                }
            });
        (base, token.to_string())
    } else {
        let resolved = cfg
            .resolve_mcp(args.profile_id.as_deref())
            .map_err(|e| e.to_string())?;
        let base = args
            .api_base
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or(resolved.api_base);
        (base, resolved.api_token)
    };
    let client = mcp::McpClient::new(api_base.clone(), Some(api_token));
    let suites = client
        .list_suites(Some(1), Some(1))
        .await
        .map_err(|e| format!("PromptStdio 连接失败（{api_base}）: {e}"))?;
    Ok(format!(
        "PromptStdio 连接成功 · {api_base} · 套件列表可读（本页 {} 条）",
        suites.len()
    ))
}

#[tauri::command]
pub fn cancel_generation(state: tauri::State<CancelState>) -> Result<(), String> {
    state.flag.store(true, Ordering::SeqCst);
    state.notify.notify_one();
    tracing::info!("agent generation cancel requested");
    Ok(())
}

#[tauri::command]
pub fn respond_confirmation(
    state: tauri::State<ConfirmState>,
    id: String,
    approved: bool,
    remember: Option<stitch::allow::AllowRule>,
) -> Result<(), String> {
    {
        let mut guard = state.pending.lock().map_err(|e| e.to_string())?;
        if let Some(tx) = guard.remove(&id) {
            let _ = tx.send(approved);
            tracing::info!(%id, approved, "confirmation response");
        }
    }
    //「记住此规则」: persist a normalized rule so the same scope skips
    // confirmation from the next call on (same turn included).
    if approved && let Some(rule) = remember.and_then(stitch::allow::AllowRules::normalize) {
        let mut rules = state.rules.lock().map_err(|e| e.to_string())?;
        if rules.add(rule.clone()) {
            if let Err(e) = rules.save() {
                tracing::warn!(error = %e, "failed to persist allow rule");
            }
            tracing::info!(
                tool = %rule.tool,
                scope = %rule.scope,
                value = %rule.value,
                "allow rule remembered"
            );
        }
    }
    Ok(())
}

/// Settings UI: current allow rules (tool + scope + value triples).
#[tauri::command]
pub fn get_allow_rules(
    state: tauri::State<'_, ConfirmState>,
) -> Result<Vec<stitch::allow::AllowRule>, String> {
    let rules = state.rules.lock().map_err(|e| e.to_string())?;
    Ok(rules.rules.clone())
}

/// Settings UI: remove one rule (exact triple match, idempotent). Returns
/// the updated list so the frontend refreshes in one round-trip.
#[tauri::command]
pub fn remove_allow_rule(
    state: tauri::State<'_, ConfirmState>,
    tool: String,
    scope: String,
    value: String,
) -> Result<Vec<stitch::allow::AllowRule>, String> {
    let mut rules = state.rules.lock().map_err(|e| e.to_string())?;
    if rules.remove(tool.trim(), scope.trim(), value.trim())
        && let Err(e) = rules.save()
    {
        tracing::warn!(error = %e, "failed to persist allow rule removal");
    }
    Ok(rules.rules.clone())
}

/// Settings UI: clear all allow rules. Returns the (now empty) list.
#[tauri::command]
pub fn clear_allow_rules(
    state: tauri::State<'_, ConfirmState>,
) -> Result<Vec<stitch::allow::AllowRule>, String> {
    let mut rules = state.rules.lock().map_err(|e| e.to_string())?;
    rules.clear();
    if let Err(e) = rules.save() {
        tracing::warn!(error = %e, "failed to persist allow rules clear");
    }
    Ok(rules.rules.clone())
}

#[tauri::command]
pub fn respond_plan(
    state: tauri::State<PlanState>,
    id: String,
    approved: bool,
) -> Result<(), String> {
    let mut guard = state.pending.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = guard.remove(&id) {
        let _ = tx.send(approved);
        tracing::info!(%id, approved, "plan response");
    }
    Ok(())
}

#[derive(Default, Clone)]
pub struct WorkDirState {
    pub path: Arc<Mutex<String>>,
}

impl WorkDirState {
    pub fn new() -> Self {
        Self {
            path: Arc::new(Mutex::new(resolve_initial_work_dir())),
        }
    }
}

#[tauri::command]
pub fn get_work_dir(state: tauri::State<WorkDirState>) -> Result<String, String> {
    Ok(state.path.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
pub fn set_work_dir(state: tauri::State<WorkDirState>, path: String) -> Result<String, String> {
    let p = PathBuf::from(path.trim());
    if !p.exists() {
        return Err(format!("目录不存在: {path}"));
    }
    if !p.is_dir() {
        return Err(format!("不是目录: {path}"));
    }
    let canonical = p.canonicalize().map_err(|e| e.to_string())?;
    let canon_str = display_path(&canonical);
    tracing::info!(path = %canon_str, "work directory changed");
    *state.path.lock().map_err(|e| e.to_string())? = canon_str.clone();
    persist_work_dir(&canon_str);
    Ok(canon_str)
}

#[tauri::command]
pub fn browse_work_dir() -> Result<Option<String>, String> {
    // Native OS dialog via rfd (avoid spawning PowerShell each time — slow & janky).
    let folder = rfd::FileDialog::new()
        .set_title("选择项目工作目录")
        .pick_folder();
    Ok(folder.map(|p| display_path(&p)))
}

/// Open a local directory in the OS file manager (Explorer / Finder / xdg-open).
#[tauri::command]
pub fn open_folder_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(path.trim());
    if !p.exists() {
        return Err(format!("目录不存在: {path}"));
    }
    if !p.is_dir() {
        return Err(format!("不是目录: {path}"));
    }
    let shown = display_path(&p);
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&shown)
            .spawn()
            .map_err(|e| format!("无法打开文件夹: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&shown)
            .spawn()
            .map_err(|e| format!("无法打开文件夹: {e}"))?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&shown)
            .spawn()
            .map_err(|e| format!("无法打开文件夹: {e}"))?;
        Ok(())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = shown;
        Err("当前平台不支持打开文件夹".into())
    }
}

async fn wait_plan_approval(
    plan_state: &PlanState,
    plan_id: &str,
    cancel_flag: &AtomicBool,
) -> Result<bool, String> {
    let (tx, rx) = oneshot::channel();
    {
        let mut guard = plan_state.pending.lock().map_err(|e| e.to_string())?;
        guard.insert(plan_id.to_string(), tx);
    }
    let approved = tokio::select! {
        result = rx => result.unwrap_or(false),
        _ = async {
            loop {
                if cancel_flag.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        } => false,
        _ = tokio::time::sleep(std::time::Duration::from_secs(300)) => false,
    };
    let mut guard = plan_state.pending.lock().map_err(|e| e.to_string())?;
    guard.remove(plan_id);
    Ok(approved)
}

async fn pump_agent_events(
    app: &tauri::AppHandle,
    event_rx: &mut tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    cancel_notify: &tokio::sync::Notify,
    agent_handle: tokio::task::JoinHandle<(anyhow::Result<agent::AgentResult>, Session)>,
    cancel_flag: &AtomicBool,
) -> Result<Option<Session>, String> {
    pump_agent_events_opts(
        app,
        event_rx,
        cancel_notify,
        agent_handle,
        cancel_flag,
        true,
    )
    .await
}

/// Pump agent events to the UI.
/// When `forward_done` is false (intermediate plan steps), `Done` is not
/// emitted so the chat stays locked until the final segment.
async fn pump_agent_events_opts(
    app: &tauri::AppHandle,
    event_rx: &mut tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    cancel_notify: &tokio::sync::Notify,
    agent_handle: tokio::task::JoinHandle<(anyhow::Result<agent::AgentResult>, Session)>,
    cancel_flag: &AtomicBool,
    forward_done: bool,
) -> Result<Option<Session>, String> {
    let mut saw_done = false;
    let mut last_response = String::new();
    loop {
        tokio::select! {
            event = event_rx.recv() => {
                match event {
                    Some(event) => {
                        if let AgentEvent::Done { ref response, .. } = event {
                            saw_done = true;
                            last_response = response.clone();
                            if !forward_done {
                                continue;
                            }
                        }
                        let payload = serde_json::to_value(&event).unwrap_or_default();
                        let _ = app.emit("agent-event", payload);
                    }
                    None => {
                        break;
                    }
                }
            }
            _ = cancel_notify.notified() => {
                emit_cancelled(app);
                agent_handle.abort();
                return Ok(None);
            }
        }
    }

    if cancel_flag.load(Ordering::SeqCst) {
        emit_cancelled(app);
        return Ok(None);
    }

    match agent_handle.await {
        Ok((Ok(result), session)) => {
            if forward_done && !saw_done {
                let _ = app.emit(
                    "agent-event",
                    serde_json::to_value(AgentEvent::Done {
                        response: result.response,
                        iterations: result.iterations,
                        input_tokens: result.input_tokens,
                        output_tokens: result.output_tokens,
                        context_tokens: result.context_tokens,
                        context_limit: result.context_limit,
                        hit_iteration_cap: false,
                    })
                    .unwrap_or_default(),
                );
            } else if !forward_done {
                let _ = last_response;
            }
            Ok(Some(session))
        }
        Ok((Err(e), session)) => {
            let msg = format!("{e:#}");
            if cancel_flag.load(Ordering::SeqCst)
                || msg.to_ascii_lowercase().contains("cancelled")
                || msg.to_ascii_lowercase().contains("canceled")
            {
                emit_cancelled(app);
                Ok(None)
            } else {
                let _ = app.emit(
                    "agent-event",
                    serde_json::json!({
                        "type": "error",
                        "message": msg,
                    }),
                );
                Ok(Some(session))
            }
        }
        Err(e) => {
            let msg = format!("Agent task panicked: {e}");
            let _ = app.emit(
                "agent-event",
                serde_json::json!({
                    "type": "error",
                    "message": msg,
                }),
            );
            Ok(None)
        }
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // 历史 API 签名
pub async fn send_message(
    app: tauri::AppHandle,
    state: tauri::State<'_, CancelState>,
    confirm_state: tauri::State<'_, ConfirmState>,
    plan_state: tauri::State<'_, PlanState>,
    work_dir_state: tauri::State<'_, WorkDirState>,
    session_store: tauri::State<'_, AgentSessionStore>,
    message: String,
    images: Option<Vec<String>>,
    history: Option<Vec<HistoryMessage>>,
    plan_mode: Option<bool>,
    profile_id: Option<String>,
    model: Option<String>,
    chat_session_id: Option<String>,
    resume: Option<bool>,
    rewind_to_user: Option<String>,
    rewind_drop: Option<bool>,
    recording: Option<bool>,
) -> Result<(), String> {
    let _busy = try_acquire_generation(&state)?;
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let resolved = cfg
        .resolve_llm(
            profile_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            model.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        )
        .map_err(|e| e.to_string())?;
    let api_key = resolved.api_key;
    let model = resolved.model;
    let api_base = resolved.api_base;
    // Image payload sanity first — a bad image should not waste a describe call.
    let imgs: Vec<String> = images.unwrap_or_default();
    if imgs.len() > 9 {
        return Err("单条消息最多 9 张图片。".into());
    }
    for u in &imgs {
        if u.len() > 6_000_000 || !u.starts_with("data:image/") {
            return Err("图片数据不合法（需 data:image/ 前缀且不超过 6MB）。".into());
        }
    }
    // Vision gate + local describe layer: a vision-capable model receives the
    // images directly; otherwise the local vision model (Ollama qwen3-vl by
    // default) describes them and the text-only model gets the description.
    let mut effective_message = message.clone();
    let effective_images: Option<&[String]> = if imgs.is_empty()
        || stitch::agent::tokens::model_supports_vision(&model)
    {
        (!imgs.is_empty()).then_some(imgs.as_slice())
    } else {
        if !cfg.local_vision.enabled {
            return Err(
                "当前模型不支持图片输入。可在模型设置中启用本地视觉描述（Ollama qwen3-vl 等）。"
                    .into(),
            );
        }
        let timeout = Duration::from_secs(cfg.local_vision.timeout_secs.max(1));
        // Global deadline: images beyond it degrade to placeholders.
        let deadline = std::time::Instant::now() + timeout;
        let mut descriptions: Vec<Option<String>> = Vec::with_capacity(imgs.len());
        let mut failed_reason: Option<String> = None;
        for url in &imgs {
            if std::time::Instant::now() >= deadline {
                descriptions.push(None);
                if failed_reason.is_none() {
                    failed_reason = Some("描述超时".into());
                }
                continue;
            }
            match stitch::llm::vision::describe_image(
                &cfg.local_vision.api_base,
                &cfg.local_vision.model,
                url,
                timeout,
            )
            .await
            {
                Ok(d) => {
                    tracing::info!(desc_len = d.len(), "local vision described image");
                    descriptions.push(Some(d));
                }
                Err(e) => {
                    let reason = match e {
                        stitch::llm::vision::DescribeFailure::Unreachable => {
                            "本地视觉服务未运行".into()
                        }
                        stitch::llm::vision::DescribeFailure::Timeout => "描述超时".into(),
                        stitch::llm::vision::DescribeFailure::Http { status, .. } => {
                            format!("本地视觉服务错误 {status}")
                        }
                        stitch::llm::vision::DescribeFailure::Parse
                        | stitch::llm::vision::DescribeFailure::Empty => "描述结果异常".into(),
                    };
                    if failed_reason.is_none() {
                        failed_reason = Some(reason);
                    }
                    descriptions.push(None);
                }
            }
        }
        effective_message = stitch::llm::vision::compose_description_text(&message, &descriptions);
        if let Some(reason) = failed_reason {
            tracing::warn!(%reason, "local vision describe degraded");
            let _ = app.emit(
                "agent-event",
                serde_json::json!({
                    "type": "notice",
                    "message": format!("本地视觉描述失败，已降级发送：{reason}"),
                }),
            );
        }
        None
    };
    let max_iterations = cfg.max_iterations;
    let plan_mode = plan_mode.unwrap_or(false);
    let resume = resume.unwrap_or(false);
    let chat_id = chat_session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let work_dir = work_dir_state
        .path
        .lock()
        .map_err(|e| e.to_string())?
        .clone();

    let is_recording = recording.unwrap_or(false);
    if is_recording {
        tracing::info!("skill recording mode active for turn");
    }
    let tools = build_agent_registry(&work_dir, &cfg).await;
    let system_prompt = agent::prompt::build_system_prompt(&work_dir, &tools);

    // History carries image data URLs only when the model receives images
    // directly; the describe path sends a stripped copy (text-only).
    let history_effective: Option<Vec<HistoryMessage>> = if effective_images.is_none() {
        history.map(|h| {
            h.iter()
                .map(|m| HistoryMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                    images: None,
                })
                .collect()
        })
    } else {
        history
    };
    let mut session = resolve_agent_session(
        &session_store,
        &work_dir,
        chat_id.as_deref(),
        system_prompt,
        history_effective.as_deref(),
        &effective_message,
        effective_images,
        resume,
        rewind_to_user.as_deref(),
        rewind_drop.unwrap_or(false),
    );
    let rollback = session_before_turn(
        &session,
        &effective_message,
        effective_images.unwrap_or(&[]),
        resume,
    );

    // Mid-turn crash-safe persistence: flushed incrementally inside the agent
    // loop; rolled back on user stop so "stop = discard this turn" holds.
    let turn_flusher: Option<Arc<Mutex<persist::TurnFlusher>>> = chat_id
        .as_deref()
        .filter(|_| !work_dir.trim().is_empty())
        .and_then(|id| {
            persist::TurnFlusher::begin(std::path::Path::new(&work_dir), id, &rollback)
                .map(|f| Arc::new(Mutex::new(f)))
        });

    state.flag.store(false, Ordering::SeqCst);
    let cancel_flag = state.flag.clone();
    let cancel_notify = state.notify.clone();

    if plan_mode {
        let plan_session = session.clone();
        let plan = agent::plan::generate_plan(&plan_session, &api_base, &model, &api_key)
            .await
            .map_err(|e| e.to_string())?;

        if cancel_flag.load(Ordering::SeqCst) {
            emit_cancelled(&app);
            if let Some(id) = chat_id.as_deref() {
                rollback_turn_disk(&turn_flusher, &rollback);
                session_store.put(id, rollback);
            }
            return Ok(());
        }

        let plan = if plan.is_empty() {
            tracing::warn!("plan mode produced empty plan; using single-step fallback");
            agent::plan::Plan {
                title: Some("执行计划".into()),
                steps: vec![agent::plan::PlanStep {
                    description: message.clone(),
                    status: agent::plan::PlanStepStatus::Pending,
                }],
            }
        } else {
            plan
        };

        let plan_id = new_plan_id();
        let _ = app.emit(
            "agent-event",
            serde_json::to_value(AgentEvent::PlanProposed {
                id: plan_id.clone(),
                plan: plan.clone(),
            })
            .unwrap_or_default(),
        );

        let approved = wait_plan_approval(&plan_state, &plan_id, &cancel_flag).await?;
        if cancel_flag.load(Ordering::SeqCst) {
            emit_cancelled(&app);
            return Ok(());
        }
        if !approved {
            let _ = app.emit(
                "agent-event",
                serde_json::to_value(AgentEvent::PlanRejected).unwrap_or_default(),
            );
            let _ = app.emit(
                "agent-event",
                serde_json::json!({
                    "type": "done",
                    "response": "计划已拒绝，未执行。",
                    "iterations": 0,
                }),
            );
            return Ok(());
        }

        let _ = app.emit(
            "agent-event",
            serde_json::to_value(AgentEvent::PlanApproved).unwrap_or_default(),
        );

        session.add_assistant_message(format!("已批准执行计划：\n\n{}", plan.format()));

        let step_budget = max_iterations.clamp(4, 12);
        let total_steps = plan.steps.len();
        for (i, step) in plan.steps.iter().enumerate() {
            if cancel_flag.load(Ordering::SeqCst) {
                emit_cancelled(&app);
                if let Some(id) = chat_id.as_deref() {
                    rollback_turn_disk(&turn_flusher, &rollback);
                    session_store.put(id, rollback.clone());
                }
                return Ok(());
            }

            let _ = app.emit(
                "agent-event",
                serde_json::to_value(AgentEvent::PlanStepStart {
                    index: i,
                    description: step.description.clone(),
                })
                .unwrap_or_default(),
            );

            session.add_user_message(agent::plan::step_execution_prompt(
                i,
                total_steps,
                &step.description,
            ));

            let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
            let agent_flag = cancel_flag.clone();
            let confirm_pending = confirm_state.pending.clone();
            let rules_s = confirm_state.rules.clone();
            let work_dir_s = work_dir.clone();
            let api_base_s = api_base.clone();
            let model_s = model.clone();
            let api_key_s = api_key.clone();
            let tools_s = tools.clone();
            let flusher_s = turn_flusher.clone();
            let agent_handle = tokio::spawn(async move {
                let result = agent::run_react_streaming(
                    &mut session,
                    &api_base_s,
                    &model_s,
                    &api_key_s,
                    &tools_s,
                    step_budget,
                    confirm_pending,
                    Some(work_dir_s.as_str()),
                    rules_s,
                    &event_tx,
                    &agent_flag,
                    flusher_s.as_ref(),
                )
                .await;
                (result, session)
            });

            let finished = pump_agent_events_opts(
                &app,
                &mut event_rx,
                &cancel_notify,
                agent_handle,
                &cancel_flag,
                false,
            )
            .await?;

            let Some(s) = finished else {
                if let Some(id) = chat_id.as_deref() {
                    rollback_turn_disk(&turn_flusher, &rollback);
                    session_store.put(id, rollback.clone());
                }
                return Ok(());
            };
            session = s;

            let _ = app.emit(
                "agent-event",
                serde_json::to_value(AgentEvent::PlanStepDone {
                    index: i,
                    description: step.description.clone(),
                })
                .unwrap_or_default(),
            );
        }

        if cancel_flag.load(Ordering::SeqCst) {
            emit_cancelled(&app);
            if let Some(id) = chat_id.as_deref() {
                rollback_turn_disk(&turn_flusher, &rollback);
                session_store.put(id, rollback.clone());
            }
            return Ok(());
        }

        session.add_user_message(agent::plan::plan_summary_prompt());
        let summary_budget = max_iterations.clamp(3, 6);
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
        let agent_flag = cancel_flag.clone();
        let confirm_pending = confirm_state.pending.clone();
        let rules_s = confirm_state.rules.clone();
        let work_dir_s = work_dir.clone();
        let flusher_s = turn_flusher.clone();
        let agent_handle = tokio::spawn(async move {
            let result = agent::run_react_streaming(
                &mut session,
                &api_base,
                &model,
                &api_key,
                &tools,
                summary_budget,
                confirm_pending,
                Some(work_dir_s.as_str()),
                rules_s,
                &event_tx,
                &agent_flag,
                flusher_s.as_ref(),
            )
            .await;
            (result, session)
        });

        let finished = pump_agent_events_opts(
            &app,
            &mut event_rx,
            &cancel_notify,
            agent_handle,
            &cancel_flag,
            true,
        )
        .await?;

        match finished {
            Some(s) => {
                if let Some(id) = chat_id.as_deref() {
                    persist_agent_session(&work_dir, id, &s);
                    session_store.put(id, s);
                }
            }
            None => {
                if let Some(id) = chat_id.as_deref() {
                    rollback_turn_disk(&turn_flusher, &rollback);
                    session_store.put(id, rollback);
                }
            }
        }
        return Ok(());
    }

    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
    let agent_flag = cancel_flag.clone();
    let confirm_pending = confirm_state.pending.clone();
    let rules_s = confirm_state.rules.clone();
    let work_dir_s = work_dir.clone();
    let flusher_s = turn_flusher.clone();
    let agent_handle = tokio::spawn(async move {
        let result = agent::run_react_streaming(
            &mut session,
            &api_base,
            &model,
            &api_key,
            &tools,
            max_iterations,
            confirm_pending,
            Some(work_dir_s.as_str()),
            rules_s,
            &event_tx,
            &agent_flag,
            flusher_s.as_ref(),
        )
        .await;
        (result, session)
    });

    let finished = pump_agent_events(
        &app,
        &mut event_rx,
        &cancel_notify,
        agent_handle,
        &cancel_flag,
    )
    .await?;

    match finished {
        Some(s) => {
            if let Some(id) = chat_id.as_deref() {
                persist_agent_session(&work_dir, id, &s);
                session_store.put(id, s);
            }
        }
        None => {
            if let Some(id) = chat_id.as_deref() {
                rollback_turn_disk(&turn_flusher, &rollback);
                session_store.put(id, rollback);
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub fn clear_agent_session(
    work_dir_state: tauri::State<'_, WorkDirState>,
    session_store: tauri::State<'_, AgentSessionStore>,
    chat_session_id: String,
) -> Result<(), String> {
    let id = chat_session_id.trim();
    if id.is_empty() {
        return Ok(());
    }
    session_store.remove(id);
    if let Ok(guard) = work_dir_state.path.lock() {
        let wd = guard.clone();
        if !wd.trim().is_empty()
            && let Some(dir) = persist::session_dir(std::path::Path::new(&wd), id)
            && let Err(e) = persist::delete_session_dir(&dir)
        {
            tracing::warn!(%e, %id, "failed to delete persisted agent session");
        }
    }
    Ok(())
}

/// Drop in-memory Agent session only (keep disk). Used by e2e to simulate restart restore.
#[tauri::command]
pub fn drop_agent_memory(
    session_store: tauri::State<'_, AgentSessionStore>,
    chat_session_id: String,
) -> Result<(), String> {
    let id = chat_session_id.trim();
    if id.is_empty() {
        return Ok(());
    }
    session_store.remove(id);
    Ok(())
}

fn resolve_persist_dir(
    work_dir_state: &WorkDirState,
    chat_session_id: &str,
) -> Result<std::path::PathBuf, String> {
    let id = chat_session_id.trim();
    if id.is_empty() {
        return Err("会话 id 为空".into());
    }
    let wd = work_dir_state
        .path
        .lock()
        .map_err(|_| "工作区锁定失败".to_string())?
        .clone();
    if wd.trim().is_empty() {
        return Err("未绑定工作区".into());
    }
    persist::session_dir(std::path::Path::new(&wd), id).ok_or_else(|| "无效会话 id".into())
}

fn load_manifest_or_new(dir: &std::path::Path, chat_id: &str, work_dir: &str) -> Manifest {
    let man_path = dir.join("manifest.json");
    if man_path.is_file()
        && let Ok(raw) = std::fs::read_to_string(&man_path)
        && let Ok(m) = serde_json::from_str::<Manifest>(&raw)
    {
        return m;
    }
    Manifest::new(chat_id, std::path::Path::new(work_dir))
}

#[derive(Debug, Serialize)]
pub struct CheckpointSummaryDto {
    pub epoch: u32,
    pub parent_epoch: u32,
    pub compression_level: String,
    pub summary_preview: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct CheckpointDiffDto {
    pub from_epoch: u32,
    pub to_epoch: u32,
    pub summary_changed: bool,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct RollbackResultDto {
    pub epoch: u32,
    pub summary: String,
    pub resume_text: String,
}

/// List on-disk checkpoints for the current work dir session (newest first).
#[tauri::command]
pub fn list_session_checkpoints(
    work_dir_state: tauri::State<'_, WorkDirState>,
    chat_session_id: String,
) -> Result<Vec<CheckpointSummaryDto>, String> {
    let dir = resolve_persist_dir(&work_dir_state, &chat_session_id)?;
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let list = persist::list_checkpoints(&dir).map_err(|e| e.to_string())?;
    Ok(list
        .into_iter()
        .map(|c| CheckpointSummaryDto {
            epoch: c.epoch,
            parent_epoch: c.parent_epoch,
            compression_level: c.compression_level,
            summary_preview: c.summary_preview,
            created_at: c.created_at,
        })
        .collect())
}

/// Diff two checkpoint epochs (structured + L1 text).
#[tauri::command]
pub fn diff_session_checkpoints(
    work_dir_state: tauri::State<'_, WorkDirState>,
    chat_session_id: String,
    from_epoch: u32,
    to_epoch: u32,
) -> Result<CheckpointDiffDto, String> {
    let dir = resolve_persist_dir(&work_dir_state, &chat_session_id)?;
    let diff = persist::diff_checkpoints(&dir, from_epoch, to_epoch).map_err(|e| e.to_string())?;
    Ok(CheckpointDiffDto {
        from_epoch: diff.from_epoch,
        to_epoch: diff.to_epoch,
        summary_changed: diff.summary_changed,
        text: diff.text,
    })
}

/// Manual rollback to an older checkpoint; drops newer epochs and clears memory store.
#[tauri::command]
pub fn rollback_session_epoch(
    work_dir_state: tauri::State<'_, WorkDirState>,
    session_store: tauri::State<'_, AgentSessionStore>,
    chat_session_id: String,
    target_epoch: u32,
) -> Result<RollbackResultDto, String> {
    let id = chat_session_id.trim().to_string();
    let dir = resolve_persist_dir(&work_dir_state, &id)?;
    if !dir.is_dir() {
        return Err("会话尚未落盘".into());
    }
    let wd = work_dir_state
        .path
        .lock()
        .map_err(|_| "工作区锁定失败".to_string())?
        .clone();
    let mut manifest = load_manifest_or_new(&dir, &id, &wd);
    let (_session, cp) =
        persist::rollback_to_epoch(&dir, target_epoch, &mut manifest).map_err(|e| e.to_string())?;
    session_store.remove(&id);
    Ok(RollbackResultDto {
        epoch: cp.epoch,
        summary: cp.summary_natural.clone(),
        resume_text: persist::format_checkpoint_for_resume(&cp),
    })
}

/// Silent GC: remove `.stitch/sessions/<id>` not listed in `keep_ids` under `work_dir`.
#[tauri::command]
pub fn gc_orphan_agent_sessions(work_dir: String, keep_ids: Vec<String>) -> Result<usize, String> {
    let wd = work_dir.trim();
    if wd.is_empty() {
        return Ok(0);
    }
    let path = std::path::Path::new(wd);
    if !path.is_dir() {
        return Ok(0);
    }
    persist::gc_orphan_sessions(path, &keep_ids).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct WorkspaceCheckpointDto {
    pub session_id: String,
    pub epoch: u32,
    pub summary_preview: String,
    pub resume_text: String,
    pub created_at: String,
}

/// Newest checkpoint in a work dir (for optional「载入上一检查点」).
#[tauri::command]
pub fn latest_workspace_checkpoint(
    work_dir: String,
    exclude_session_id: Option<String>,
) -> Result<Option<WorkspaceCheckpointDto>, String> {
    let wd = work_dir.trim();
    if wd.is_empty() {
        return Ok(None);
    }
    let path = std::path::Path::new(wd);
    if !path.is_dir() {
        return Ok(None);
    }
    let exclude = exclude_session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let found = persist::latest_workspace_checkpoint(path, exclude).map_err(|e| e.to_string())?;
    Ok(found.map(|r| WorkspaceCheckpointDto {
        session_id: r.session_id,
        epoch: r.epoch,
        summary_preview: r.summary_preview,
        resume_text: r.resume_text,
        created_at: r.created_at,
    }))
}

#[tauri::command]
pub async fn list_suites() -> Result<Vec<SuiteSummary>, String> {
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let client = mcp_client(&cfg)?;
    client
        .list_suites(Some(1), Some(50))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_agents() -> Result<Vec<AgentSummary>, String> {
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let client = mcp_client(&cfg)?;
    client
        .list_agents(Some(1), Some(50))
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct CreatePromptArgs {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct CreatePromptResult {
    pub id: String,
    pub title: String,
}

/// Save a personal prompt to PromptStdio (requires account Token).
#[tauri::command]
pub async fn create_prompt(args: CreatePromptArgs) -> Result<CreatePromptResult, String> {
    let title = args.title.trim();
    let content = args.content.trim();
    if title.is_empty() {
        return Err("标题不能为空".into());
    }
    if content.is_empty() {
        return Err("内容不能为空".into());
    }
    let title = title.chars().take(255).collect::<String>();
    let content = content.chars().take(5000).collect::<String>();
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let client = mcp_client(&cfg)?;
    let created = client
        .create_prompt(&title, &content, args.description.as_deref(), args.tags)
        .await
        .map_err(|e| e.to_string())?;
    Ok(CreatePromptResult {
        id: created.id,
        title: created.title,
    })
}

#[derive(Debug, Deserialize)]
pub struct SubmitExploreArgs {
    pub prompt_id: String,
}

#[derive(Debug, Serialize)]
pub struct SubmitExploreResult {
    pub system_prompt_id: String,
    pub slug: String,
    pub status: String,
    pub already_submitted: bool,
}

/// Submit a personal prompt for Explore review (requires account Token).
#[tauri::command]
pub async fn submit_explore(args: SubmitExploreArgs) -> Result<SubmitExploreResult, String> {
    let id = args.prompt_id.trim();
    if id.is_empty() {
        return Err("提示词 id 不能为空".into());
    }
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let client = mcp_client(&cfg)?;
    let submitted = client.submit_explore(id).await.map_err(|e| e.to_string())?;
    Ok(SubmitExploreResult {
        system_prompt_id: submitted.system_prompt_id,
        slug: submitted.slug,
        status: submitted.status,
        already_submitted: submitted.already_submitted,
    })
}

#[tauri::command]
pub async fn run_suite(
    app: tauri::AppHandle,
    state: tauri::State<'_, CancelState>,
    confirm_state: tauri::State<'_, ConfirmState>,
    work_dir_state: tauri::State<'_, WorkDirState>,
    id: String,
    profile_id: Option<String>,
    model: Option<String>,
) -> Result<(), String> {
    let _busy = try_acquire_generation(&state)?;
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let client = mcp_client(&cfg)?;
    let suite = client.get_suite(&id).await.map_err(|e| e.to_string())?;
    if suite.steps.is_empty() {
        return Err("套件没有可执行的步骤".into());
    }

    let suite_track_id = suite.id.clone();
    spawn_track(
        "stitch_suite_run",
        serde_json::json!({ "task_suite_id": suite_track_id }),
    );

    let resolved = cfg
        .resolve_llm(
            profile_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            model.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        )
        .map_err(|e| e.to_string())?;
    let api_key = resolved.api_key;
    let model = resolved.model;
    let api_base = resolved.api_base;
    let max_iterations = cfg.max_iterations;
    let work_dir = work_dir_state
        .path
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    state.flag.store(false, Ordering::SeqCst);
    let cancel_flag = state.flag.clone();
    let cancel_notify = state.notify.clone();

    let mut combined = String::new();
    let total_steps = suite.steps.len();
    let mut suite_outcome = "done";

    for (idx, step) in suite.steps.iter().enumerate() {
        if cancel_flag.load(Ordering::SeqCst) {
            suite_outcome = "cancelled";
            break;
        }

        let _ = app.emit(
            "agent-event",
            serde_json::to_value(AgentEvent::PlanStepStart {
                index: idx,
                description: step.step_title.clone(),
            })
            .unwrap_or_default(),
        );

        let tools = build_agent_registry(&work_dir, &cfg).await;
        let system_prompt = agent::prompt::build_system_prompt(&work_dir, &tools);
        let mut session = Session::new(system_prompt);
        let user_message = format!(
            "你正在执行任务套件「{}」的步骤 {}/{}。\n\n任务说明：{}\n\n请执行以下步骤，完成后简要报告结果：\n\n## {}\n\n{}",
            suite.title,
            step.position,
            suite.step_count,
            suite.description.as_deref().unwrap_or("无"),
            step.step_title,
            step.content,
        );
        session.add_user_message(&user_message);

        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
        let agent_flag = cancel_flag.clone();
        let confirm_pending = confirm_state.pending.clone();
        let rules_s = confirm_state.rules.clone();
        let work_dir_s = work_dir.clone();
        let api_base_c = api_base.clone();
        let model_c = model.clone();
        let api_key_c = api_key.clone();
        let agent_handle = tokio::spawn(async move {
            let result = agent::run_react_streaming(
                &mut session,
                &api_base_c,
                &model_c,
                &api_key_c,
                &tools,
                max_iterations,
                confirm_pending,
                Some(work_dir_s.as_str()),
                rules_s,
                &event_tx,
                &agent_flag,
                None,
            )
            .await;
            (result, session)
        });

        // Forward events but suppress intermediate "done" until last step.
        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(AgentEvent::Done { response, .. }) => {
                            combined.push_str(&format!(
                                "## 步骤 {}/{}\n{}\n\n",
                                idx + 1,
                                total_steps,
                                response
                            ));
                        }
                        Some(event) => {
                            let payload = serde_json::to_value(&event).unwrap_or_default();
                            let _ = app.emit("agent-event", payload);
                        }
                        None => break,
                    }
                }
                _ = cancel_notify.notified() => {
                    let _ = app.emit(
                        "agent-event",
                        serde_json::json!({
                            "type": "cancelled",
                            "message": "Generation cancelled by user."
                        }),
                    );
                    agent_handle.abort();
                    spawn_track(
                        "stitch_suite_done",
                        serde_json::json!({
                            "task_suite_id": suite.id,
                            "outcome": "cancelled",
                        }),
                    );
                    return Ok(());
                }
            }
        }

        match agent_handle.await {
            Ok((Ok(result), _)) => {
                if combined.is_empty() || !combined.contains(&result.response) {
                    combined.push_str(&format!(
                        "## 步骤 {}/{}\n{}\n\n",
                        idx + 1,
                        total_steps,
                        result.response
                    ));
                }
                let _ = app.emit(
                    "agent-event",
                    serde_json::to_value(AgentEvent::PlanStepDone {
                        index: idx,
                        description: step.step_title.clone(),
                    })
                    .unwrap_or_default(),
                );
            }
            Ok((Err(e), _)) => {
                let titles: Vec<(usize, &str)> = suite
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i, s.step_title.as_str()))
                    .collect();
                let summary = format_suite_failure_summary(
                    &suite.title,
                    idx,
                    total_steps,
                    &step.step_title,
                    &format!("{e:#}"),
                    &combined,
                    &titles,
                );
                let _ = app.emit(
                    "agent-event",
                    serde_json::json!({
                        "type": "error",
                        "message": summary,
                    }),
                );
                spawn_track(
                    "stitch_suite_done",
                    serde_json::json!({
                        "task_suite_id": suite.id,
                        "outcome": "failed",
                    }),
                );
                return Ok(());
            }
            Err(e) if e.is_cancelled() => {
                spawn_track(
                    "stitch_suite_done",
                    serde_json::json!({
                        "task_suite_id": suite.id,
                        "outcome": "cancelled",
                    }),
                );
                return Ok(());
            }
            Err(e) => {
                let titles: Vec<(usize, &str)> = suite
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i, s.step_title.as_str()))
                    .collect();
                let summary = format_suite_failure_summary(
                    &suite.title,
                    idx,
                    total_steps,
                    &step.step_title,
                    &format!("Agent task panicked: {e}"),
                    &combined,
                    &titles,
                );
                let _ = app.emit(
                    "agent-event",
                    serde_json::json!({
                        "type": "error",
                        "message": summary,
                    }),
                );
                spawn_track(
                    "stitch_suite_done",
                    serde_json::json!({
                        "task_suite_id": suite.id,
                        "outcome": "failed",
                    }),
                );
                return Ok(());
            }
        }
    }

    if cancel_flag.load(Ordering::SeqCst) {
        suite_outcome = "cancelled";
    }
    spawn_track(
        "stitch_suite_done",
        serde_json::json!({
            "task_suite_id": suite.id,
            "outcome": suite_outcome,
        }),
    );

    if !cancel_flag.load(Ordering::SeqCst) {
        let _ = app.emit(
            "agent-event",
            serde_json::json!({
                "type": "done",
                "response": if combined.is_empty() {
                    format!("套件「{}」执行完成。", suite.title)
                } else {
                    combined
                },
                "iterations": total_steps,
            }),
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn run_agent(
    app: tauri::AppHandle,
    state: tauri::State<'_, CancelState>,
    confirm_state: tauri::State<'_, ConfirmState>,
    work_dir_state: tauri::State<'_, WorkDirState>,
    id: String,
    profile_id: Option<String>,
    model: Option<String>,
) -> Result<(), String> {
    let _busy = try_acquire_generation(&state)?;
    let cfg = StitchConfig::load().map_err(|e| e.to_string())?;
    let client = mcp_client(&cfg)?;
    let plan = client
        .run_agent_by_name(&id, None)
        .await
        .map_err(|e| e.to_string())?;

    let name = plan
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&id)
        .to_string();
    spawn_track(
        "stitch_agent_run",
        serde_json::json!({ "source": name, "from": "agent" }),
    );
    let steps = plan
        .get("steps")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if steps.is_empty() {
        return Err("智能体没有可执行的步骤".into());
    }

    let orch_rules_str = plan
        .get("orchestration_rules")
        .and_then(|v| v.as_array())
        .map(|rules| {
            rules
                .iter()
                .filter_map(|r| r.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let completion_instruction = plan
        .get("completion")
        .and_then(|v| v.get("instruction"))
        .and_then(|v| v.as_str())
        .unwrap_or("执行完成后报告结果。")
        .to_string();

    let resolved = cfg
        .resolve_llm(
            profile_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            model.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        )
        .map_err(|e| e.to_string())?;
    let api_key = resolved.api_key;
    let model = resolved.model;
    let api_base = resolved.api_base;
    let max_iterations = cfg.max_iterations;
    let work_dir = work_dir_state
        .path
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let tools = build_agent_registry(&work_dir, &cfg).await;
    let system_prompt = format!(
        "{}\n\n## 编排规则\n{orch_rules_str}\n\n## 完成指引\n{completion_instruction}",
        agent::prompt::build_system_prompt(&work_dir, &tools),
    );
    let mut session = Session::new(system_prompt);

    let mut task_description = format!("执行智能体「{name}」的 {} 个步骤：\n\n", steps.len());
    for step in &steps {
        let pos = step.get("position").and_then(|v| v.as_i64()).unwrap_or(0);
        let title = step
            .get("step_title")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let content = step.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let preview = step
            .get("content_preview")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        task_description.push_str(&format!(
            "## 步骤 {pos}: {title}\n\n{}\n\n",
            if content.is_empty() { preview } else { content }
        ));
    }
    session.add_user_message(&task_description);

    state.flag.store(false, Ordering::SeqCst);
    let cancel_flag = state.flag.clone();
    let cancel_notify = state.notify.clone();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
    let agent_flag = cancel_flag.clone();
    let confirm_pending = confirm_state.pending.clone();
    let rules_s = confirm_state.rules.clone();
    let work_dir_s = work_dir.clone();
    let agent_handle = tokio::spawn(async move {
        let result = agent::run_react_streaming(
            &mut session,
            &api_base,
            &model,
            &api_key,
            &tools,
            max_iterations,
            confirm_pending,
            Some(work_dir_s.as_str()),
            rules_s,
            &event_tx,
            &agent_flag,
            None,
        )
        .await;
        (result, session)
    });

    let _ = pump_agent_events(
        &app,
        &mut event_rx,
        &cancel_notify,
        agent_handle,
        &cancel_flag,
    )
    .await?;
    Ok(())
}

// ── Title Bar Theme ───────────────────────────────────────────────────

#[tauri::command]
pub fn set_titlebar_theme(app: tauri::AppHandle, dark: bool) -> Result<(), String> {
    platform::set_theme(&app, dark);
    Ok(())
}

// ── Taskbar Progress ──────────────────────────────────────────────────

#[tauri::command]
pub fn clear_taskbar_progress(app: tauri::AppHandle) -> Result<(), String> {
    platform::clear(&app);
    Ok(())
}

// ── Startup State ─────────────────────────────────────────────────

#[derive(Clone)]
pub struct StartupState {
    pub finished: Arc<AtomicBool>,
    pub window_shown: Arc<AtomicBool>,
}

#[tauri::command]
pub fn finish_startup(
    app: tauri::AppHandle,
    state: tauri::State<StartupState>,
    dark: bool,
) -> Result<(), String> {
    state.finished.store(true, Ordering::SeqCst);
    platform::finish_splash_and_show(&app, dark, &state.window_shown)
}

// ── Auto-updater commands ──────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct UpdateStatus {
    pub available: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub release_notes: Option<String>,
    pub download_url: Option<String>,
}

#[tauri::command]
pub async fn check_update(app: tauri::AppHandle) -> Result<UpdateStatus, String> {
    let current = app.package_info().version.to_string();

    let updater = app
        .updater()
        .map_err(|e| format!("更新器初始化失败: {e}"))?;

    match updater.check().await {
        Ok(Some(update)) => {
            let latest = update.version.clone();
            let notes = update.body.clone();
            let status = UpdateStatus {
                available: current != latest,
                current_version: current,
                latest_version: Some(latest),
                release_notes: notes,
                download_url: None,
            };
            Ok(status)
        }
        Ok(None) => Ok(UpdateStatus {
            available: false,
            current_version: current,
            latest_version: None,
            release_notes: None,
            download_url: None,
        }),
        Err(e) => {
            tracing::warn!("Update check failed: {e}");
            let msg = e.to_string();
            let friendly = if msg.contains("pubkey") || msg.contains("public key") {
                "更新尚未配置签名公钥，暂无法在线升级".to_string()
            } else if msg.contains("dns") || msg.contains("connect") || msg.contains("timed out") {
                "无法连接更新服务，请稍后重试".to_string()
            } else {
                format!("检查更新失败: {msg}")
            };
            Err(friendly)
        }
    }
}

#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app
        .updater()
        .map_err(|e| format!("更新器初始化失败: {e}"))?;

    match updater.check().await {
        Ok(Some(update)) => {
            update
                .download_and_install(|_downloaded, _total| {}, || {})
                .await
                .map_err(|e| format!("更新安装失败: {e}"))?;
            Ok(())
        }
        Ok(None) => Err("没有可用的更新".into()),
        Err(e) => Err(format!("检查更新失败: {e}")),
    }
}

/// Mirror frontend diagnostics into Rust tracing (visible in terminal / log file).
#[tauri::command]
pub fn frontend_log(level: String, message: String) {
    match level.as_str() {
        "error" => tracing::error!(target: "stitch_ui", "{message}"),
        "warn" => tracing::warn!(target: "stitch_ui", "{message}"),
        _ => tracing::info!(target: "stitch_ui", "{message}"),
    }
}

// ── Window geometry persistence ───────────────────────────────────
//
// 落盘路径：stitch 配置目录下 `window-state.json`（与 config.toml 同级）。
// compact 浮条是独立模式：启用时强制小窗，禁用时应恢复到「进入 compact 前」
// 的用户窗口几何，因此 compact 期间不记录、离开前快照一次。

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowStateSnapshot {
    pub width: u32,
    pub height: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

static COMPACT_PRE_GEOMETRY: Mutex<Option<WindowStateSnapshot>> = Mutex::new(None);

fn window_state_path() -> PathBuf {
    let dir = stitch::config::config_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    dir.join("window-state.json")
}

fn load_window_state_file() -> Option<WindowStateSnapshot> {
    let content = std::fs::read_to_string(window_state_path()).ok()?;
    serde_json::from_str(&content).ok()
}

fn capture_window_geometry(
    window: &tauri::WebviewWindow,
    maximized: bool,
) -> Option<WindowStateSnapshot> {
    let size = window.outer_size().ok()?;
    let pos = window.outer_position().ok()?;
    Some(WindowStateSnapshot {
        width: size.width,
        height: size.height,
        x: Some(pos.x),
        y: Some(pos.y),
        maximized,
    })
}

fn apply_window_geometry(window: &tauri::WebviewWindow, snap: &WindowStateSnapshot) {
    if snap.maximized {
        let _ = window.maximize();
        return;
    }
    if snap.width > 0 && snap.height > 0 {
        let _ = window.set_size(tauri::PhysicalSize::new(snap.width, snap.height));
    }
    // 位置单独应用：跨显示器 / 屏幕掉线时 set_position 可能失败，忽略失败保留居中。
    if let (Some(x), Some(y)) = (snap.x, snap.y) {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

pub(crate) fn apply_startup_window_state(app: &tauri::AppHandle) {
    let Some(snap) = load_window_state_file() else {
        return;
    };
    if let Some(window) = app.get_webview_window("main") {
        apply_window_geometry(&window, &snap);
    }
}

#[tauri::command]
pub async fn save_window_state(app: tauri::AppHandle, maximized: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("main window not found")?;
    if window.is_minimized().map_err(|e| e.to_string())? {
        return Ok(());
    }
    // compact 期间的几何是模式强制值，不是用户布局——不覆盖落盘。
    if COMPACT_PRE_GEOMETRY
        .lock()
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let Some(snap) = capture_window_geometry(&window, maximized) else {
        return Ok(());
    };
    let content = serde_json::to_string(&snap).map_err(|e| e.to_string())?;
    std::fs::write(window_state_path(), content).map_err(|e| e.to_string())
}

/// One interpolated frame of the compact morph animation (physical px).
struct CompactStep {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Compact bar geometry constant (physical px), mirrored by the frontend bar.
const COMPACT_W: f64 = 420.0;
const COMPACT_H: f64 = 64.0;
const COMPACT_MARGIN: f64 = 16.0;

#[allow(dead_code)] // 保留：窗口层变形动画参考实现（切换已改瞬间+前端过渡）
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// Ease-out-back: overshoots the target slightly, then settles back — the
/// lively「精灵落位」bounce for the morph's position.
#[allow(dead_code)] // 保留：窗口层变形动画参考实现（切换已改瞬间+前端过渡）
fn ease_out_back(t: f64) -> f64 {
    const C1: f64 = 0.6;
    const C3: f64 = C1 + 1.0;
    let u = t - 1.0;
    1.0 + C3 * u * u * u + C1 * u * u
}

/// Interpolated morph frames from `from` to `to` (6 steps).
/// Size always eases out monotonically; position bounces (`lively`) on the
/// enter morph and eases out plainly on restore.
/// Pure — unit-tested separately from the window calls.
#[allow(dead_code)] // 保留：窗口层变形动画参考实现（切换已改瞬间+前端过渡）
fn compact_animation_steps(
    from: (f64, f64, f64, f64),
    to: (f64, f64, f64, f64),
    lively: bool,
) -> Vec<CompactStep> {
    const STEPS: usize = 12;
    (1..=STEPS)
        .map(|i| {
            let t = i as f64 / STEPS as f64;
            let pos_eased = if lively {
                ease_out_back(t)
            } else {
                ease_out_cubic(t)
            };
            let size_eased = ease_out_cubic(t);
            CompactStep {
                x: from.0 + (to.0 - from.0) * pos_eased,
                y: from.1 + (to.1 - from.1) * pos_eased,
                width: from.2 + (to.2 - from.2) * size_eased,
                height: from.3 + (to.3 - from.3) * size_eased,
            }
        })
        .collect()
}

/// Morph duration: production default 600ms; set STITCH_COMPACT_ANIM_MS to
/// stretch it for visual observation (clamped 200ms–8s).
#[allow(dead_code)] // 保留：窗口层变形动画参考实现（切换已改瞬间+前端过渡）
fn compact_anim_duration_ms() -> u64 {
    std::env::var("STITCH_COMPACT_ANIM_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(|v| v.clamp(200, 8000))
        .unwrap_or(600)
}

#[allow(dead_code)] // 保留：窗口层变形动画参考实现（切换已改瞬间+前端过渡）
fn compact_anim_step_sleep() -> std::time::Duration {
    std::time::Duration::from_millis((compact_anim_duration_ms() / 6).max(1))
}

/// Bottom-right corner of the monitor the cursor is on (fallback: primary),
/// flush with a small margin — the bar morphs toward where the user works.
fn compact_target_corner(app: &tauri::AppHandle) -> Option<(f64, f64)> {
    let cursor = app.cursor_position().ok();
    let monitor = cursor
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let size = monitor.size();
    Some((
        (size.width as f64 - COMPACT_W - COMPACT_MARGIN).max(0.0),
        (size.height as f64 - COMPACT_H - COMPACT_MARGIN).max(0.0),
    ))
}

/// Apply a frame of the morph animation (position + size together so the
/// window visibly "collapses into" the corner).
fn apply_compact_step(window: &tauri::WebviewWindow, step: &CompactStep) {
    let _ = window.set_position(tauri::PhysicalPosition::new(step.x as i32, step.y as i32));
    let _ = window.set_size(tauri::PhysicalSize::new(
        (step.width.max(1.0)) as u32,
        (step.height.max(1.0)) as u32,
    ));
}

#[tauri::command]
pub fn set_compact_mode(app: tauri::AppHandle, compact: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("main window not found")?;
    if compact {
        // 快照用户几何，供退出 compact 时还原。
        let pre = if window.is_maximized().unwrap_or(false) {
            WindowStateSnapshot {
                maximized: true,
                ..Default::default()
            }
        } else {
            capture_window_geometry(&window, false).unwrap_or_default()
        };
        if let Ok(mut slot) = COMPACT_PRE_GEOMETRY.lock() {
            *slot = Some(pre);
        }
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
        window.set_resizable(false).map_err(|e| e.to_string())?;
        window.set_skip_taskbar(true).map_err(|e| e.to_string())?;
        // 去掉系统标题栏——420×64 的浮条不该被装饰吃掉；透明窗口下前端圆角即浮条外形。
        let _ = window.set_decorations(false);
        // 最大化窗口先还原，否则 Windows 上 set_size 不生效。
        let _ = window.unmaximize();

        // 一次切换到位（不再逐步 resize——窗口层步进在 Windows 上跳变/闪烁；
        // 转换观感由前端过渡动画承担，GPU 合成更流畅稳定）
        let (tx, ty) = compact_target_corner(&app).unwrap_or((0.0, 0.0));
        apply_compact_step(
            &window,
            &CompactStep {
                x: tx,
                y: ty,
                width: COMPACT_W,
                height: COMPACT_H,
            },
        );
        tracing::info!("Compact mode enabled (420x64 overlay, instant)");
    } else {
        let pre = COMPACT_PRE_GEOMETRY
            .lock()
            .map_err(|e| e.to_string())?
            .take();
        window.set_always_on_top(false).map_err(|e| e.to_string())?;
        window.set_resizable(true).map_err(|e| e.to_string())?;
        window.set_skip_taskbar(false).map_err(|e| e.to_string())?;
        let _ = window.set_decorations(true);
        match pre {
            Some(snap) if snap.maximized => {
                let _ = window.set_size(tauri::LogicalSize::new(1120.0, 740.0));
                let _ = window.maximize();
            }
            Some(snap) => {
                // 一次还原到位（与进入对称——转换动画由前端过渡承担）
                apply_window_geometry(&window, &snap);
            }
            None => {
                window
                    .set_size(tauri::LogicalSize::new(1120.0, 740.0))
                    .map_err(|e| e.to_string())?;
                window.center().map_err(|e| e.to_string())?;
            }
        }
        tracing::info!("Compact mode disabled (geometry restored)");
    }
    Ok(())
}

#[cfg(test)]
mod llm_test_error_tests {
    use super::friendly_llm_test_error;

    #[test]
    fn auth_json_becomes_friendly_key_message() {
        let body = r#"{"error":{"message":"Authentication Fails, Your api key: aaaa is invalid","type":"authentication_error"}}"#;
        assert_eq!(
            friendly_llm_test_error(401, body),
            "API Key 无效，请检查后重试"
        );
    }

    #[test]
    fn long_body_is_truncated() {
        let body = "x".repeat(200);
        assert_eq!(
            friendly_llm_test_error(500, &body),
            "模型连接失败（HTTP 500）"
        );
    }
}

/// 浮条拖拽停止后吸附屏幕边缘（仅 compact 模式由前端调用）。
/// 水平贴最近侧（左/右，margin 8px），垂直保持用户拖拽位置。
#[tauri::command]
pub fn snap_compact_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("main window not found")?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let Some(mon) = window.current_monitor().map_err(|e| e.to_string())? else {
        return Ok(());
    };
    let mb = mon.size();
    let mx = mon.position().x;
    let center = pos.x as f64 + size.width as f64 / 2.0;
    let m_center = mx as f64 + mb.width as f64 / 2.0;
    const MARGIN: f64 = 8.0;
    let target_x = if center < m_center {
        mx as f64 + MARGIN
    } else {
        mx as f64 + mb.width as f64 - size.width as f64 - MARGIN
    };
    let _ = window.set_position(tauri::PhysicalPosition::new(target_x as i32, pos.y));
    Ok(())
}

#[cfg(test)]
mod compact_animation_tests {
    use super::compact_animation_steps;

    #[test]
    fn steps_monotonic_ease_out_and_end_exact() {
        let steps = compact_animation_steps(
            (0.0, 0.0, 1120.0, 740.0),
            (400.0, 500.0, 420.0, 64.0),
            false,
        );
        assert_eq!(steps.len(), 12); // STEPS 常量（窗口变形动画已弃用但逻辑保留）
        let sizes: Vec<f64> = steps.iter().map(|s| s.width).collect();
        let positions: Vec<f64> = steps.iter().map(|s| s.x).collect();
        // 单调逼近目标：尺寸递减、x 递增，终点精确命中。
        assert!(sizes.windows(2).all(|w| w[0] > w[1]));
        assert!(positions.windows(2).all(|w| w[0] < w[1]));
        let last = steps.last().unwrap();
        assert_eq!(
            (last.x, last.y, last.width, last.height),
            (400.0, 500.0, 420.0, 64.0)
        );
        // ease-out：首步步长最大、末步步长最小。
        let deltas: Vec<f64> = sizes.windows(2).map(|w| w[0] - w[1]).collect();
        assert!(deltas[0] > deltas[1]);
        assert!(deltas[1] > deltas[2]);
        assert!(deltas[2] > deltas[3]);
        assert!(deltas[3] > deltas[4]);
    }

    #[test]
    fn lively_morph_overshoots_position_then_settles_exact() {
        let steps =
            compact_animation_steps((0.0, 0.0, 1120.0, 740.0), (400.0, 500.0, 420.0, 64.0), true);
        assert_eq!(steps.len(), 12); // STEPS 常量
        let positions: Vec<f64> = steps.iter().map(|s| s.x).collect();
        // 回弹：中间某步越过目标（>400），最终精确落回。
        let max_pos = positions.iter().cloned().fold(0.0f64, f64::max);
        assert!(max_pos > 400.0, "expected overshoot, got {max_pos}");
        let last = steps.last().unwrap();
        assert_eq!(
            (last.x, last.y, last.width, last.height),
            (400.0, 500.0, 420.0, 64.0)
        );
        // 尺寸仍单调（不闪跳）。
        let sizes: Vec<f64> = steps.iter().map(|s| s.width).collect();
        assert!(sizes.windows(2).all(|w| w[0] > w[1]));
    }

    #[test]
    fn steps_noop_when_from_equals_to() {
        let steps = compact_animation_steps((1.0, 2.0, 3.0, 4.0), (1.0, 2.0, 3.0, 4.0), true);
        assert_eq!(steps.len(), 12); // STEPS 常量
        for s in &steps {
            assert_eq!((s.x, s.y, s.width, s.height), (1.0, 2.0, 3.0, 4.0));
        }
    }
}

#[cfg(test)]
mod suite_summary_tests {
    use super::format_suite_failure_summary;

    #[test]
    fn summary_includes_completed_failed_and_skipped() {
        let titles = [(0, "收集"), (1, "改写"), (2, "复核")];
        let text = format_suite_failure_summary(
            "演示套件",
            1,
            3,
            "改写",
            "模型超时",
            "## 步骤 1/3\n已收集 2 条\n\n",
            &titles,
        );
        assert!(text.contains("未全部完成：第 2/3 步失败（改写）"));
        assert!(text.contains("## 已完成步骤"));
        assert!(text.contains("已收集 2 条"));
        assert!(text.contains("## 失败步骤"));
        assert!(text.contains("原因：模型超时"));
        assert!(text.contains("## 未执行步骤"));
        assert!(text.contains("步骤 3/3 · 复核"));
        assert!(!text.contains("步骤 1/3 · 收集\n"));
    }

    #[test]
    fn summary_first_step_failure_omits_completed_section() {
        let titles = [(0, "起步"), (1, "收尾")];
        let text = format_suite_failure_summary("短套件", 0, 2, "起步", "无 API Key", "", &titles);
        assert!(text.contains("第 1/2 步失败"));
        assert!(!text.contains("## 已完成步骤"));
        assert!(text.contains("未执行步骤"));
        assert!(text.contains("步骤 2/2 · 收尾"));
    }
}

#[cfg(test)]
mod local_skills_tests {
    use super::discover_local_skills;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_skill(dir: &std::path::Path, slug: &str, name: &str) {
        let root = dir.join(slug);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test\n---\n\n# {name}\n"),
        )
        .unwrap();
    }

    fn temp_root(tag: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("stitch-skills-{tag}-{n}"));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn merges_user_skills_and_workspace_wins_on_slug() {
        let work = temp_root("work");
        let home = temp_root("home");
        write_skill(
            &work.join(".agents").join("skills"),
            "shared",
            "Workspace Shared",
        );
        write_skill(
            &work.join(".agents").join("skills"),
            "only-work",
            "Only Work",
        );
        write_skill(
            &home.join(".agents").join("skills"),
            "shared",
            "User Shared",
        );
        write_skill(
            &home.join(".agents").join("skills"),
            "only-user",
            "Only User",
        );

        let rows = discover_local_skills(Some(&work), Some(&home));
        let by_slug: std::collections::HashMap<_, _> =
            rows.iter().map(|r| (r.slug.as_str(), r)).collect();

        assert_eq!(by_slug["shared"].scope, "workspace");
        assert_eq!(by_slug["shared"].title, "Workspace Shared");
        assert_eq!(by_slug["only-work"].scope, "workspace");
        assert_eq!(by_slug["only-user"].scope, "user");
        assert!(
            by_slug["only-user"]
                .rel_path
                .starts_with("~/.agents/skills/")
        );

        let _ = fs::remove_dir_all(&work);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn user_only_when_work_dir_empty() {
        let home = temp_root("home-only");
        write_skill(
            &home.join(".cursor").join("skills"),
            "home-skill",
            "Home Skill",
        );
        let rows = discover_local_skills(None, Some(&home));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].slug, "home-skill");
        assert_eq!(rows[0].scope, "user");
        let _ = fs::remove_dir_all(&home);
    }
}
