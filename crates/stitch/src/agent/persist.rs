//! Session Autopilot persistence (ADR-036).
//!
//! Authoritative Agent history lives under:
//! `{work_dir}/.stitch/sessions/{session_id}/`
//! — `messages.jsonl`, `manifest.json`, `checkpoints/epoch-N.json`.
//!
//! UI localStorage is a projection only; never the source of truth for tool_calls.

use crate::agent::tokens;
use crate::session::{Message, Session};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const MANIFEST_VERSION: u32 = 1;
const CHECKPOINT_VERSION: u32 = 2;
/// Tool message bodies longer than this are written under `outputs/` on disk.
pub const TOOL_OUTPUT_INLINE_MAX: usize = 8_192;
const EXTERNAL_PREFIX: &str = "[external:outputs/";
/// ADR-036: retain the newest N checkpoint generations; older files are GC'd.
pub const KEEP_CHECKPOINT_EPOCHS: usize = 3;

/// One jsonl line: RFC3339 timestamp + OpenAI-shaped message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonlRecord {
    pub ts: String,
    #[serde(flatten)]
    pub msg: Message,
}

/// Per-session manifest (committed epoch pointer).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub session_id: String,
    pub work_dir: String,
    pub committed_epoch: u32,
    pub msg_count: usize,
    pub estimated_tokens: usize,
    pub created_at: String,
    pub updated_at: String,
    /// Set when restore fell back to text-only UI history (diagnostic).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_degraded: Option<String>,
}

impl Manifest {
    pub fn new(session_id: &str, work_dir: &Path) -> Self {
        let now = now_rfc3339();
        Self {
            version: MANIFEST_VERSION,
            session_id: session_id.to_string(),
            work_dir: work_dir.to_string_lossy().into_owned(),
            committed_epoch: 0,
            msg_count: 0,
            estimated_tokens: 0,
            created_at: now.clone(),
            updated_at: now,
            restore_degraded: None,
        }
    }
}

const LAYERS_VERSION: u32 = 1;

/// Warm/cold layering archive sidecar (`layers.json`). Epoch-stamped so a
/// rollback or a crash before the manifest advanced drops stale entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayersFile {
    pub version: u32,
    #[serde(default)]
    pub warm: Vec<LayerRecord<crate::agent::layers::CompressedTurn>>,
    #[serde(default)]
    pub cold: Vec<LayerRecord<crate::agent::layers::ColdEntry>>,
}

/// One archived entry with the session epoch it was written under.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerRecord<T> {
    pub epoch: u32,
    pub entry: T,
}

/// Structured compact checkpoint (hard / partial).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub version: u32,
    pub session_id: String,
    pub epoch: u32,
    pub parent_epoch: u32,
    /// `"partial"` (soft/~70%) or `"full"` (hard/~85%).
    pub compression_level: String,
    pub msg_range: [usize; 2],
    pub summary_natural: String,
    pub goals: Vec<String>,
    pub decisions: Vec<CheckpointDecision>,
    pub open_items: Vec<String>,
    pub artifacts: Vec<CheckpointArtifact>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointDecision {
    pub what: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointArtifact {
    pub local_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub relative_path: String,
}

/// Sanitize session id for a single path segment.
pub fn sanitize_session_id(id: &str) -> Option<String> {
    let t = id.trim();
    if t.is_empty() || t.len() > 128 {
        return None;
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(t.to_string())
}

/// `{work_dir}/.stitch/sessions/{session_id}`
pub fn session_dir(work_dir: &Path, session_id: &str) -> Option<PathBuf> {
    let id = sanitize_session_id(session_id)?;
    Some(work_dir.join(".stitch").join("sessions").join(id))
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn ensure_dir(dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    Ok(())
}

fn messages_path(dir: &Path) -> PathBuf {
    dir.join("messages.jsonl")
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("manifest.json")
}

fn checkpoints_dir(dir: &Path) -> PathBuf {
    dir.join("checkpoints")
}

fn checkpoint_path(dir: &Path, epoch: u32) -> PathBuf {
    checkpoints_dir(dir).join(format!("epoch-{epoch}.json"))
}

/// Load authoritative session from disk. Returns `None` if missing.
pub fn load_session(dir: &Path) -> anyhow::Result<Option<(Session, Manifest)>> {
    let man_path = manifest_path(dir);
    let msg_path = messages_path(dir);
    if !man_path.is_file() {
        return Ok(None);
    }
    if !msg_path.is_file() {
        tracing::warn!(path = %dir.display(), "messages.jsonl missing; checkpoint fallback");
        return load_from_checkpoint_fallback(dir);
    }
    let man_raw = fs::read_to_string(&man_path)?;
    let manifest: Manifest = match serde_json::from_str(&man_raw) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(%e, path = %man_path.display(), "corrupt manifest; trying checkpoint fallback");
            return load_from_checkpoint_fallback(dir);
        }
    };
    let messages = match read_jsonl(&msg_path) {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => {
            // File exists but yielded no valid records → treat as corrupt.
            tracing::warn!(
                path = %msg_path.display(),
                "messages.jsonl empty or unreadable; checkpoint fallback"
            );
            return load_from_checkpoint_fallback(dir);
        }
        Err(e) => {
            tracing::warn!(%e, path = %msg_path.display(), "corrupt messages.jsonl; checkpoint fallback");
            return load_from_checkpoint_fallback(dir);
        }
    };
    let mut session = Session {
        messages,
        iteration: 0,
        tokens_used: manifest.estimated_tokens,
        epoch: manifest.committed_epoch,
        layers: Some(read_layers(dir, manifest.committed_epoch)),
    };
    hydrate_external_outputs(dir, &mut session);
    crate::agent::context::repair_message_sequence(&mut session.messages);
    Ok(Some((session, manifest)))
}

fn load_from_checkpoint_fallback(dir: &Path) -> anyhow::Result<Option<(Session, Manifest)>> {
    let Some(cp) = load_latest_readable_checkpoint(dir)? else {
        return Ok(None);
    };
    // Minimal resume: system stub + checkpoint as user summary (no full tool chain).
    let mut session = Session::new(
        "You are a coding agent. Earlier conversation was restored from a checkpoint after storage damage.",
    );
    session.epoch = cp.epoch;
    session.add_user_message(format!(
        "[Restored checkpoint epoch {} — prior tool chain unavailable]\n\n{}",
        cp.epoch,
        format_checkpoint_for_resume(&cp)
    ));
    let mut manifest = Manifest::new(&cp.session_id, Path::new(""));
    manifest.committed_epoch = cp.epoch;
    manifest.msg_count = session.messages.len();
    manifest.estimated_tokens = tokens::estimate_messages(&session.messages);
    manifest.restore_degraded = Some("checkpoint_fallback".into());
    Ok(Some((session, manifest)))
}

pub fn format_checkpoint_for_resume(cp: &Checkpoint) -> String {
    let mut parts = Vec::new();
    if !cp.summary_natural.trim().is_empty() {
        parts.push(cp.summary_natural.trim().to_string());
    }
    if !cp.goals.is_empty() {
        parts.push(format!("Goals:\n- {}", cp.goals.join("\n- ")));
    }
    if !cp.decisions.is_empty() {
        let lines: Vec<String> = cp
            .decisions
            .iter()
            .map(|d| {
                if d.why.is_empty() {
                    d.what.clone()
                } else {
                    format!("{} ({})", d.what, d.why)
                }
            })
            .collect();
        parts.push(format!("Decisions:\n- {}", lines.join("\n- ")));
    }
    if !cp.open_items.is_empty() {
        parts.push(format!("Open:\n- {}", cp.open_items.join("\n- ")));
    }
    if !cp.artifacts.is_empty() {
        let lines: Vec<String> = cp
            .artifacts
            .iter()
            .map(|a| format!("{} ({})", a.title, a.local_id))
            .collect();
        parts.push(format!("Artifacts:\n- {}", lines.join("\n- ")));
    }
    parts.join("\n\n")
}

fn load_latest_readable_checkpoint(dir: &Path) -> anyhow::Result<Option<Checkpoint>> {
    let cp_dir = checkpoints_dir(dir);
    if !cp_dir.is_dir() {
        return Ok(None);
    }
    let mut epochs: Vec<u32> = Vec::new();
    for ent in fs::read_dir(&cp_dir)? {
        let ent = ent?;
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if let Some(rest) = name.strip_prefix("epoch-")
            && let Some(num) = rest.strip_suffix(".json")
            && let Ok(n) = num.parse::<u32>()
        {
            epochs.push(n);
        }
    }
    epochs.sort_unstable();
    for epoch in epochs.into_iter().rev() {
        let path = checkpoint_path(dir, epoch);
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(cp) = serde_json::from_str::<Checkpoint>(&raw)
        {
            return Ok(Some(cp));
        }
    }
    Ok(None)
}

fn read_jsonl(path: &Path) -> anyhow::Result<Vec<Message>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<JsonlRecord>(line) {
            Ok(rec) => out.push(rec.msg),
            Err(e) => {
                // Tolerate a bad line by skipping; if too many fail, error.
                tracing::warn!(line = i + 1, %e, "skip bad jsonl line");
            }
        }
    }
    Ok(out)
}

