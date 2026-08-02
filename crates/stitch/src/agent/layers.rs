//! Three-tier context layering (ADR-036 extension).
//!
//! Hot (full detail) → Warm (compressed tool summaries) → Cold (goal + conclusion).
//! Automatic promotion when the user references archived content.

use crate::agent::tokens;
use crate::session::{Message, Role, Session};

/// A compressed representation of one turn, stored in the warm layer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompressedTurn {
    pub user_goal: String,
    pub tool_summaries: Vec<String>,
    pub decisions: Vec<String>,
    pub files: Vec<String>,
    /// Keywords extracted for reference detection.
    pub keywords: Vec<String>,
}

/// Minimal cold-layer entry: just the goal and conclusion.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ColdEntry {
    pub goal: String,
    pub conclusion: String,
    pub keywords: Vec<String>,
}

/// Per-tier token/message counts for UI and threshold checks.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct LayerStats {
    pub hot_msgs: usize,
    pub warm_entries: usize,
    pub cold_entries: usize,
    pub hot_tokens: usize,
    pub warm_tokens: usize,
    pub cold_tokens: usize,
    pub total_tokens: usize,
    pub limit: usize,
}

impl LayerStats {
    pub fn context_pct(&self) -> u8 {
        if self.limit == 0 {
            return 0;
        }
        let pct = (self.total_tokens.saturating_mul(100)) / self.limit;
        pct.min(100) as u8
    }
}

/// Configuration for automatic context layering.
#[derive(Debug, Clone)]
pub struct LayerConfig {
    /// Enable layering (default true).
    pub enabled: bool,
    /// Max messages to retain in hot layer.
    pub keep_recent: usize,
    /// Max warm entries before some are pushed to cold.
    pub max_warm: usize,
    /// Max cold entries before merging oldest.
    pub max_cold: usize,
    /// Max tokens before triggering compaction.
    pub max_tokens: usize,
}

impl Default for LayerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_recent: 20,
            max_warm: 10,
            max_cold: 20,
            max_tokens: 64_000,
        }
    }
}

/// Manages the three-tier context: Hot (full messages), Warm (compressed turns),
/// Cold (goal+conclusion).
#[derive(Debug, Clone)]
pub struct LayerManager {
    pub warm: Vec<CompressedTurn>,
    pub cold: Vec<ColdEntry>,
    pub config: LayerConfig,
}

impl LayerManager {
    pub fn new(config: LayerConfig) -> Self {
        Self {
            warm: Vec::new(),
            cold: Vec::new(),
            config,
        }
    }

    /// Check thresholds and compact if needed. Returns true if compaction occurred.
    pub fn tick(&mut self, messages: &[Message]) -> bool {
        if !self.config.enabled {
            return false;
        }
        let est = tokens::estimate_messages(messages);
        if est <= self.config.max_tokens {
            return false;
        }
        // Trigger warm compaction: push oldest hot messages to warm.
        // This is a heuristic step — the actual message replacement happens
        // in context.rs via the existing compact pipeline.
        tracing::info!(
            estimated_tokens = est,
            msg_count = messages.len(),
            warm_entries = self.warm.len(),
            cold_entries = self.cold.len(),
            "context layering triggered"
        );
        true
    }

