//! Terminal rendering layer.
//!
//! Two modes:
//! - `repl`: Default streaming Markdown renderer (pulldown-cmark + syntect)
//! - `tui`: Optional ratatui full-screen mode (behind `tui` feature)

pub mod dialog;
pub mod markdown;
#[cfg(feature = "tui")]
pub mod tui;

use markdown::StreamBuf;
use std::cell::RefCell;

thread_local! {
    static STREAM_BUF: RefCell<StreamBuf> = RefCell::new(StreamBuf::default());
}

/// Render a streaming token to the terminal.
///
/// Tokens are micro-buffered (~8ms) to reduce terminal I/O flicker.
/// Call `finish_stream()` at the end of a response to flush remaining content.
pub fn render_token(token: &str) {
    STREAM_BUF.with(|buf| {
        let _ = buf.borrow_mut().push(token);
    });
}

/// Flush any buffered streaming tokens and reset the buffer.
pub fn finish_stream() {
    STREAM_BUF.with(|buf| {
        let _ = buf.borrow_mut().finish();
    });
}

/// Render a completed message with full Markdown formatting.
///
/// Applies syntax highlighting to code blocks, formatting to headings,
/// lists, tables, and inline styles (bold, italic, code).
pub fn render_message(content: &str) {
    if content.is_empty() {
        return;
    }
    // Flush any streaming tokens first so output remains ordered.
    finish_stream();
    println!(); // Blank line before rendered message
    if let Err(e) = markdown::render(content) {
        // Fallback: plain text if rendering fails
        eprintln!("(render: {e})");
        println!("{content}");
    }
    println!(); // Spacing after
}

/// Render a tool execution status line.
pub fn render_tool_status(tool_name: &str, success: bool) {
    let icon = if success { "  ✅" } else { "  ❌" };
    eprintln!("{icon} {tool_name}");
}