/// Rewrite authoritative messages + update manifest (turn-end / compact sync).
///
/// Oversized tool bodies are externalized under `outputs/` in the **on-disk**
/// copy only; the in-memory `session` is left intact for the live agent.
pub fn save_session(dir: &Path, session: &Session, manifest: &mut Manifest) -> anyhow::Result<()> {
    ensure_dir(dir)?;
    // Layers first: if we crash before the manifest advances, the epoch filter
    // on load drops the newer entries instead of leaving them dangling.
    if let Err(e) = write_layers(dir, session) {
        tracing::warn!(%e, "layers.json write failed; messages still authoritative");
    }
    let mut disk_msgs = session.messages.clone();
    let _ = externalize_tool_outputs(dir, &mut disk_msgs)?;
    // Lightweight backend: image data URLs never persist — strip them from
    // the disk copy only (the in-memory session keeps them for this turn).
    for m in &mut disk_msgs {
        m.content.strip_images();
    }
    let tmp = dir.join("messages.jsonl.tmp");
    let final_path = messages_path(dir);
    {
        let mut f = File::create(&tmp)?;
        let ts = now_rfc3339();
        for msg in &disk_msgs {
            let rec = JsonlRecord {
                ts: ts.clone(),
                msg: msg.clone(),
            };
            serde_json::to_writer(&mut f, &rec)?;
            f.write_all(b"\n")?;
        }
        f.sync_all()?;
    }
    fs::rename(&tmp, &final_path)?;

    manifest.msg_count = session.messages.len();
    // Estimate from live session (full tool bodies) for UI / thresholds.
    manifest.estimated_tokens = tokens::estimate_messages(&session.messages);
    manifest.committed_epoch = session.epoch;
    manifest.updated_at = now_rfc3339();
    write_manifest(dir, manifest)?;
    if let Err(e) = prune_orphan_outputs(dir) {
        tracing::warn!(%e, "orphan outputs prune after save failed");
    }
    Ok(())
}

fn layers_path(dir: &Path) -> PathBuf {
    dir.join("layers.json")
}

/// Atomic write of the warm/cold archive. Entries are stamped with the
/// session epoch at write time (per-entry creation epochs are not kept in
/// memory; `TurnFlusher::rollback` restores this file via `before.layers`).
fn write_layers(dir: &Path, session: &Session) -> anyhow::Result<()> {
    let Some(lm) = session.layers.as_ref() else {
        return Ok(());
    };
    let file = LayersFile {
        version: LAYERS_VERSION,
        warm: lm
            .warm
            .iter()
            .map(|e| LayerRecord {
                epoch: session.epoch,
                entry: e.clone(),
            })
            .collect(),
        cold: lm
            .cold
            .iter()
            .map(|e| LayerRecord {
                epoch: session.epoch,
                entry: e.clone(),
            })
            .collect(),
    };
    let tmp = dir.join("layers.json.tmp");
    {
        let mut f = File::create(&tmp)?;
        serde_json::to_writer(&mut f, &file)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, layers_path(dir))?;
    Ok(())
}

/// Load the archive, dropping entries stamped after the committed epoch
/// (crash / rollback leftovers). Missing or corrupt file degrades to a fresh
/// layer manager without touching the authoritative messages.
fn read_layers(dir: &Path, committed_epoch: u32) -> crate::agent::layers::LayerManager {
    let raw = match fs::read_to_string(layers_path(dir)) {
        Ok(s) => s,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(%e, path = %layers_path(dir).display(), "layers.json unreadable");
            }
            return crate::agent::layers::LayerManager::default();
        }
    };
    let file: LayersFile = match serde_json::from_str(&raw) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(%e, path = %layers_path(dir).display(), "layers.json corrupt");
            return crate::agent::layers::LayerManager::default();
        }
    };
    crate::agent::layers::LayerManager {
        warm: file
            .warm
            .into_iter()
            .filter(|r| r.epoch <= committed_epoch)
            .map(|r| r.entry)
            .collect(),
        cold: file
            .cold
            .into_iter()
            .filter(|r| r.epoch <= committed_epoch)
            .map(|r| r.entry)
            .collect(),
        config: crate::agent::layers::LayerConfig::default(),
    }
}

fn outputs_dir(dir: &Path) -> PathBuf {
    dir.join("outputs")
}

fn sanitize_output_stem(tool_call_id: &str) -> String {
    let mut s: String = tool_call_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s = "tool".into();
    }
    if s.len() > 96 {
        s.truncate(96);
    }
    s
}

/// Parse `[external:outputs/NAME.txt]` marker; returns file name only.
pub fn parse_external_output_ref(content: &str) -> Option<&str> {
    let rest = content.strip_prefix(EXTERNAL_PREFIX)?;
    let end = rest.find(']')?;
    let name = &rest[..end];
    if name.is_empty() || name.contains("..") || name.contains('/') || name.contains('\\') {
        return None;
    }
    if !name.ends_with(".txt") {
        return None;
    }
    Some(name)
}