    /// Build a warm entry from a user message and its following assistant/tool messages.
    pub fn compress_to_warm(turn_messages: &[Message]) -> CompressedTurn {
        let mut goal = String::new();
        let mut tool_summaries: Vec<String> = Vec::new();
        let mut decisions: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();
        let mut keywords: Vec<String> = Vec::new();

        for msg in turn_messages {
            match msg.role {
                crate::session::Role::User => {
                    goal = msg.content.text().chars().take(200).collect();
                    extract_keywords(msg.content.text(), &mut keywords);
                }
                crate::session::Role::Assistant => {
                    // Collect tool calls as summaries.
                    if let Some(ref calls) = msg.tool_calls {
                        for tc in calls {
                            let summary = format!("{} → …", tc.function.name);
                            tool_summaries.push(summary);
                        }
                    }
                    // Detect decision-like patterns.
                    for line in msg.content.text().lines() {
                        let trimmed = line.trim();
                        if trimmed.starts_with("- ")
                            || trimmed.starts_with("* ")
                            || trimmed.starts_with("1. ")
                        {
                            let d: String = trimmed.chars().take(120).collect();
                            decisions.push(d);
                        }
                    }
                    // Detect file paths.
                    for word in msg.content.text().split_whitespace() {
                        if word.contains('.') && word.len() >= 4 && !word.starts_with("http") {
                            let f: String = word.chars().take(80).collect();
                            files.push(f);
                        }
                    }
                    extract_keywords(msg.content.text(), &mut keywords);
                }
                crate::session::Role::Tool => {
                    // Extract first line of tool output as summary.
                    if let Some(line) = msg.content.text().lines().next() {
                        let s: String = line.chars().take(120).collect();
                        if !s.is_empty() {
                            tool_summaries.push(s);
                        }
                    }
                }
                _ => {}
            }
        }

        // Deduplicate keywords, keep top 20.
        keywords.sort();
        keywords.dedup();
        keywords.truncate(20);

        CompressedTurn {
            user_goal: goal,
            tool_summaries,
            decisions,
            files,
            keywords,
        }
    }

    /// Compress a warm entry to a cold entry.
    pub fn compress_to_cold(warm: &CompressedTurn) -> ColdEntry {
        ColdEntry {
            goal: warm.user_goal.clone(),
            conclusion: warm
                .decisions
                .first()
                .cloned()
                .unwrap_or_else(|| "(no conclusion)".into()),
            keywords: warm.keywords.clone(),
        }
    }

    /// Push a warm entry and cascade to cold if over limit.
    pub fn push_warm(&mut self, entry: CompressedTurn) {
        self.warm.push(entry);
        while self.warm.len() > self.config.max_warm {
            if let Some(oldest) = self.warm.first().cloned() {
                self.cold.push(Self::compress_to_cold(&oldest));
                self.warm.remove(0);
            }
        }
        while self.cold.len() > self.config.max_cold {
            // Merge two oldest cold entries.
            if self.cold.len() >= 2 {
                let a = self.cold.remove(0);
                let b = self.cold.remove(0);
                let mut merged_kw = a.keywords;
                merged_kw.extend(b.keywords);
                merged_kw.sort();
                merged_kw.dedup();
                merged_kw.truncate(20);
                self.cold.insert(
                    0,
                    ColdEntry {
                        goal: format!("{}; {}", a.goal, b.goal),
                        conclusion: b.conclusion,
                        keywords: merged_kw,
                    },
                );
            } else {
                break;
            }
        }
    }

    /// Split removed history (from a hard compact) into turns at `Role::User`
    /// boundaries, compress each to warm and push (cascade to cold).
    /// Head/tail segments without a user message merge into the adjacent turn.
    /// Returns how many warm entries were created (0 when disabled, empty,
    /// or the slice contains no user message).
    pub fn push_removed_range(&mut self, removed: &[Message]) -> usize {
        if !self.config.enabled || removed.is_empty() {
            return 0;
        }
        let mut created = 0;
        let mut chunk_start = 0usize;
        let mut first = true;
        for (i, msg) in removed.iter().enumerate() {
            if msg.role != crate::session::Role::User {
                continue;
            }
            if first {
                first = false;
                chunk_start = 0; // leading non-user segment merges into first turn
            } else {
                created += self.push_chunk(&removed[chunk_start..i]);
                chunk_start = i;
            }
        }
        if !first {
            // Trailing assistant/tool segment (if any) stays in the last turn.
            created += self.push_chunk(&removed[chunk_start..]);
        }
        if created > 0 {
            tracing::info!(
                removed = removed.len(),
                warm = created,
                warm_entries = self.warm.len(),
                cold_entries = self.cold.len(),
                "removed history layered into warm/cold"
            );
        }
        created
    }

    fn push_chunk(&mut self, chunk: &[Message]) -> usize {
        if chunk.is_empty() {
            return 0;
        }
        let entry = Self::compress_to_warm(chunk);
        self.push_warm(entry);
        1
    }

