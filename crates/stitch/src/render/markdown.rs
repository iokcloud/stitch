//! Markdown → Terminal renderer with syntax-highlighted code blocks.
//!
//! Uses pulldown-cmark 0.12 for parsing and syntect for code highlighting.
//! Outputs via crossterm for ANSI styling (bold, italic, colors).

use crossterm::{
    queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::io::{self, Write, stdout};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Render Markdown content to the terminal with full formatting.
pub fn render(content: &str) -> io::Result<()> {
    let mut out = stdout();
    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let parser = Parser::new_ext(content, Options::all());

    // State machine
    let mut first = true;
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();
    let mut in_heading = false;

    // Inline formatting stack (for nested bold/italic)
    let mut bold_depth: u32 = 0;
    let mut italic_depth: u32 = 0;

    // Link URL tracking (TagEnd::Link has no dest_url)
    let mut link_url: Option<String> = None;

    for event in parser {
        match event {
            // ── Code blocks ──────────────────────────────────
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    _ => String::new(),
                };
                code_buf.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                if !first {
                    writeln!(out)?;
                }
                first = false;
                render_code_block(&mut out, &code_buf, &code_lang, &ps, &ts)?;
                code_buf.clear();
                code_lang.clear();
            }
            Event::Text(ref text) if in_code_block => {
                code_buf.push_str(text);
            }

            // ── Headings ─────────────────────────────────────
            Event::Start(Tag::Heading { level, .. }) => {
                if !first {
                    writeln!(out)?;
                }
                first = false;
                in_heading = true;
                let prefix = "#".repeat(level as usize);
                queue!(out, SetAttribute(Attribute::Bold))?;
                let color = match level {
                    HeadingLevel::H1 => Color::Yellow,
                    HeadingLevel::H2 => Color::Cyan,
                    _ => Color::White,
                };
                queue!(out, SetForegroundColor(color))?;
                write!(out, "{prefix} ")?;
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                queue!(out, SetAttribute(Attribute::Reset))?;
                queue!(out, ResetColor)?;
                writeln!(out)?;
            }
            Event::Text(ref text) if in_heading => {
                write!(out, "{text}")?;
            }

            // ── Lists ────────────────────────────────────────
            Event::Start(Tag::List(..)) => {
                if !first {
                    writeln!(out)?;
                }
                first = false;
            }
            Event::End(TagEnd::List(..)) => {
                writeln!(out)?;
            }
            Event::Start(Tag::Item) => {
                write!(out, "  • ")?;
            }

            // ── Paragraphs ───────────────────────────────────
            Event::Start(Tag::Paragraph) => {
                if !first {
                    writeln!(out)?;
                }
                first = false;
            }
            Event::End(TagEnd::Paragraph) => {
                writeln!(out)?;
                // Reset inline styles
                if bold_depth > 0 || italic_depth > 0 {
                    queue!(out, SetAttribute(Attribute::Reset))?;
                    bold_depth = 0;
                    italic_depth = 0;
                }
            }

            // ── Inline formatting ────────────────────────────
            Event::Start(Tag::Emphasis) => {
                italic_depth += 1;
                queue!(out, SetAttribute(Attribute::Italic))?;
            }
            Event::End(TagEnd::Emphasis) => {
                italic_depth = italic_depth.saturating_sub(1);
                restore_inline_style(&mut out, bold_depth, italic_depth)?;
            }
            Event::Start(Tag::Strong) => {
                bold_depth += 1;
                queue!(out, SetAttribute(Attribute::Bold))?;
            }
            Event::End(TagEnd::Strong) => {
                bold_depth = bold_depth.saturating_sub(1);
                restore_inline_style(&mut out, bold_depth, italic_depth)?;
            }

            // ── Inline code ──────────────────────────────────
            Event::Code(code) => {
                queue!(out, SetForegroundColor(Color::DarkYellow))?;
                write!(out, "`{code}`")?;
                queue!(out, ResetColor)?;
            }

            // ── Breaks & rules ───────────────────────────────
            Event::SoftBreak => {
                write!(out, " ")?;
            }
            Event::HardBreak => {
                writeln!(out)?;
            }
            Event::Rule => {
                let (w, _) = crossterm::terminal::size().unwrap_or((80, 24));
                queue!(out, SetForegroundColor(Color::DarkGrey))?;
                writeln!(out, "{}", "─".repeat(w as usize))?;
                queue!(out, ResetColor)?;
            }

            // ── Blockquotes ──────────────────────────────────
            Event::Start(Tag::BlockQuote(_kind)) => {
                if !first {
                    writeln!(out)?;
                }
                first = false;
                queue!(out, SetForegroundColor(Color::DarkGrey))?;
                write!(out, "▌ ")?;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                queue!(out, ResetColor)?;
                writeln!(out)?;
            }

            // ── Links ────────────────────────────────────────
            Event::Start(Tag::Link { dest_url, .. }) => {
                queue!(out, SetForegroundColor(Color::Blue))?;
                queue!(out, SetAttribute(Attribute::Underlined))?;
                link_url = Some(dest_url.to_string());
                write!(out, "[")?;
            }
            Event::End(TagEnd::Link) => {
                let url = link_url.take().unwrap_or_default();
                write!(out, "]({url})")?;
                queue!(out, ResetColor)?;
                queue!(out, SetAttribute(Attribute::NoUnderline))?;
            }

            // ── Images (terminal: show alt text + url) ───────
            Event::Start(Tag::Image { dest_url, .. }) => {
                write!(out, "[img: {dest_url}]")?;
            }

            // ── Tables ───────────────────────────────────────
            Event::Start(Tag::Table(_)) => {
                if !first {
                    writeln!(out)?;
                }
                first = false;
            }
            Event::End(TagEnd::Table) => {
                writeln!(out)?;
            }
            Event::Start(Tag::TableHead) => {
                queue!(out, SetAttribute(Attribute::Bold))?;
            }
            Event::End(TagEnd::TableHead) => {
                queue!(out, SetAttribute(Attribute::Reset))?;
                writeln!(out)?;
            }
            Event::Start(Tag::TableRow) => {}
            Event::End(TagEnd::TableRow) => {
                writeln!(out)?;
            }
            Event::Start(Tag::TableCell) => {
                write!(out, " | ")?;
            }
            Event::End(TagEnd::TableCell) => {}

            // ── Strikethrough ────────────────────────────────
            Event::Start(Tag::Strikethrough) => {
                queue!(out, SetAttribute(Attribute::CrossedOut))?;
            }
            Event::End(TagEnd::Strikethrough) => {
                queue!(out, SetAttribute(Attribute::NotCrossedOut))?;
            }

            // ── Task list ────────────────────────────────────
            Event::TaskListMarker(checked) => {
                let mark = if checked { "[x]" } else { "[ ]" };
                let color = if checked {
                    Color::Green
                } else {
                    Color::DarkGrey
                };
                queue!(out, SetForegroundColor(color))?;
                write!(out, "{mark} ")?;
                queue!(out, ResetColor)?;
            }

            // ── HTML passthrough (ignore for terminal) ───────
            Event::Html(_) | Event::InlineHtml(_) => {}

            // Generic text (when formatting is active, just append)
            Event::Text(ref text) => {
                write!(out, "{text}")?;
            }
            _ => {}
        }
    }

    out.flush()?;
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────