/// Write oversized tool message bodies to `outputs/` and replace content with a stub.
/// Returns how many messages were externalized.
pub fn externalize_tool_outputs(dir: &Path, messages: &mut [Message]) -> anyhow::Result<usize> {
    use crate::session::Role;
    let mut n = 0usize;
    for msg in messages.iter_mut() {
        if msg.role != Role::Tool {
            continue;
        }
        if msg.content.text().len() <= TOOL_OUTPUT_INLINE_MAX {
            continue;
        }
        if parse_external_output_ref(msg.content.text()).is_some() {
            continue;
        }
        let id = msg
            .tool_call_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("tool");
        let stem = sanitize_output_stem(id);
        let file_name = format!("{stem}.txt");
        let out = outputs_dir(dir);
        ensure_dir(&out)?;
        let path = out.join(&file_name);
        fs::write(&path, msg.content.text())?;
        let preview: String = msg.content.text().chars().take(480).collect();
        msg.content = format!("{EXTERNAL_PREFIX}{file_name}]\n{preview}\n…").into();
        n += 1;
    }
    Ok(n)
}

/// Rehydrate `[external:outputs/…]` stubs back into full tool bodies (best-effort).
pub fn hydrate_external_outputs(dir: &Path, session: &mut Session) {
    use crate::session::Role;
    for msg in &mut session.messages {
        if msg.role != Role::Tool {
            continue;
        }
        let Some(name) = parse_external_output_ref(msg.content.text()) else {
            continue;
        };
        let path = outputs_dir(dir).join(name);
        match fs::read_to_string(&path) {
            Ok(full) if !full.is_empty() => msg.content = full.into(),
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    %e,
                    "missing external tool output; keeping stub"
                );
            }
        }
    }
}

/// Append only the trailing `new_count` messages (tool-result flush).
pub fn append_messages(
    dir: &Path,
    messages: &[Message],
    new_count: usize,
    manifest: &mut Manifest,
) -> anyhow::Result<()> {
    if new_count == 0 || messages.is_empty() {
        return Ok(());
    }
    ensure_dir(dir)?;
    let path = messages_path(dir);
    let start = messages.len().saturating_sub(new_count);
    // The append path must strip images too — otherwise the flush window
    // between mid-turn flush and the turn-end rewrite would persist base64.
    let mut tail: Vec<Message> = messages[start..].to_vec();
    for m in &mut tail {
        m.content.strip_images();
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let ts = now_rfc3339();
    for msg in &tail {
        let rec = JsonlRecord {
            ts: ts.clone(),
            msg: msg.clone(),
        };
        serde_json::to_writer(&mut f, &rec)?;
        f.write_all(b"\n")?;
    }
    f.sync_all()?;
    manifest.msg_count = messages.len();
    manifest.estimated_tokens = tokens::estimate_messages(messages);
    manifest.updated_at = now_rfc3339();
    write_manifest(dir, manifest)?;
    Ok(())
}

fn write_manifest(dir: &Path, manifest: &Manifest) -> anyhow::Result<()> {
    ensure_dir(dir)?;
    let tmp = dir.join("manifest.json.tmp");
    let final_path = manifest_path(dir);
    {
        let mut f = File::create(&tmp)?;
        serde_json::to_writer_pretty(&mut f, manifest)?;
        f.write_all(b"\n")?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &final_path)?;
    Ok(())
}

/// Atomic checkpoint commit: write epoch-N.tmp → fsync → rename → update manifest pointer.
pub fn commit_checkpoint(
    dir: &Path,
    checkpoint: &Checkpoint,
    manifest: &mut Manifest,
) -> anyhow::Result<()> {
    // Reject stale CAS: epoch must be newer than the committed one.
    // Gap epochs (epoch > committed+1) are allowed: commit advances the pointer.
    if checkpoint.epoch <= manifest.committed_epoch {
        anyhow::bail!(
            "stale checkpoint epoch {} (committed={})",
            checkpoint.epoch,
            manifest.committed_epoch
        );
    }

    let cp_dir = checkpoints_dir(dir);
    ensure_dir(&cp_dir)?;
    let tmp = cp_dir.join(format!("epoch-{}.json.tmp", checkpoint.epoch));
    let final_path = checkpoint_path(dir, checkpoint.epoch);
    {
        let mut f = File::create(&tmp)?;
        serde_json::to_writer_pretty(&mut f, checkpoint)?;
        f.write_all(b"\n")?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &final_path)?;

    manifest.committed_epoch = checkpoint.epoch;
    manifest.updated_at = now_rfc3339();
    write_manifest(dir, manifest)?;
    // Silent GC after commit (ADR-036 S2) — never popup.
    if let Err(e) = gc_session_dir(dir) {
        tracing::warn!(%e, "session gc after checkpoint failed");
    }
    Ok(())
}

/// Build a structured checkpoint from the condensed user message + session meta.
pub fn checkpoint_from_compact(
    session_id: &str,
    _session: &Session,
    parent_epoch: u32,
    summary_natural: &str,
    compression_level: &str,
    msg_range: [usize; 2],
) -> Checkpoint {
    let (goals, decisions, open_items) = extract_structured_fields(summary_natural);
    Checkpoint {
        version: CHECKPOINT_VERSION,
        session_id: session_id.to_string(),
        epoch: parent_epoch.saturating_add(1),
        parent_epoch,
        compression_level: compression_level.to_string(),
        msg_range,
        summary_natural: summary_natural.to_string(),
        goals,
        decisions,
        open_items,
        artifacts: Vec::new(),
        created_at: now_rfc3339(),
    }
}

/// Heuristic: pull bullet-like lines into goals / open items; keep rest as decisions.
fn extract_structured_fields(summary: &str) -> (Vec<String>, Vec<CheckpointDecision>, Vec<String>) {
    let mut goals = Vec::new();
    let mut decisions = Vec::new();
    let mut open_items = Vec::new();
    for raw in summary.lines() {
        let line = raw.trim().trim_start_matches(['-', '*', '•']).trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.contains("todo")
            || lower.contains("open")
            || line.contains("未完成")
            || line.contains("待办")
            || line.contains("阻塞")
        {
            open_items.push(line.to_string());
        } else if lower.starts_with("goal") || line.contains("目标") || lower.starts_with("task")
        {
            goals.push(line.to_string());
        } else if decisions.len() < 12 {
            decisions.push(CheckpointDecision {
                what: line.to_string(),
                why: String::new(),
                files: Vec::new(),
            });
        }
    }
    if goals.is_empty() && !summary.trim().is_empty() {
        goals.push(
            summary
                .lines()
                .next()
                .unwrap_or(summary)
                .trim()
                .chars()
                .take(200)
                .collect(),
        );
    }
    (goals, decisions, open_items)
}

/// Remove session directory (user deleted chat).
pub fn delete_session_dir(dir: &Path) -> anyhow::Result<()> {
    if dir.is_dir() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// `{work_dir}/.stitch/sessions`
pub fn sessions_root(work_dir: &Path) -> PathBuf {
    work_dir.join(".stitch").join("sessions")
}

/// List session id directories under the work dir (sanitized names only).
pub fn list_persisted_session_ids(work_dir: &Path) -> anyhow::Result<Vec<String>> {
    let root = sessions_root(work_dir);
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for ent in fs::read_dir(&root)? {
        let ent = ent?;
        if !ent.file_type()?.is_dir() {
            continue;
        }
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if sanitize_session_id(&name).is_some() {
            ids.push(name.into_owned());
        }
    }
    ids.sort();
    Ok(ids)
}

/// Delete on-disk session dirs whose ids are not in `keep_ids` (silent; ADR-036 S3).
pub fn gc_orphan_sessions(work_dir: &Path, keep_ids: &[String]) -> anyhow::Result<usize> {
    use std::collections::HashSet;
    let keep: HashSet<String> = keep_ids
        .iter()
        .filter_map(|s| sanitize_session_id(s))
        .collect();
    let mut removed = 0usize;
    for id in list_persisted_session_ids(work_dir)? {
        if keep.contains(&id) {
            continue;
        }
        let Some(dir) = session_dir(work_dir, &id) else {
            continue;
        };
        match delete_session_dir(&dir) {
            Ok(()) => {
                removed += 1;
                tracing::info!(%id, path = %dir.display(), "orphan agent session gc");
            }
            Err(e) => tracing::warn!(%e, %id, "orphan agent session gc failed"),
        }
    }
    Ok(removed)
}

/// Pointer to the newest checkpoint anywhere under a work dir (optional exclude).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCheckpointRef {
    pub session_id: String,
    pub epoch: u32,
    pub summary_preview: String,
    pub resume_text: String,
    pub created_at: String,
}

/// Newest checkpoint across all persisted sessions in `work_dir`.
pub fn latest_workspace_checkpoint(
    work_dir: &Path,
    exclude_session_id: Option<&str>,
) -> anyhow::Result<Option<WorkspaceCheckpointRef>> {
    let exclude = exclude_session_id.and_then(sanitize_session_id);
    let mut best: Option<(String, Checkpoint)> = None;
    for id in list_persisted_session_ids(work_dir)? {
        if exclude.as_ref() == Some(&id) {
            continue;
        }
        let Some(dir) = session_dir(work_dir, &id) else {
            continue;
        };
        let Some(sum) = list_checkpoints(&dir)?.into_iter().next() else {
            continue;
        };
        let Some(cp) = load_checkpoint(&dir, sum.epoch)? else {
            continue;
        };
        let take = match &best {
            None => true,
            Some((_, cur)) => {
                cp.created_at > cur.created_at
                    || (cp.created_at == cur.created_at && cp.epoch > cur.epoch)
            }
        };
        if take {
            best = Some((id, cp));
        }
    }
    Ok(best.map(|(session_id, cp)| WorkspaceCheckpointRef {
        session_id,
        epoch: cp.epoch,
        summary_preview: truncate_preview(&cp.summary_natural, 160),
        resume_text: format_checkpoint_for_resume(&cp),
        created_at: cp.created_at,
    }))
}

// ─── S2: GC / diff / manual rollback (ADR-036) ───────────────────────────────

/// Result of silent session GC.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GcReport {
    pub checkpoints_removed: usize,
    pub outputs_removed: usize,
}

