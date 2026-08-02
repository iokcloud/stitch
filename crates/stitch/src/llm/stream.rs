//! SSE stream parsing and tool call accumulation.
//!
//! OpenAI streams tool call arguments in chunks, so we need to
//! accumulate them before emitting a complete ToolCall event.
//!
//! UTF-8 framing: never decode network chunks with `from_utf8_lossy` in
//! isolation — a multi-byte character split across TCP chunks becomes
//! U+FFFD (���). Frame on raw bytes until ASCII `\n` (cannot appear inside
//! a multi-byte UTF-8 sequence), then decode. Same invariant as browser
//! `TextDecoder({ stream: true })` and production SSE clients that buffer
//! bytes before line split.

use super::FunctionDelta;

/// Soft cap on unfinished SSE line bytes (malformed / adversarial streams).
const SSE_LINE_BUF_MAX: usize = 1_048_576;

/// Accumulate raw SSE bytes and emit complete lines only after `\n`.
///
/// Holding incomplete UTF-8 in the byte buffer across chunks prevents the
/// classic mid-character `from_utf8_lossy` corruption.
#[derive(Debug, Default)]
pub struct SseLineBuffer {
    buf: Vec<u8>,
}

impl SseLineBuffer {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(4096),
        }
    }

    /// Append a network chunk. Returns `Err` if the unfinished line exceeds
    /// [`SSE_LINE_BUF_MAX`] (caller should abort the stream).
    pub fn push(&mut self, chunk: &[u8]) -> Result<(), String> {
        if self.buf.len().saturating_add(chunk.len()) > SSE_LINE_BUF_MAX {
            return Err(format!(
                "SSE line buffer exceeded {SSE_LINE_BUF_MAX} bytes (no newline)"
            ));
        }
        self.buf.extend_from_slice(chunk);
        Ok(())
    }

    /// Drain every complete line (excluding the terminating `\n`).
    /// Partial trailing bytes (including a split UTF-8 sequence) stay buffered.
    pub fn drain_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line_bytes: Vec<u8> = self.buf.drain(..=pos).collect();
            line_bytes.pop(); // drop `\n`
            if line_bytes.last() == Some(&b'\r') {
                line_bytes.pop();
            }
            lines.push(decode_sse_line(&line_bytes));
        }
        lines
    }

    /// Flush any bytes left after the stream ends (normally empty).
    pub fn flush_remainder(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        let rest = std::mem::take(&mut self.buf);
        Some(decode_sse_line(&rest))
    }

    #[cfg(test)]
    fn pending_len(&self) -> usize {
        self.buf.len()
    }
}

/// Decode one finished SSE line. Genuine invalid UTF-8 (not chunk splits)
/// still becomes U+FFFD so a bad provider cannot abort the whole stream.
fn decode_sse_line(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// A tool call being accumulated from streaming deltas.
#[derive(Debug, Clone)]
pub struct PendingToolCall {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: String,
}

impl PendingToolCall {
    /// Try to finalize this pending tool call into a complete one.
    pub fn finalize(self) -> Option<CompletedToolCall> {
        match (self.id, self.name) {
            (Some(id), Some(name)) => Some(CompletedToolCall {
                id,
                name,
                arguments: self.arguments,
            }),
            _ => None,
        }
    }
}

/// A fully resolved tool call.
#[derive(Debug, Clone)]
pub struct CompletedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Accumulate a tool call delta into the pending list.
///
/// OpenAI sends tool calls with an `index` to distinguish multiple
/// parallel calls. Each delta may contain:
/// - `id`: the tool call ID (only in the first delta)
/// - `function.name`: the function name (only in the first delta)
/// - `function.arguments`: JSON fragment (may span multiple deltas)
pub fn accumulate_tool_call(
    acc: &mut Vec<PendingToolCall>,
    index: usize,
    id: Option<String>,
    function: Option<FunctionDelta>,
) {
    // Ensure we have an entry for this index
    while acc.len() <= index {
        acc.push(PendingToolCall {
            index: acc.len(),
            id: None,
            name: None,
            arguments: String::new(),
        });
    }

    let pending = &mut acc[index];

    if let Some(id) = id {
        pending.id = Some(id);
    }

    if let Some(func) = function {
        if let Some(name) = func.name {
            pending.name = Some(name);
        }
        if let Some(args) = func.arguments {
            pending.arguments.push_str(&args);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: 3-byte Chinese split after 1st byte → was `���` with per-chunk lossy.
    #[test]
    fn sse_line_buffer_preserves_chinese_split_mid_char() {
        // 「新」= E6 96 B0 — same split that produced three U+FFFD in production.
        let mut line = Vec::new();
        line.extend_from_slice(br#"data: {"choices":[{"delta":{"content":""#);
        line.extend_from_slice("新文件".as_bytes());
        line.extend_from_slice(br#""}}]}"#);
        line.push(b'\n');

        let xin = "新".as_bytes();
        let xin_at = line
            .windows(xin.len())
            .position(|w| w == xin)
            .expect("新 in line");
        let split_at = xin_at + 1; // after first byte of 新

        let mut buf = SseLineBuffer::new();
        buf.push(&line[..split_at]).unwrap();
        assert!(buf.drain_lines().is_empty());
        assert!(buf.pending_len() > 0);

        buf.push(&line[split_at..]).unwrap();
        let lines = buf.drain_lines();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("新文件"),
            "expected intact 新文件, got {:?}",
            lines[0]
        );
        assert!(
            !lines[0].contains('\u{FFFD}'),
            "must not insert replacement chars: {:?}",
            lines[0]
        );
    }

    #[test]
    fn sse_line_buffer_preserves_4byte_emoji_split() {
        // 🎉 = F0 9F 8E 89
        let mut buf = SseLineBuffer::new();
        buf.push(b"data: party ").unwrap();
        buf.push(&[0xF0, 0x9F]).unwrap();
        assert!(buf.drain_lines().is_empty());
        buf.push(&[0x8E, 0x89]).unwrap();
        buf.push(b" time\n").unwrap();
        let lines = buf.drain_lines();
        assert_eq!(lines, vec!["data: party 🎉 time".to_string()]);
    }

    #[test]
    fn sse_line_buffer_handles_crlf_and_multiple_lines() {
        let mut buf = SseLineBuffer::new();
        buf.push(b"data: a\r\ndata: b\n").unwrap();
        let lines = buf.drain_lines();
        assert_eq!(lines, vec!["data: a".to_string(), "data: b".to_string()]);
    }

    #[test]
    fn sse_line_buffer_rejects_oversized_line() {
        let mut buf = SseLineBuffer::new();
        let chunk = vec![b'x'; SSE_LINE_BUF_MAX / 2 + 1];
        buf.push(&chunk).unwrap();
        let err = buf.push(&chunk).unwrap_err();
        assert!(err.contains("exceeded"));
    }
}