    /// Detect references to warm/cold entries in user text.
    /// Returns indices of warm entries that should be promoted to hot.
    pub fn detect_references(&self, user_text: &str) -> (Vec<usize>, Vec<usize>) {
        let mut warm_hits: Vec<usize> = Vec::new();
        let mut cold_hits: Vec<usize> = Vec::new();

        let lower = user_text.to_lowercase();

        for (i, entry) in self.warm.iter().enumerate() {
            let mut score = 0;
            for kw in &entry.keywords {
                if kw.len() >= 3 && lower.contains(&kw.to_lowercase()) {
                    score += 1;
                }
            }
            if score >= 2 {
                warm_hits.push(i);
            }
        }

        for (i, entry) in self.cold.iter().enumerate() {
            let mut score = 0;
            for kw in &entry.keywords {
                if kw.len() >= 3 && lower.contains(&kw.to_lowercase()) {
                    score += 1;
                }
            }
            if score >= 2 {
                cold_hits.push(i);
            }
        }

        (warm_hits, cold_hits)
    }

    /// Promote a warm entry back to hot (remove from warm, return the entry).
    pub fn promote_warm(&mut self, index: usize) -> Option<CompressedTurn> {
        if index < self.warm.len() {
            Some(self.warm.remove(index))
        } else {
            None
        }
    }

    /// Promote a cold entry to warm (remove from cold, return as CompressedTurn).
    pub fn promote_cold(&mut self, index: usize) -> Option<CompressedTurn> {
        if index < self.cold.len() {
            let entry = self.cold.remove(index);
            Some(CompressedTurn {
                user_goal: entry.goal,
                tool_summaries: vec![],
                decisions: vec![entry.conclusion],
                files: vec![],
                keywords: entry.keywords,
            })
        } else {
            None
        }
    }

    /// Detect references to archived content in user text and promote the
    /// matched warm/cold entries back into the hot context.
    ///
    /// Returns the formatted archive block to merge into the user message, or
    /// `None` when nothing matched. Promoted entries are consumed — their
    /// content now lives in the returned block, so they leave the archive.
    ///
    /// Note: mid-turn crash before the turn-end rewrite silently loses the
    /// promotion (the archive entries persist on disk and are re-detected on
    /// the next turn); a user rewind over the augmented message drops that
    /// segment entirely — accepted, the user discarded that dialogue.
    pub fn promote_referenced(&mut self, user_text: &str) -> Option<String> {
        if !self.config.enabled || (self.warm.is_empty() && self.cold.is_empty()) {
            return None;
        }
        if user_text.trim().is_empty() {
            return None;
        }
        let (warm_hits, cold_hits) = self.detect_references(user_text);
        if warm_hits.is_empty() && cold_hits.is_empty() {
            return None;
        }
        // Descending order: removing a lower index shifts the rest, so an
        // ascending walk would promote the wrong entry or miss the tail.
        // Capped so one turn can only pull back a bounded block; unpromoted
        // hits stay archived and are re-detected later.
        let mut entries: Vec<CompressedTurn> = Vec::new();
        for i in warm_hits.iter().rev() {
            if entries.len() >= MAX_PROMOTE_PER_CALL {
                break;
            }
            if let Some(e) = self.promote_warm(*i) {
                entries.push(e);
            }
        }
        for i in cold_hits.iter().rev() {
            if entries.len() >= MAX_PROMOTE_PER_CALL {
                break;
            }
            if let Some(e) = self.promote_cold(*i) {
                entries.push(e);
            }
        }
        if entries.is_empty() {
            return None;
        }
        tracing::info!(
            warm = warm_hits.len(),
            cold = cold_hits.len(),
            promoted = entries.len(),
            warm_entries = self.warm.len(),
            cold_entries = self.cold.len(),
            "promoted archived context into hot"
        );
        let mut body = String::new();
        for e in &entries {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&format_entry(e));
        }
        Some(format!(
            "{PROMOTE_MARKER}\n以下是先前归档的上下文摘要，供继续该任务时参考。\n\n{body}"
        ))
    }

    /// Estimate layer stats for the current session.
    pub fn estimate_stats(&self, messages: &[Message], context_limit: usize) -> LayerStats {
        let hot_tokens = tokens::estimate_messages(messages);
        let warm_tokens: usize = self
            .warm
            .iter()
            .map(|e| {
                tokens::estimate_text(&e.user_goal)
                    + e.tool_summaries
                        .iter()
                        .map(|s| tokens::estimate_text(s))
                        .sum::<usize>()
                    + e.decisions
                        .iter()
                        .map(|s| tokens::estimate_text(s))
                        .sum::<usize>()
            })
            .sum();
        let cold_tokens: usize = self
            .cold
            .iter()
            .map(|e| tokens::estimate_text(&e.goal) + tokens::estimate_text(&e.conclusion))
            .sum();

        LayerStats {
            hot_msgs: messages.len(),
            warm_entries: self.warm.len(),
            cold_entries: self.cold.len(),
            hot_tokens,
            warm_tokens,
            cold_tokens,
            total_tokens: hot_tokens + warm_tokens + cold_tokens,
            limit: context_limit,
        }
    }
}