/// Lightweight checkpoint row for UI / IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointSummary {
    pub epoch: u32,
    pub parent_epoch: u32,
    pub compression_level: String,
    pub summary_preview: String,
    pub created_at: String,
}

/// Structured diff between two committed epochs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointDiff {
    pub from_epoch: u32,
    pub to_epoch: u32,
    pub summary_changed: bool,
    pub goals_added: Vec<String>,
    pub goals_removed: Vec<String>,
    pub decisions_added: Vec<String>,
    pub decisions_removed: Vec<String>,
    pub open_items_added: Vec<String>,
    pub open_items_removed: Vec<String>,
    pub artifacts_added: Vec<String>,
    pub artifacts_removed: Vec<String>,
    /// Neutral L1 text for UI / Agent.
    pub text: String,
}

fn list_checkpoint_epochs(dir: &Path) -> anyhow::Result<Vec<u32>> {
    let cp_dir = checkpoints_dir(dir);
    if !cp_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut epochs: Vec<u32> = Vec::new();
    for ent in fs::read_dir(&cp_dir)? {
        let ent = ent?;
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if let Some(rest) = name.strip_prefix("epoch-")
            && let Some(num) = rest.strip_suffix(".json")
            && let Ok(n) = num.parse::<u32>()
        {
            epochs.push(n);
        }
    }
    epochs.sort_unstable();
    Ok(epochs)
}

fn load_checkpoint(dir: &Path, epoch: u32) -> anyhow::Result<Option<Checkpoint>> {
    let path = checkpoint_path(dir, epoch);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    match serde_json::from_str::<Checkpoint>(&raw) {
        Ok(cp) => Ok(Some(cp)),
        Err(e) => {
            tracing::warn!(%e, epoch, path = %path.display(), "corrupt checkpoint skipped");
            Ok(None)
        }
    }
}

/// Drop checkpoint files older than the newest `KEEP_CHECKPOINT_EPOCHS`.
pub fn prune_old_checkpoints(dir: &Path) -> anyhow::Result<usize> {
    let epochs = list_checkpoint_epochs(dir)?;
    if epochs.len() <= KEEP_CHECKPOINT_EPOCHS {
        return Ok(0);
    }
    let keep_from = epochs.len() - KEEP_CHECKPOINT_EPOCHS;
    let mut removed = 0usize;
    for &epoch in &epochs[..keep_from] {
        let path = checkpoint_path(dir, epoch);
        match fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) => tracing::warn!(%e, epoch, "failed to prune checkpoint"),
        }
        let tmp = checkpoints_dir(dir).join(format!("epoch-{epoch}.json.tmp"));
        let _ = fs::remove_file(tmp);
    }
    Ok(removed)
}

fn collect_referenced_output_names(
    dir: &Path,
) -> anyhow::Result<std::collections::HashSet<String>> {
    use std::collections::HashSet;
    let mut refs = HashSet::new();
    let msg_path = messages_path(dir);
    if msg_path.is_file() {
        let file = File::open(&msg_path)?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(rec) = serde_json::from_str::<JsonlRecord>(line)
                && let Some(name) = parse_external_output_ref(rec.msg.content.text())
            {
                refs.insert(name.to_string());
            }
        }
    }
    Ok(refs)
}

/// Remove `outputs/*.txt` not referenced by current `messages.jsonl`.
pub fn prune_orphan_outputs(dir: &Path) -> anyhow::Result<usize> {
    let out = outputs_dir(dir);
    if !out.is_dir() {
        return Ok(0);
    }
    let refs = collect_referenced_output_names(dir)?;
    let mut removed = 0usize;
    for ent in fs::read_dir(&out)? {
        let ent = ent?;
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if !name.ends_with(".txt") {
            continue;
        }
        if refs.contains(name.as_ref()) {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) => tracing::warn!(%e, file = %name, "failed to prune orphan output"),
        }
    }
    Ok(removed)
}

/// Silent GC: prune old epochs + orphan tool outputs. No user prompt.
pub fn gc_session_dir(dir: &Path) -> anyhow::Result<GcReport> {
    let checkpoints_removed = prune_old_checkpoints(dir)?;
    let outputs_removed = prune_orphan_outputs(dir)?;
    if checkpoints_removed > 0 || outputs_removed > 0 {
        tracing::info!(
            checkpoints_removed,
            outputs_removed,
            path = %dir.display(),
            "session autopilot gc"
        );
    }
    Ok(GcReport {
        checkpoints_removed,
        outputs_removed,
    })
}