fn restore_inline_style(out: &mut impl Write, bold: u32, italic: u32) -> io::Result<()> {
    if bold == 0 && italic == 0 {
        queue!(out, SetAttribute(Attribute::Reset))
    } else if bold == 0 {
        queue!(out, SetAttribute(Attribute::NormalIntensity))?;
        queue!(out, SetAttribute(Attribute::Italic))
    } else if italic == 0 {
        queue!(out, SetAttribute(Attribute::NoItalic))?;
        queue!(out, SetAttribute(Attribute::Bold))
    } else {
        queue!(out, SetAttribute(Attribute::Bold))?;
        queue!(out, SetAttribute(Attribute::Italic))
    }
}

// ── Code block rendering ────────────────────────────────────────

fn render_code_block(
    out: &mut impl Write,
    code: &str,
    lang: &str,
    ps: &SyntaxSet,
    ts: &ThemeSet,
) -> io::Result<()> {
    let display_lang = if lang.is_empty() { "code" } else { lang };
    let syntax = find_syntax(ps, lang);

    // Top border
    queue!(out, SetForegroundColor(Color::DarkGrey))?;
    writeln!(out, "┌─ {display_lang}")?;

    if let Some(syntax) = syntax {
        let theme = choose_dark_theme(ts);
        let mut h = HighlightLines::new(syntax, theme);

        for line in LinesWithEndings::from(code) {
            queue!(out, SetForegroundColor(Color::DarkGrey))?;
            write!(out, "│ ")?;
            queue!(out, ResetColor)?;

            let ranges = h
                .highlight_line(line, ps)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
            let escaped = syntect::util::as_24_bit_terminal_escaped(&ranges, false);
            write!(out, "{escaped}")?;
        }
    } else {
        for line in code.lines() {
            queue!(out, SetForegroundColor(Color::DarkGrey))?;
            write!(out, "│ ")?;
            queue!(out, SetForegroundColor(Color::Grey))?;
            writeln!(out, "{line}")?;
        }
        queue!(out, ResetColor)?;
    }

    // Bottom border
    queue!(out, SetForegroundColor(Color::DarkGrey))?;
    writeln!(out, "└─")?;
    queue!(out, ResetColor)?;

    Ok(())
}

fn choose_dark_theme(ts: &ThemeSet) -> &syntect::highlighting::Theme {
    ts.themes
        .get("base16-ocean.dark")
        .or_else(|| {
            ts.themes.values().find(|t| {
                t.settings
                    .background
                    .map_or(false, |bg| bg.r < 80 && bg.g < 80 && bg.b < 80)
            })
        })
        .unwrap_or_else(|| {
            // Any theme is better than crashing
            ts.themes.values().next().expect("ThemeSet has no themes")
        })
}

