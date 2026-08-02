//! Guards against stuck ReAct loops (duplicate tool calls, etc.).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const WINDOW: usize = 8;
/// If the same fingerprint hasn't been seen for this long, treat it as fresh.
const DUPLICATE_STALE_DURATION: Duration = Duration::from_secs(15);

/// Rolling fingerprints of recent tool calls (name + normalized args) with timestamps.
#[derive(Debug, Clone)]
struct Entry {
    fp: String,
    at: Instant,
}

#[derive(Debug, Default, Clone)]
pub struct ToolCallGuard {
    recent: VecDeque<Entry>,
    /// How many consecutive blocked duplicates in a row (reset on allow).
    consecutive_blocks: usize,
}

impl ToolCallGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fingerprint a tool call for duplicate detection.
    pub fn fingerprint(name: &str, arguments: &str) -> String {
        format!("{name}\0{}", normalize_args(arguments))
    }

    /// Returns `true` if this call should be blocked (not executed again).
    pub fn should_block(&mut self, name: &str, arguments: &str) -> bool {
        let now = Instant::now();
        let fp = Self::fingerprint(name, arguments);

        // Purge stale entries older than DUPLICATE_STALE_DURATION
        while self
            .recent
            .front()
            .is_some_and(|e| now - e.at > DUPLICATE_STALE_DURATION)
        {
            self.recent.pop_front();
        }

        let consecutive = self
            .recent
            .back()
            .is_some_and(|e| e.fp == fp && now - e.at <= DUPLICATE_STALE_DURATION);
        let prior = self.recent.iter().filter(|e| e.fp == fp).count();
        let block = consecutive || prior >= 2;

        if block {
            self.consecutive_blocks = self.consecutive_blocks.saturating_add(1);
        } else {
            self.consecutive_blocks = 0;
            self.recent.push_back(Entry { fp, at: now });
            if self.recent.len() > WINDOW {
                self.recent.pop_front();
            }
        }
        block
    }

    /// After several blocked duplicates, force the model to stop tooling.
    pub fn should_force_final(&self) -> bool {
        self.consecutive_blocks >= 2
    }

    pub fn blocked_result(name: &str) -> serde_json::Value {
        serde_json::json!({
            "success": false,
            "output": format!(
                "[Agent hint] Duplicate identical `{name}` call with the same arguments was blocked. \
                 Do not repeat it. Change path/args, use a different tool, or write the final answer now."
            )
        })
    }
}

fn normalize_args(arguments: &str) -> String {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(serde_json::Value::Object(map)) => {
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for k in keys {
                if let Some(v) = map.get(&k) {
                    out.insert(k, v.clone());
                }
            }
            serde_json::Value::Object(out).to_string()
        }
        Ok(v) => v.to_string(),
        Err(_) => trimmed.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::ToolCallGuard;

    #[test]
    fn allows_first_then_blocks_consecutive_duplicate() {
        let mut g = ToolCallGuard::new();
        assert!(!g.should_block("read_file", r#"{"path":"a.rs"}"#));
        assert!(g.should_block("read_file", r#"{"path":"a.rs"}"#));
        assert!(!g.should_force_final());
        assert!(g.should_block("read_file", r#"{"path":"a.rs"}"#));
        assert!(g.should_force_final());
    }

    #[test]
    fn allows_different_args() {
        let mut g = ToolCallGuard::new();
        assert!(!g.should_block("read_file", r#"{"path":"a.rs"}"#));
        assert!(!g.should_block("read_file", r#"{"path":"b.rs"}"#));
    }

    #[test]
    fn normalizes_key_order() {
        let mut g = ToolCallGuard::new();
        assert!(!g.should_block("git_diff", r#"{"staged":false,"path":"x"}"#));
        assert!(g.should_block("git_diff", r#"{"path":"x","staged":false}"#));
    }

    #[test]
    fn blocks_third_occurrence_in_window() {
        let mut g = ToolCallGuard::new();
        assert!(!g.should_block("git_status", "{}"));
        assert!(!g.should_block("list_directory", r#"{"path":"."}"#));
        // second identical after interruption: prior=1, not consecutive -> still allow once?
        // prior >= 2 means third time. Second time with gap:
        assert!(!g.should_block("git_status", "{}")); // prior=1, consec=false -> allow
        assert!(g.should_block("git_status", "{}")); // prior=2 -> block
    }
}