fn truncate_preview(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max).collect();
    out.push('…');
    out
}

/// List readable checkpoints, newest first.
pub fn list_checkpoints(dir: &Path) -> anyhow::Result<Vec<CheckpointSummary>> {
    let mut out = Vec::new();
    for epoch in list_checkpoint_epochs(dir)?.into_iter().rev() {
        if let Some(cp) = load_checkpoint(dir, epoch)? {
            out.push(CheckpointSummary {
                epoch: cp.epoch,
                parent_epoch: cp.parent_epoch,
                compression_level: cp.compression_level,
                summary_preview: truncate_preview(&cp.summary_natural, 160),
                created_at: cp.created_at,
            });
        }
    }
    Ok(out)
}

fn set_diff(old: &[String], new: &[String]) -> (Vec<String>, Vec<String>) {
    let old_set: std::collections::HashSet<&str> = old.iter().map(|s| s.as_str()).collect();
    let new_set: std::collections::HashSet<&str> = new.iter().map(|s| s.as_str()).collect();
    let added: Vec<String> = new
        .iter()
        .filter(|s| !old_set.contains(s.as_str()))
        .cloned()
        .collect();
    let removed: Vec<String> = old
        .iter()
        .filter(|s| !new_set.contains(s.as_str()))
        .cloned()
        .collect();
    (added, removed)
}

/// Diff two checkpoint epochs (from → to). Either order accepted; fields describe `to − from`.
pub fn diff_checkpoints(
    dir: &Path,
    from_epoch: u32,
    to_epoch: u32,
) -> anyhow::Result<CheckpointDiff> {
    let from = load_checkpoint(dir, from_epoch)?
        .ok_or_else(|| anyhow::anyhow!("checkpoint epoch {from_epoch} not found"))?;
    let to = load_checkpoint(dir, to_epoch)?
        .ok_or_else(|| anyhow::anyhow!("checkpoint epoch {to_epoch} not found"))?;

    let from_goals = &from.goals;
    let to_goals = &to.goals;
    let (goals_added, goals_removed) = set_diff(from_goals, to_goals);

    let from_dec: Vec<String> = from
        .decisions
        .iter()
        .map(|d| {
            if d.why.is_empty() {
                d.what.clone()
            } else {
                format!("{} ({})", d.what, d.why)
            }
        })
        .collect();
    let to_dec: Vec<String> = to
        .decisions
        .iter()
        .map(|d| {
            if d.why.is_empty() {
                d.what.clone()
            } else {
                format!("{} ({})", d.what, d.why)
            }
        })
        .collect();
    let (decisions_added, decisions_removed) = set_diff(&from_dec, &to_dec);

    let (open_items_added, open_items_removed) = set_diff(&from.open_items, &to.open_items);

    let from_art: Vec<String> = from
        .artifacts
        .iter()
        .map(|a| format!("{} ({})", a.title, a.local_id))
        .collect();
    let to_art: Vec<String> = to
        .artifacts
        .iter()
        .map(|a| format!("{} ({})", a.title, a.local_id))
        .collect();
    let (artifacts_added, artifacts_removed) = set_diff(&from_art, &to_art);

    let summary_changed = from.summary_natural.trim() != to.summary_natural.trim();

    let mut lines = Vec::new();
    lines.push(format!("检查点 {from_epoch} → {to_epoch}"));
    if summary_changed {
        lines.push("摘要有更新".into());
    }
    push_diff_section(&mut lines, "目标", &goals_added, &goals_removed);
    push_diff_section(&mut lines, "决策", &decisions_added, &decisions_removed);
    push_diff_section(&mut lines, "未完成", &open_items_added, &open_items_removed);
    push_diff_section(&mut lines, "产物", &artifacts_added, &artifacts_removed);
    if lines.len() == 1 {
        lines.push("无结构化差异".into());
    }

    Ok(CheckpointDiff {
        from_epoch,
        to_epoch,
        summary_changed,
        goals_added,
        goals_removed,
        decisions_added,
        decisions_removed,
        open_items_added,
        open_items_removed,
        artifacts_added,
        artifacts_removed,
        text: lines.join("\n"),
    })
}

fn push_diff_section(lines: &mut Vec<String>, label: &str, added: &[String], removed: &[String]) {
    if added.is_empty() && removed.is_empty() {
        return;
    }
    lines.push(format!("{label}:"));
    for a in added {
        lines.push(format!("  + {a}"));
    }
    for r in removed {
        lines.push(format!("  - {r}"));
    }
}

fn session_from_checkpoint(cp: &Checkpoint) -> Session {
    let mut session =
        Session::new("You are a coding agent. Conversation restored from a checkpoint.");
    session.epoch = cp.epoch;
    session.add_user_message(format!(
        "[Restored checkpoint epoch {}]\n\n{}",
        cp.epoch,
        format_checkpoint_for_resume(cp)
    ));
    session
}

/// Manual rollback: restore authoritative session from `target_epoch`, drop newer checkpoints.
///
/// Requires an existing checkpoint file for `target_epoch` and `target_epoch < committed`.
pub fn rollback_to_epoch(
    dir: &Path,
    target_epoch: u32,
    manifest: &mut Manifest,
) -> anyhow::Result<(Session, Checkpoint)> {
    if target_epoch >= manifest.committed_epoch {
        anyhow::bail!(
            "rollback target {target_epoch} must be older than committed {}",
            manifest.committed_epoch
        );
    }
    let cp = load_checkpoint(dir, target_epoch)?
        .ok_or_else(|| anyhow::anyhow!("checkpoint epoch {target_epoch} not found"))?;

    let session = session_from_checkpoint(&cp);

    // Drop checkpoints newer than target before rewriting messages.
    for epoch in list_checkpoint_epochs(dir)? {
        if epoch > target_epoch {
            let path = checkpoint_path(dir, epoch);
            let _ = fs::remove_file(path);
            let tmp = checkpoints_dir(dir).join(format!("epoch-{epoch}.json.tmp"));
            let _ = fs::remove_file(tmp);
        }
    }

    manifest.committed_epoch = target_epoch;
    manifest.restore_degraded = Some(format!("manual_rollback_to_{target_epoch}"));
    save_session(dir, &session, manifest)?;
    let _ = gc_session_dir(dir)?;
    Ok((session, cp))
}

/// Soft threshold (~70%) and hard (~85%) against a context limit.
pub fn soft_token_limit(context_limit: usize) -> usize {
    context_limit.saturating_mul(70) / 100
}

pub fn hard_token_limit(context_limit: usize) -> usize {
    context_limit.saturating_mul(85) / 100
}

// ─── Turn-level incremental flush (crash-safe mid-turn persistence) ─────────
//
// A long turn may run dozens of tool calls before `Done`. Persisting only at
// turn end loses all of that on crash / kill. `TurnFlusher` appends new
// messages to `messages.jsonl` as the turn progresses (cheap path), and does a
// full rewrite + checkpoint commit when a hard compact bumps the epoch.
// A user-initiated stop keeps the "discard this turn" semantics via `rollback`.

/// Disk anchor captured at turn begin; rollback restores exactly this state.
#[derive(Debug, Clone)]
pub struct TurnBeginMark {
    pub epoch: u32,
    pub committed_epoch: u32,
    pub msg_count: usize,
}