// ── Language detection ──────────────────────────────────────────

fn find_syntax<'a>(ps: &'a SyntaxSet, lang: &str) -> Option<&'a syntect::parsing::SyntaxReference> {
    if lang.is_empty() {
        return None;
    }

    if let Some(s) = ps.find_syntax_by_name(lang) {
        return Some(s);
    }
    // Case-insensitive fallback
    let lower = lang.to_lowercase();
    for s in ps.syntaxes() {
        if s.name.to_lowercase() == lower {
            return Some(s);
        }
    }

    for token in lang.split(&[',', '-', '_', ' ']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if let Some(s) = ps.find_syntax_by_token(token) {
            return Some(s);
        }
    }

    // Common aliases
    let alias = match lang.to_lowercase().as_str() {
        "js" | "javascript" => "JavaScript",
        "ts" | "typescript" => "TypeScript",
        "py" | "python" | "python3" => "Python",
        "rs" | "rust" => "Rust",
        "sh" | "bash" | "shell" | "zsh" => "Bash",
        "ps1" | "powershell" | "pwsh" => "PowerShell",
        "toml" => "TOML",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "md" | "markdown" => "Markdown",
        "html" => "HTML",
        "css" => "CSS",
        "scss" | "sass" => "SCSS",
        "sql" => "SQL",
        "go" | "golang" => "Go",
        "java" => "Java",
        "kt" | "kotlin" => "Kotlin",
        "swift" => "Swift",
        "c" => "C",
        "cpp" | "c++" | "cxx" => "C++",
        "cs" | "csharp" | "c#" => "C#",
        "rb" | "ruby" => "Ruby",
        "php" => "PHP",
        "lua" => "Lua",
        "r" => "R",
        "scala" => "Scala",
        "dart" => "Dart",
        "make" | "makefile" => "Makefile",
        "dockerfile" | "docker" => "Dockerfile",
        "vim" | "viml" => "VimL",
        "xml" => "XML",
        "ini" | "cfg" | "conf" => "INI",
        "diff" | "patch" => "Diff",
        "tex" | "latex" => "LaTeX",
        _ => return None,
    };
    ps.find_syntax_by_name(alias)
}

// ── Streaming helpers ───────────────────────────────────────────

use std::time::Instant;

/// Streaming token buffer — reduces terminal flicker by batching rapid tokens.
#[derive(Default)]
pub struct StreamBuf {
    buf: String,
    last_flush: Option<Instant>,
}

impl StreamBuf {
    const FLUSH_INTERVAL_MS: u64 = 8;
    const FLUSH_BYTE_THRESHOLD: usize = 32;

    /// Push a token; flushes if the buffer is full or the interval has elapsed.
    pub fn push(&mut self, token: &str) -> io::Result<()> {
        self.buf.push_str(token);

        let now = Instant::now();
        let should_flush = match self.last_flush {
            Some(last) => {
                now.duration_since(last).as_millis() >= Self::FLUSH_INTERVAL_MS as u128
                    || self.buf.len() >= Self::FLUSH_BYTE_THRESHOLD
            }
            None => true,
        };

        if should_flush {
            self.flush_now()?;
        }
        Ok(())
    }

    /// Flush remaining content (call at end of stream).
    pub fn finish(&mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            self.flush_now()?;
        }
        Ok(())
    }

    fn flush_now(&mut self) -> io::Result<()> {
        let mut out = stdout();
        write!(out, "{}", self.buf)?;
        out.flush()?;
        self.buf.clear();
        self.last_flush = Some(Instant::now());
        Ok(())
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_syntax_common_languages() {
        let ps = SyntaxSet::load_defaults_newlines();
        // Default embedded syntax set includes these core languages
        assert!(find_syntax(&ps, "Rust").is_some());
        assert!(find_syntax(&ps, "Python").is_some());
        assert!(find_syntax(&ps, "JavaScript").is_some());
        assert!(find_syntax(&ps, "C").is_some());
        assert!(find_syntax(&ps, "JSON").is_some());
        assert!(find_syntax(&ps, "HTML").is_some());
    }

    #[test]
    fn find_syntax_aliases() {
        let ps = SyntaxSet::load_defaults_newlines();
        assert!(find_syntax(&ps, "rs").is_some());
        assert!(find_syntax(&ps, "py").is_some());
        assert!(find_syntax(&ps, "js").is_some());
        assert!(find_syntax(&ps, "sh").is_some());
    }

    #[test]
    fn find_syntax_unknown_returns_none() {
        let ps = SyntaxSet::load_defaults_newlines();
        assert!(find_syntax(&ps, "madeupang").is_none());
        assert!(find_syntax(&ps, "").is_none());
    }

    #[test]
    fn stream_buf_flushes_on_threshold() {
        let mut buf = StreamBuf::default();
        let big = "x".repeat(StreamBuf::FLUSH_BYTE_THRESHOLD);
        buf.push(&big).unwrap();
        // Should have flushed — finish should be a no-op
        buf.finish().unwrap();
    }
}