impl Default for LayerManager {
    fn default() -> Self {
        Self::new(LayerConfig::default())
    }
}

/// Promote archived context referenced by the latest user message back into
/// the hot layer, merged as a prefix block on that message. No-op for
/// sessions without layering, without a user message, or already augmented.
pub fn promote_referenced_context(session: &mut Session) -> bool {
    let Some(lm) = session.layers.as_mut() else {
        return false;
    };
    let Some(idx) = session.messages.iter().rposition(|m| m.role == Role::User) else {
        return false;
    };
    if session.messages[idx].content.contains(PROMOTE_MARKER) {
        return false;
    }
    let Some(block) = lm.promote_referenced(session.messages[idx].content.text()) else {
        return false;
    };
    // Image parts are preserved; the block becomes the leading text part.
    session.messages[idx]
        .content
        .prepend_text(&format!("{block}\n\n"));
    true
}

/// One archived turn rendered as a compact block; empty sections omitted.
fn format_entry(e: &CompressedTurn) -> String {
    let mut s = format!("- 目标：{}", e.user_goal);
    if !e.decisions.is_empty() {
        let cleaned: Vec<&str> = e.decisions.iter().map(|d| clean_marker(d)).collect();
        s.push_str("\n- 决策：");
        s.push_str(&cleaned.join("；"));
    }
    if !e.tool_summaries.is_empty() {
        s.push_str("\n- 工具摘要：");
        s.push_str(&e.tool_summaries.join("；"));
    }
    if !e.files.is_empty() {
        s.push_str("\n- 相关文件：");
        s.push_str(&e.files.join("、"));
    }
    s
}

/// Strip the decision-list markers that `compress_to_warm` kept (`- ` / `* ` / `1. `).
fn clean_marker(s: &str) -> &str {
    let t = s.trim_start();
    for p in ["- ", "* ", "1. "] {
        if let Some(rest) = t.strip_prefix(p) {
            return rest;
        }
    }
    t
}

/// Extract keyword candidates from text (latin words ≥ 4 chars + CJK bigrams).
fn extract_keywords(text: &str, into: &mut Vec<String>) {
    // Latin words.
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        if word.len() >= 4 {
            let lower = word.to_lowercase();
            if !STOP_WORDS.contains(&lower.as_str()) {
                into.push(lower);
            }
        }
    }
    // CJK bigrams.
    let cjk_chars: Vec<char> = text.chars().filter(|c| super::tokens::is_cjk(*c)).collect();
    for window in cjk_chars.windows(2) {
        into.push(window.iter().collect());
    }
}

/// Marker merged into a user message after archived context was promoted.
/// Must not start with the condensed-summary prefix (context.rs matches it).
const PROMOTE_MARKER: &str = "[归档恢复]";
/// Max entries pulled back in one turn; extra hits stay archived.
const MAX_PROMOTE_PER_CALL: usize = 8;