/// Incremental flusher for one live turn. All ops are best-effort: errors are
/// logged and never break the agent loop.
#[derive(Debug)]
pub struct TurnFlusher {
    dir: PathBuf,
    session_id: String,
    manifest: Manifest,
    flushed_msgs: usize,
    last_epoch: u32,
    mark: TurnBeginMark,
}

impl TurnFlusher {
    /// Anchor at the pre-turn session (`before` = session without this turn's
    /// user message). Returns `None` for invalid session ids.
    pub fn begin(work_dir: &Path, chat_id: &str, before: &Session) -> Option<Self> {
        let dir = session_dir(work_dir, chat_id)?;
        let mut manifest = Manifest::new(chat_id, work_dir);
        if let Ok(raw) = fs::read_to_string(manifest_path(&dir))
            && let Ok(prev) = serde_json::from_str::<Manifest>(&raw)
        {
            manifest.created_at = prev.created_at;
            manifest.committed_epoch = prev.committed_epoch;
        }
        Some(Self {
            dir,
            session_id: chat_id.to_string(),
            flushed_msgs: before.messages.len(),
            last_epoch: before.epoch,
            mark: TurnBeginMark {
                epoch: before.epoch,
                committed_epoch: manifest.committed_epoch,
                msg_count: before.messages.len(),
            },
            manifest,
        })
    }

    pub fn mark(&self) -> &TurnBeginMark {
        &self.mark
    }

    /// Flush messages appended since the last flush. Epoch bump (hard compact)
    /// forces a full rewrite + checkpoint commit; otherwise append-only.
    pub fn flush(&mut self, session: &Session) {
        if session.epoch != self.last_epoch {
            let parent = session.epoch.saturating_sub(1);
            let summary = crate::agent::context::condensed_summary_text(session).unwrap_or("");
            let cp = checkpoint_from_compact(
                &self.session_id,
                session,
                parent,
                summary,
                "full",
                [0, session.messages.len().saturating_sub(1)],
            );
            if let Err(e) = commit_checkpoint(&self.dir, &cp, &mut self.manifest) {
                tracing::warn!(%e, "turn flush checkpoint commit failed");
            }
            if let Err(e) = save_session(&self.dir, session, &mut self.manifest) {
                tracing::warn!(%e, "turn flush save failed");
            }
            self.flushed_msgs = session.messages.len();
            self.last_epoch = session.epoch;
            return;
        }
        if session.messages.len() > self.flushed_msgs {
            let new_count = session.messages.len() - self.flushed_msgs;
            if let Err(e) =
                append_messages(&self.dir, &session.messages, new_count, &mut self.manifest)
            {
                tracing::warn!(%e, "turn flush append failed");
            }
            self.flushed_msgs = session.messages.len();
        }
    }

    /// User stopped the turn: restore disk to the begin mark so the aborted
    /// turn does not resurface after an app restart (stop = discard turn).
    pub fn rollback(&mut self, before: &Session) {
        self.manifest.committed_epoch = self.mark.committed_epoch;
        if let Err(e) = save_session(&self.dir, before, &mut self.manifest) {
            tracing::warn!(%e, "turn rollback save failed");
        }
        // save_session pins committed_epoch = before.epoch; re-assert the mark.
        self.manifest.committed_epoch = self.mark.committed_epoch;
        if let Err(e) = write_manifest(&self.dir, &self.manifest) {
            tracing::warn!(%e, "turn rollback manifest failed");
        }
        // Drop checkpoints committed during the aborted turn.
        if let Ok(epochs) = list_checkpoint_epochs(&self.dir) {
            for epoch in epochs
                .into_iter()
                .filter(|e| *e > self.mark.committed_epoch)
            {
                let _ = fs::remove_file(checkpoint_path(&self.dir, epoch));
                let tmp = checkpoints_dir(&self.dir).join(format!("epoch-{epoch}.json.tmp"));
                let _ = fs::remove_file(tmp);
            }
        }
        self.flushed_msgs = before.messages.len();
        self.last_epoch = before.epoch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Role;
    use tempfile::tempdir;

    #[test]
    fn sanitize_rejects_path_chars() {
        assert!(sanitize_session_id("../x").is_none());
        assert!(sanitize_session_id("ok-id_1").is_some());
    }

    #[test]
    fn roundtrip_save_load() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess1";
        let dir = session_dir(work, sid).unwrap();
        let mut session = Session::new("system");
        session.add_user_message("hello");
        session.add_assistant_message("hi");
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &session, &mut man).unwrap();

        let (loaded, man2) = load_session(&dir).unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 3);
        assert_eq!(loaded.messages[1].content.text(), "hello");
        assert_eq!(man2.msg_count, 3);
        assert_eq!(man2.session_id, sid);
    }

    #[test]
    fn checkpoint_cas_and_fallback() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess2";
        let dir = session_dir(work, sid).unwrap();
        let mut session = Session::new("system");
        session.add_user_message("a");
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &session, &mut man).unwrap();

        let cp = checkpoint_from_compact(
            sid,
            &session,
            0,
            "- goal: ship\n- 阻塞: disk",
            "full",
            [0, 1],
        );
        assert_eq!(cp.epoch, 1);
        commit_checkpoint(&dir, &cp, &mut man).unwrap();
        assert_eq!(man.committed_epoch, 1);

        // Stale rejected
        let stale = checkpoint_from_compact(sid, &session, 0, "old", "full", [0, 1]);
        assert!(commit_checkpoint(&dir, &stale, &mut man).is_err());

        // Corrupt messages → fallback still yields session
        fs::write(messages_path(&dir), "{not json\n").unwrap();
        let (fb, man_fb) = load_session(&dir).unwrap().unwrap();
        assert!(man_fb.restore_degraded.is_some());
        assert_eq!(fb.epoch, 1);
        assert!(fb.messages.iter().any(|m| m.content.contains("ship")));
    }

    #[test]
    fn tool_output_externalize_roundtrip() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-ext";
        let dir = session_dir(work, sid).unwrap();
        let mut session = Session::new("system");
        session.add_user_message("list");
        let big = "X".repeat(TOOL_OUTPUT_INLINE_MAX + 200);
        session.messages.push(Message {
            role: Role::Assistant,
            content: String::new().into(),
            tool_calls: Some(vec![crate::session::ToolCall {
                id: "call_abc".into(),
                call_type: "function".into(),
                function: crate::session::FunctionCall {
                    name: "list_directory".into(),
                    arguments: "{}".into(),
                },
            }]),
            tool_call_id: None,
        });
        session.messages.push(Message {
            role: Role::Tool,
            content: big.clone().into(),
            tool_calls: None,
            tool_call_id: Some("call_abc".into()),
        });
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &session, &mut man).unwrap();

        // Live session unchanged
        assert_eq!(session.messages.last().unwrap().content.text(), &big);

        // Disk has stub
        let raw = fs::read_to_string(messages_path(&dir)).unwrap();
        assert!(raw.contains("[external:outputs/call_abc.txt]"));
        assert!(!raw.contains(&big));
        assert!(dir.join("outputs").join("call_abc.txt").is_file());

        let (loaded, _) = load_session(&dir).unwrap().unwrap();
        let tool = loaded
            .messages
            .iter()
            .find(|m| m.role == Role::Tool)
            .unwrap();
        assert_eq!(tool.content.text(), &big);
    }

    #[test]
    fn append_grows_jsonl() {
        let tmp = tempdir().unwrap();
        let dir = session_dir(tmp.path(), "s3").unwrap();
        let mut session = Session::new("sys");
        let mut man = Manifest::new("s3", tmp.path());
        save_session(&dir, &session, &mut man).unwrap();
        session.add_user_message("u1");
        append_messages(&dir, &session.messages, 1, &mut man).unwrap();
        let (loaded, _) = load_session(&dir).unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 2);
    }

    #[test]
    fn gc_keeps_last_three_epochs_and_orphans() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-gc";
        let dir = session_dir(work, sid).unwrap();
        let mut session = Session::new("system");
        session.add_user_message("a");
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &session, &mut man).unwrap();

        for parent in 0u32..4 {
            let cp = checkpoint_from_compact(
                sid,
                &session,
                parent,
                &format!("- goal: e{}", parent + 1),
                "full",
                [0, 1],
            );
            commit_checkpoint(&dir, &cp, &mut man).unwrap();
            session.epoch = cp.epoch;
        }
        assert_eq!(man.committed_epoch, 4);
        let epochs = list_checkpoint_epochs(&dir).unwrap();
        assert_eq!(epochs, vec![2, 3, 4]);

        // Orphan output file
        let out = outputs_dir(&dir);
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("orphan.txt"), "gone").unwrap();
        let report = gc_session_dir(&dir).unwrap();
        assert_eq!(report.outputs_removed, 1);
        assert!(!out.join("orphan.txt").is_file());
    }

    #[test]
    fn diff_and_manual_rollback() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-rb";
        let dir = session_dir(work, sid).unwrap();
        let mut session = Session::new("system");
        session.add_user_message("start");
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &session, &mut man).unwrap();

        let cp1 = checkpoint_from_compact(
            sid,
            &session,
            0,
            "- goal: alpha\n- 待办: one",
            "full",
            [0, 1],
        );
        commit_checkpoint(&dir, &cp1, &mut man).unwrap();
        session.epoch = 1;

        let cp2 = checkpoint_from_compact(
            sid,
            &session,
            1,
            "- goal: beta\n- 待办: two",
            "full",
            [0, 2],
        );
        commit_checkpoint(&dir, &cp2, &mut man).unwrap();
        session.epoch = 2;
        assert_eq!(man.committed_epoch, 2);

        let diff = diff_checkpoints(&dir, 1, 2).unwrap();
        assert!(diff.summary_changed);
        assert!(diff.goals_added.iter().any(|g| g.contains("beta")));
        assert!(diff.goals_removed.iter().any(|g| g.contains("alpha")));
        assert!(diff.text.contains("检查点 1 → 2"));

        let listed = list_checkpoints(&dir).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].epoch, 2);

        let (restored, cp) = rollback_to_epoch(&dir, 1, &mut man).unwrap();
        assert_eq!(cp.epoch, 1);
        assert_eq!(man.committed_epoch, 1);
        assert_eq!(restored.epoch, 1);
        assert!(
            man.restore_degraded
                .as_deref()
                .unwrap()
                .contains("manual_rollback")
        );
        assert!(!checkpoint_path(&dir, 2).is_file());
        assert!(checkpoint_path(&dir, 1).is_file());
        assert!(
            restored
                .messages
                .iter()
                .any(|m| m.content.contains("alpha"))
        );

        // Cannot rollback to current
        assert!(rollback_to_epoch(&dir, 1, &mut man).is_err());
    }

    #[test]
    fn turn_flusher_incremental_and_crash_resume() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-flush";
        let dir = session_dir(work, sid).unwrap();

        // Pre-turn state persisted (as a finished turn would be).
        let mut before = Session::new("system");
        before.add_user_message(" earlier");
        before.add_assistant_message("ok");
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &before, &mut man).unwrap();

        let mut flusher = TurnFlusher::begin(work, sid, &before).unwrap();
        assert_eq!(flusher.mark().msg_count, before.messages.len());

        // Live turn: user message + tool round.
        let mut live = before.clone();
        live.add_user_message("do a long task");
        flusher.flush(&live);
        live.add_assistant_tool_calls(
            String::new(),
            vec![crate::session::ToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: crate::session::FunctionCall {
                    name: "run_command".into(),
                    arguments: "{}".into(),
                },
            }],
        );
        live.add_tool_result("c1".to_string(), "partial output");
        flusher.flush(&live);

        // Crash = no turn-end save. Disk must already carry the partial turn.
        let (loaded, _) = load_session(&dir).unwrap().unwrap();
        assert_eq!(loaded.messages.len(), live.messages.len());
        assert!(
            loaded
                .messages
                .iter()
                .any(|m| m.content.contains("do a long task"))
        );
        assert!(
            loaded
                .messages
                .iter()
                .any(|m| m.content.contains("partial output"))
        );
    }

    #[test]
    fn turn_flusher_epoch_bump_commits_checkpoint() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-flush-epoch";
        let dir = session_dir(work, sid).unwrap();

        let before = Session::new("system");
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &before, &mut man).unwrap();

        let mut flusher = TurnFlusher::begin(work, sid, &before).unwrap();

        // Simulate a hard compact: epoch bumped, history replaced by summary.
        let mut live = before.clone();
        live.add_user_message(
            "[Earlier conversation — condensed]\n\n- goal: keep going\n- 待办: step two",
        );
        live.epoch = 1;
        flusher.flush(&live);

        let epochs = list_checkpoint_epochs(&dir).unwrap();
        assert_eq!(epochs, vec![1]);
        let (loaded, man2) = load_session(&dir).unwrap().unwrap();
        assert_eq!(loaded.epoch, 1);
        assert_eq!(man2.committed_epoch, 1);

        // Same epoch → incremental path, no duplicate checkpoint.
        live.add_user_message("next");
        flusher.flush(&live);
        assert_eq!(list_checkpoint_epochs(&dir).unwrap(), vec![1]);
    }

    #[test]
    fn turn_flusher_rollback_restores_begin_mark() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-flush-rb";
        let dir = session_dir(work, sid).unwrap();

        let mut before = Session::new("system");
        before.add_user_message("stable history");
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &before, &mut man).unwrap();

        let mut flusher = TurnFlusher::begin(work, sid, &before).unwrap();

        // Aborted turn: appended messages + a compact checkpoint.
        let mut live = before.clone();
        live.add_user_message("stop me midway");
        live.add_assistant_message("working…");
        flusher.flush(&live);
        live.epoch = 1;
        live.messages.truncate(2);
        flusher.flush(&live);
        assert_eq!(list_checkpoint_epochs(&dir).unwrap(), vec![1]);

        flusher.rollback(&before);

        let (loaded, man2) = load_session(&dir).unwrap().unwrap();
        assert_eq!(loaded.messages.len(), before.messages.len());
        assert!(
            loaded
                .messages
                .iter()
                .any(|m| m.content.contains("stable history"))
        );
        assert!(
            !loaded
                .messages
                .iter()
                .any(|m| m.content.contains("stop me midway"))
        );
        assert_eq!(man2.committed_epoch, 0);
        assert!(list_checkpoint_epochs(&dir).unwrap().is_empty());
    }

    #[test]
    fn orphan_session_gc_and_workspace_latest() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let keep_id = "keep-me";
        let drop_id = "drop-me";
        let keep_dir = session_dir(work, keep_id).unwrap();
        let drop_dir = session_dir(work, drop_id).unwrap();

        let mut s_keep = Session::new("sys");
        s_keep.add_user_message("k");
        let mut man_k = Manifest::new(keep_id, work);
        save_session(&keep_dir, &s_keep, &mut man_k).unwrap();
        let cp = checkpoint_from_compact(keep_id, &s_keep, 0, "- goal: keep-goal", "full", [0, 1]);
        commit_checkpoint(&keep_dir, &cp, &mut man_k).unwrap();

        let mut s_drop = Session::new("sys");
        s_drop.add_user_message("d");
        let mut man_d = Manifest::new(drop_id, work);
        save_session(&drop_dir, &s_drop, &mut man_d).unwrap();

        assert!(drop_dir.is_dir());
        let n = gc_orphan_sessions(work, &[keep_id.to_string()]).unwrap();
        assert_eq!(n, 1);
        assert!(!drop_dir.is_dir());
        assert!(keep_dir.is_dir());

        let latest = latest_workspace_checkpoint(work, None).unwrap().unwrap();
        assert_eq!(latest.session_id, keep_id);
        assert_eq!(latest.epoch, 1);
        assert!(latest.resume_text.contains("keep-goal"));

        let none = latest_workspace_checkpoint(work, Some(keep_id)).unwrap();
        assert!(none.is_none());
    }

    fn warm_entry(goal: &str) -> crate::agent::layers::CompressedTurn {
        crate::agent::layers::CompressedTurn {
            user_goal: goal.into(),
            tool_summaries: vec![],
            decisions: vec![],
            files: vec![],
            keywords: vec![],
        }
    }

    #[test]
    fn layers_roundtrip_save_load() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-layers";
        let dir = session_dir(work, sid).unwrap();

        let mut session = Session::new("system");
        let lm = session.layers.as_mut().unwrap();
        lm.push_warm(warm_entry("first goal"));
        lm.push_warm(warm_entry("second goal"));
        assert_eq!(lm.warm.len(), 2);
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &session, &mut man).unwrap();
        assert!(layers_path(&dir).is_file());

        let (loaded, _) = load_session(&dir).unwrap().unwrap();
        let lm2 = loaded.layers.unwrap();
        assert_eq!(lm2.warm.len(), 2);
        assert_eq!(lm2.warm[0].user_goal, "first goal");
        assert_eq!(lm2.warm[1].user_goal, "second goal");
    }

    #[test]
    fn layers_epoch_filter_drops_newer_entries() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-layers-epoch";
        let dir = session_dir(work, sid).unwrap();

        let mut session = Session::new("system");
        session
            .layers
            .as_mut()
            .unwrap()
            .push_warm(warm_entry("stale"));
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &session, &mut man).unwrap(); // committed_epoch = 0

        // Simulate a crash leftover: archive stamped under epoch 1 while the
        // manifest still commits epoch 0.
        let file = LayersFile {
            version: LAYERS_VERSION,
            warm: vec![LayerRecord {
                epoch: 1,
                entry: warm_entry("stale"),
            }],
            cold: vec![],
        };
        fs::write(layers_path(&dir), serde_json::to_string(&file).unwrap()).unwrap();

        let (loaded, man2) = load_session(&dir).unwrap().unwrap();
        assert_eq!(man2.committed_epoch, 0);
        assert!(loaded.layers.unwrap().warm.is_empty());
    }

    #[test]
    fn layers_missing_or_corrupt_degrades_to_default() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-layers-bad";
        let dir = session_dir(work, sid).unwrap();
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &Session::new("system"), &mut man).unwrap();
        // Missing-file case: legacy session dir with no archive.
        fs::remove_file(layers_path(&dir)).unwrap();
        let (loaded, _) = load_session(&dir).unwrap().unwrap();
        assert_eq!(loaded.layers.unwrap().warm.len(), 0);

        fs::write(layers_path(&dir), "{not json").unwrap();
        let (loaded2, _) = load_session(&dir).unwrap().unwrap();
        assert_eq!(loaded2.layers.unwrap().warm.len(), 0);
    }

    #[test]
    fn layers_turn_flusher_rollback_restores_before_state() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-layers-rb";
        let dir = session_dir(work, sid).unwrap();

        let mut before = Session::new("system");
        before.add_user_message("stable history");
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &before, &mut man).unwrap();

        let mut flusher = TurnFlusher::begin(work, sid, &before).unwrap();

        // Aborted turn: compacted (epoch bump + warm entry) then flushed.
        let mut live = before.clone();
        live.epoch = 1;
        live.layers
            .as_mut()
            .unwrap()
            .push_warm(warm_entry("aborted turn"));
        flusher.flush(&live);
        let (mid, _) = load_session(&dir).unwrap().unwrap();
        assert_eq!(mid.layers.unwrap().warm.len(), 1);

        flusher.rollback(&before);

        let (loaded, man2) = load_session(&dir).unwrap().unwrap();
        assert_eq!(loaded.layers.unwrap().warm.len(), 0);
        assert_eq!(man2.committed_epoch, 0);
    }

    #[test]
    fn image_strip_save_load_roundtrip() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-img-strip";
        let dir = session_dir(work, sid).unwrap();

        let mut session = Session::new("system");
        session.add_user_message(crate::session::user_content_with_images(
            "看这张图",
            &["data:image/png;base64,AAAABBBB".into()],
        ));
        session.add_assistant_message("看到了");
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &session, &mut man).unwrap();

        // In-memory session still carries the image.
        assert_eq!(session.messages[1].content.image_count(), 1);
        // Disk has no image data, only the text (folded) — and no stub needed.
        let raw = fs::read_to_string(messages_path(&dir)).unwrap();
        assert!(!raw.contains("image_url"));
        assert!(!raw.contains("AAAABBBB"));
        assert!(!raw.contains(crate::session::IMAGE_STRIPPED_STUB));
        assert!(raw.contains("看这张图"));

        let (loaded, _) = load_session(&dir).unwrap().unwrap();
        for m in &loaded.messages {
            assert_eq!(m.content.image_count(), 0);
        }
    }

    #[test]
    fn append_messages_strips_images() {
        let tmp = tempdir().unwrap();
        let work = tmp.path();
        let sid = "sess-img-append";
        let dir = session_dir(work, sid).unwrap();

        let mut session = Session::new("system");
        session.add_user_message("first");
        let mut man = Manifest::new(sid, work);
        save_session(&dir, &session, &mut man).unwrap();

        // Flush window: append an image message (as TurnFlusher would).
        session.add_user_message(crate::session::user_content_with_images(
            "",
            &["data:image/png;base64,CCCCDDDD".into()],
        ));
        append_messages(&dir, &session.messages, 1, &mut man).unwrap();

        let raw = fs::read_to_string(messages_path(&dir)).unwrap();
        assert!(!raw.contains("image_url"));
        assert!(!raw.contains("CCCCDDDD"));
        assert!(raw.contains(crate::session::IMAGE_STRIPPED_STUB));
    }
}