const STOP_WORDS: &[&str] = &[
    "this", "that", "with", "from", "have", "been", "were", "they", "their", "about", "which",
    "would", "could", "should", "there", "when", "where", "into", "also", "then", "just", "like",
    "over", "than", "them", "some", "only", "other", "more", "very", "much", "such",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Role;

    fn make_msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.to_string().into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn compress_to_warm_extracts_goal_and_keywords() {
        let msgs = vec![
            make_msg(Role::User, "请帮我创建一个 Rust 项目结构"),
            make_msg(
                Role::Assistant,
                "- 创建 Cargo.toml\n- 添加依赖\nsrc/main.rs 已创建",
            ),
        ];
        let warm = LayerManager::compress_to_warm(&msgs);
        assert!(warm.user_goal.contains("Rust"));
        assert!(warm.decisions.len() >= 2);
        assert!(warm.files.iter().any(|f| f.contains("Cargo.toml")));
    }

    #[test]
    fn promote_cold_returns_entry() {
        let mut lm = LayerManager::default();
        lm.cold.push(ColdEntry {
            goal: "test goal".into(),
            conclusion: "done".into(),
            keywords: vec!["test".into(), "goal".into()],
        });
        let promoted = lm.promote_cold(0);
        assert!(promoted.is_some());
        assert_eq!(promoted.unwrap().user_goal, "test goal");
        assert!(lm.cold.is_empty());
    }

    #[test]
    fn detect_references_finds_keyword_matches() {
        let mut lm = LayerManager::default();
        lm.warm.push(CompressedTurn {
            user_goal: "创建 Rust 项目".into(),
            tool_summaries: vec![],
            decisions: vec![],
            files: vec!["Cargo.toml".into()],
            keywords: vec!["rust".into(), "cargo".into(), "项目".into()],
        });
        let (warm_hits, cold_hits) =
            lm.detect_references("请再修改一下那个 Rust 项目的 Cargo.toml");
        assert_eq!(warm_hits.len(), 1);
        assert_eq!(cold_hits.len(), 0);
    }

    #[test]
    fn push_warm_cascades_to_cold() {
        let mut lm = LayerManager::new(LayerConfig {
            max_warm: 2,
            max_cold: 5,
            ..Default::default()
        });
        for i in 0..4 {
            lm.push_warm(CompressedTurn {
                user_goal: format!("turn {i}"),
                tool_summaries: vec![],
                decisions: vec![],
                files: vec![],
                keywords: vec![],
            });
        }
        assert_eq!(lm.warm.len(), 2); // capped at max_warm
        assert!(lm.cold.len() >= 1); // overflow went to cold
    }

    #[test]
    fn layer_stats_computes_totals() {
        let lm = LayerManager::default();
        let msgs = vec![make_msg(Role::User, "hello")];
        let stats = lm.estimate_stats(&msgs, 64_000);
        assert_eq!(stats.hot_msgs, 1);
        assert_eq!(stats.warm_entries, 0);
        assert!(stats.total_tokens > 0);
    }

    #[test]
    fn empty_session_no_panic() {
        let mut lm = LayerManager::default();
        assert!(!lm.tick(&[]));
        let (w, c) = lm.detect_references("");
        assert!(w.is_empty());
        assert!(c.is_empty());
    }

    #[test]
    fn tick_below_limit_returns_false() {
        let mut lm = LayerManager::default();
        let msgs = vec![make_msg(Role::User, "short")];
        assert!(!lm.tick(&msgs));
    }

    #[test]
    fn push_removed_range_splits_at_user_boundaries() {
        let mut lm = LayerManager::default();
        let removed = vec![
            make_msg(Role::User, "turn one"),
            make_msg(Role::Assistant, "- decision a\nsrc/main.rs"),
            make_msg(Role::Tool, "tool one out"),
            make_msg(Role::User, "turn two"),
            make_msg(Role::Assistant, "done"),
        ];
        let created = lm.push_removed_range(&removed);
        assert_eq!(created, 2);
        assert_eq!(lm.warm.len(), 2);
        assert_eq!(lm.warm[0].user_goal, "turn one");
        assert_eq!(lm.warm[1].user_goal, "turn two");
        assert!(lm.warm[1].tool_summaries.is_empty());
    }

    #[test]
    fn push_removed_range_merges_head_and_tail_segments() {
        let mut lm = LayerManager::default();
        // Head (no user) merges into first turn; tail assistant stays in last.
        let removed = vec![
            make_msg(Role::Tool, "stray result"),
            make_msg(Role::User, "first ask"),
            make_msg(Role::Assistant, "- decided x"),
            make_msg(Role::Tool, "out one"),
            make_msg(Role::User, "second ask"),
            make_msg(Role::Assistant, "- final reply"),
        ];
        let created = lm.push_removed_range(&removed);
        assert_eq!(created, 2);
        assert_eq!(lm.warm[0].user_goal, "first ask");
        assert!(
            lm.warm[0]
                .tool_summaries
                .iter()
                .any(|s| s.contains("stray"))
        );
        assert_eq!(lm.warm[1].user_goal, "second ask");
        assert!(lm.warm[1].decisions.iter().any(|d| d.contains("final")));
    }

    #[test]
    fn push_removed_range_ignores_non_user_slices() {
        let mut lm = LayerManager::default();
        let only_tools = vec![make_msg(Role::Tool, "a"), make_msg(Role::Assistant, "b")];
        assert_eq!(lm.push_removed_range(&only_tools), 0);
        assert!(lm.warm.is_empty());
        assert_eq!(lm.push_removed_range(&[]), 0);
        assert!(lm.warm.is_empty());
    }

    #[test]
    fn push_removed_range_disabled_config_returns_zero() {
        let mut lm = LayerManager::new(LayerConfig {
            enabled: false,
            ..Default::default()
        });
        let removed = vec![make_msg(Role::User, "u"), make_msg(Role::Assistant, "a")];
        assert_eq!(lm.push_removed_range(&removed), 0);
        assert!(lm.warm.is_empty());
    }

    #[test]
    fn push_removed_range_cascades_to_cold_on_overflow() {
        let mut lm = LayerManager::new(LayerConfig {
            max_warm: 1,
            max_cold: 5,
            ..Default::default()
        });
        let mut removed = Vec::new();
        for i in 0..3 {
            removed.push(make_msg(Role::User, &format!("turn {i}")));
            removed.push(make_msg(Role::Assistant, &format!("- answer {i}")));
        }
        assert_eq!(lm.push_removed_range(&removed), 3);
        assert_eq!(lm.warm.len(), 1); // capped at max_warm
        assert_eq!(lm.cold.len(), 2); // overflow went to cold
    }

    fn warm_with_keywords(goal: &str, keywords: &[&str]) -> CompressedTurn {
        CompressedTurn {
            user_goal: goal.into(),
            tool_summaries: vec![],
            decisions: vec!["- 用 cargo 搭建".into()],
            files: vec![],
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn promote_referenced_pulls_warm_entry() {
        let mut lm = LayerManager::default();
        lm.push_warm(CompressedTurn {
            user_goal: "创建 Rust 项目".into(),
            tool_summaries: vec!["cargo init".into()],
            decisions: vec!["- 用 cargo".into()],
            files: vec![],
            keywords: vec!["rust".into(), "cargo".into()],
        });
        let block = lm
            .promote_referenced("再改一下那个 rust 项目的 cargo 配置")
            .unwrap();
        assert!(block.starts_with(PROMOTE_MARKER));
        assert!(block.contains("创建 Rust 项目"));
        assert!(block.contains("用 cargo")); // decision marker stripped
        assert!(block.contains("cargo init"));
        assert!(lm.warm.is_empty());
    }

    #[test]
    fn promote_referenced_pulls_cold_entry() {
        let mut lm = LayerManager::default();
        lm.cold.push(ColdEntry {
            goal: "迁移数据库".into(),
            conclusion: "改用 SurrealDB".into(),
            keywords: vec!["migration".into(), "database".into()],
        });
        let block = lm
            .promote_referenced("migration database 的事继续")
            .unwrap();
        assert!(block.contains("迁移数据库"));
        assert!(block.contains("改用 SurrealDB"));
        assert!(lm.cold.is_empty());
    }

    #[test]
    fn promote_referenced_combines_warm_and_cold() {
        let mut lm = LayerManager::default();
        lm.push_warm(warm_with_keywords("warm goal", &["alpha", "beta"]));
        lm.cold.push(ColdEntry {
            goal: "cold goal".into(),
            conclusion: "cold done".into(),
            keywords: vec!["gamma".into(), "delta".into()],
        });
        let block = lm.promote_referenced("alpha beta gamma delta").unwrap();
        assert!(block.contains("warm goal"));
        assert!(block.contains("cold goal"));
        assert!(lm.warm.is_empty() && lm.cold.is_empty());
    }

    #[test]
    fn promote_referenced_noop_without_hits() {
        let mut lm = LayerManager::default();
        lm.push_warm(warm_with_keywords("g", &["rust", "cargo"]));
        assert!(lm.promote_referenced("完全不相关的话题").is_none());
        assert_eq!(lm.warm.len(), 1);
        assert!(lm.promote_referenced("   ").is_none());
        assert_eq!(lm.warm.len(), 1);

        let mut disabled = LayerManager::new(LayerConfig {
            enabled: false,
            ..Default::default()
        });
        disabled.push_warm(warm_with_keywords("g", &["rust", "cargo"]));
        assert!(disabled.promote_referenced("rust cargo").is_none());

        assert!(
            LayerManager::default()
                .promote_referenced("rust cargo")
                .is_none()
        );
    }

    #[test]
    fn promote_referenced_removes_indices_descending() {
        let mut lm = LayerManager::default();
        lm.push_warm(warm_with_keywords("turn a", &["alpha", "beta"]));
        lm.push_warm(warm_with_keywords("turn b", &["middle", "unused"]));
        lm.push_warm(warm_with_keywords("turn c", &["gamma", "delta"]));
        let block = lm.promote_referenced("alpha beta gamma delta").unwrap();
        assert!(block.contains("turn a"));
        assert!(block.contains("turn c"));
        assert!(!block.contains("turn b"));
        assert_eq!(lm.warm.len(), 1);
        assert_eq!(lm.warm[0].user_goal, "turn b");
    }

    #[test]
    fn promote_referenced_caps_per_call_and_retries() {
        let mut lm = LayerManager::default();
        for i in 0..10 {
            lm.push_warm(warm_with_keywords(
                &format!("goal {i}"),
                &["kwone", "kwtwo"],
            ));
        }
        let block = lm.promote_referenced("kwone kwtwo 提及").unwrap();
        assert_eq!(lm.warm.len(), 2); // 8 promoted, cap reached
        // Newest hits go first (descending index walk) — goal 9 is in.
        assert!(block.contains("goal 9"));
        assert!(!block.contains("goal 0"));
        // The remaining hits are still archived and re-detectable.
        let block2 = lm.promote_referenced("kwone kwtwo").unwrap();
        assert!(block2.contains("goal 1"));
        assert!(lm.warm.is_empty());
    }

    #[test]
    fn promote_referenced_context_merges_into_latest_user() {
        let mut session = Session::new("system");
        session
            .layers
            .as_mut()
            .unwrap()
            .push_warm(warm_with_keywords("旧任务", &["rust", "cargo"]));
        session.add_user_message("继续那个 rust cargo 任务");
        let original = session.messages.last().unwrap().content.text().to_string();
        assert!(promote_referenced_context(&mut session));
        let now = session.messages.last().unwrap().content.clone();
        assert!(now.text().starts_with(PROMOTE_MARKER));
        assert!(now.text().ends_with(&original));
        assert!(session.layers.as_ref().unwrap().warm.is_empty());
        // Same (now augmented) message is not promoted twice.
        assert!(!promote_referenced_context(&mut session));
        assert_eq!(session.messages.last().unwrap().content.text(), now.text());
    }

    #[test]
    fn promote_referenced_context_noop_without_user_message() {
        let mut session = Session::new("system");
        session.add_assistant_message("hi");
        assert!(!promote_referenced_context(&mut session));
        assert_eq!(session.messages.len(), 2);
    }

    #[test]
    fn promote_referenced_keeps_tool_pairing_intact() {
        let mut session = Session::new("system");
        session
            .layers
            .as_mut()
            .unwrap()
            .push_warm(warm_with_keywords("x", &["rust", "cargo"]));
        session.add_user_message("rust cargo 继续");
        session.add_assistant_tool_calls(
            String::new(),
            vec![crate::session::ToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: crate::session::FunctionCall {
                    name: "list_directory".into(),
                    arguments: "{}".into(),
                },
            }],
        );
        session.add_tool_result("c1".into(), "ok");
        assert!(promote_referenced_context(&mut session));
        crate::agent::context::repair_message_sequence(&mut session.messages);
        let tools: Vec<_> = session
            .messages
            .iter()
            .filter(|m| m.role == Role::Tool)
            .collect();
        assert_eq!(tools.len(), 1);
        assert!(session.messages.iter().any(|m| {
            m.role == Role::Assistant && m.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
        }));
    }
}
