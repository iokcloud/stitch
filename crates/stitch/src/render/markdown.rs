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
                code_buf.push_str(strip_control(text).as_ref());
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
                write!(out, "{}", strip_control(text))?;
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
                write!(out, "`{}`", strip_control(&code))?;
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
                write!(out, "[img: {}]", strip_control(&dest_url))?;
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
                write!(out, "{}", strip_control(text))?;
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
                .map_err(|e| io::Error::other(e.to_string()))?;
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
                    .is_some_and(|bg| bg.r < 80 && bg.g < 80 && bg.b < 80)
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

/// 全局语法集 / 主题（惰性加载一次，'static 供流式高亮器跨行持有引用）。
fn global_syntax_set() -> &'static SyntaxSet {
    static SS: std::sync::OnceLock<SyntaxSet> = std::sync::OnceLock::new();
    SS.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn global_theme() -> &'static syntect::highlighting::Theme {
    static TS: std::sync::OnceLock<syntect::highlighting::ThemeSet> = std::sync::OnceLock::new();
    TS.get_or_init(ThemeSet::load_defaults)
        .themes
        .get("base16-ocean.dark")
        .unwrap_or_else(|| {
            TS.get_or_init(ThemeSet::load_defaults)
                .themes
                .values()
                .next()
                .expect("ThemeSet has no themes")
        })
}

/// 剥离终端控制字符：移除全部 C0 控制符（保留 \t \n \r），阻断
/// ANSI/OSC/DCS 转义注入（一切以 ESC 开头的序列）。正常内容
/// （无控制字符）零拷贝借用，流式热路径无额外开销。
pub(crate) fn strip_control(s: &str) -> std::borrow::Cow<'_, str> {
    if !s
        .chars()
        .any(|c| c < ' ' && c != '\t' && c != '\n' && c != '\r')
    {
        return std::borrow::Cow::Borrowed(s);
    }
    std::borrow::Cow::Owned(
        s.chars()
            .filter(|&c| c >= ' ' || c == '\t' || c == '\n' || c == '\r')
            .collect(),
    )
}

/// 围栏语言名消毒：只保留 ASCII 字母数字与 `-` `_` `+` `#`，其余全部
/// 剥离（含 ANSI 转义）。syntect 合法语言名（Rust/C++/C#/Objective-C/
/// x86_64 等）全由此类字符构成；消毒后为空时退化为无高亮，不影响围栏
/// 判定与边框渲染。
fn sanitize_lang(lang: &str) -> String {
    lang.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '#'))
        .collect()
}

/// 标题行判定：# 开头（1-6 个）且后随空格（`#1 问题` 不算标题）。
fn is_heading_line(line: &str) -> bool {
    let t = line.trim_start();
    let hash_count = t.chars().take_while(|&c| c == '#').count();
    (1..=6).contains(&hash_count) && t.chars().nth(hash_count) == Some(' ')
}

/// 疑似围栏行（```/~~~ 开头）或标题行 → 延迟到行完整再渲染。
fn looks_like_fence_or_heading(s: &str) -> bool {
    fence_info(s).is_some() || s.trim_start().starts_with(['`', '~']) || is_heading_line(s)
}

/// 判定整行是否为围栏（``` 或 ~~~，≥3 个），返回围栏后的语言名。
fn fence_info(line: &str) -> Option<String> {
    let t = line.trim();
    let marker = if t.starts_with("```") {
        Some("```")
    } else if t.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    };
    let marker = marker?;
    let rest = t[marker.len()..].trim();
    if rest.len() >= 3 {
        // 连续围栏符开头（如 ````rust）——仍按围栏处理，语言取剥完 marker 后的部分
        return Some(rest.trim_start_matches(['`', '~']).trim().to_string());
    }
    Some(rest.to_string())
}

/// Streaming token buffer — reduces terminal flicker by batching rapid tokens.
///
/// 增量 markdown 渲染（交互模式流式路径）：普通文本逐 token 直出保住
/// 动态感；代码块维护跨 chunk 状态，整行渲染 ┌─/│/└─ 边框 + syntect
/// 高亮（HighlightLines 天然跨行，多行字符串不断裂）。
#[derive(Default)]
pub struct StreamBuf {
    buf: String,
    last_flush: Option<Instant>,
    /// 等待完整的行（代码块内行 / 疑似围栏行，跨 chunk 累积）。
    pending_line: String,
    /// 是否在代码块内（跨 chunk）。
    in_code: bool,
    /// 当前代码块语言（顶边框显示）。
    code_lang: String,
    /// 当前代码块行高亮器（跨行状态，退出代码块时复位）。
    highlighter: Option<HighlightLines<'static>>,
}

impl StreamBuf {
    const FLUSH_INTERVAL_MS: u64 = 8;
    const FLUSH_BYTE_THRESHOLD: usize = 32;

    /// Push a token; flushes if the buffer is full or the interval has elapsed.
    pub fn push(&mut self, token: &str) -> io::Result<()> {
        self.buf.push_str(&strip_control(token));

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
        self.flush_now()?;
        // 代码块未闭合（回合被中断等）——补底边框并复位状态
        if self.in_code {
            let mut out = stdout();
            queue!(out, SetForegroundColor(Color::DarkGrey))?;
            writeln!(out, "└─")?;
            queue!(out, ResetColor)?;
            out.flush()?;
            self.in_code = false;
            self.code_lang.clear();
            self.highlighter = None;
        }
        Ok(())
    }

    fn flush_now(&mut self) -> io::Result<()> {
        self.pending_line.push_str(&self.buf);
        self.buf.clear();

        let mut out = stdout();
        self.process_available_lines(&mut out)?;

        // 代码块内：剩余行尾等下一 chunk；代码块外：普通文本立即直出
        // （保持逐 token 动态感），疑似围栏行/标题行延迟到完整再判定
        if !self.in_code
            && !self.pending_line.is_empty()
            && !looks_like_fence_or_heading(&self.pending_line)
        {
            write!(out, "{}", self.pending_line)?;
            self.pending_line.clear();
        }

        out.flush()?;
        self.last_flush = Some(Instant::now());
        Ok(())
    }

    /// 渲染 pending_line 中所有完整行（含换行结尾）。
    fn process_available_lines(&mut self, out: &mut impl Write) -> io::Result<()> {
        while let Some(nl) = self.pending_line.find('\n') {
            let line = self.pending_line[..=nl].to_string();
            self.pending_line.drain(..=nl);
            self.render_line(out, &line)?;
        }
        Ok(())
    }

    /// 渲染一行（含结尾换行）。
    fn render_line(&mut self, out: &mut impl Write, line: &str) -> io::Result<()> {
        if let Some(lang) = fence_info(line) {
            if self.in_code {
                // 围栏闭合：底边框
                queue!(out, SetForegroundColor(Color::DarkGrey))?;
                writeln!(out, "└─")?;
                queue!(out, ResetColor)?;
                self.in_code = false;
                self.code_lang.clear();
                self.highlighter = None;
            } else {
                // 围栏开启：顶边框 + 语言（先消毒，防 ANSI 转义注入）
                let lang = sanitize_lang(&lang);
                self.in_code = true;
                self.code_lang.clone_from(&lang);
                queue!(out, SetForegroundColor(Color::DarkGrey))?;
                writeln!(out, "┌─ {lang}")?;
                queue!(out, ResetColor)?;
                self.highlighter = find_syntax(global_syntax_set(), &lang)
                    .map(|s| HighlightLines::new(s, global_theme()));
            }
            return Ok(());
        }

        if self.in_code {
            // 代码行：│ 前缀 + 语法高亮（跨行状态保留）
            queue!(out, SetForegroundColor(Color::DarkGrey))?;
            write!(out, "│ ")?;
            queue!(out, ResetColor)?;
            if let Some(h) = &mut self.highlighter {
                let ranges = h
                    .highlight_line(line, global_syntax_set())
                    .map_err(|e| io::Error::other(e.to_string()))?;
                let escaped = syntect::util::as_24_bit_terminal_escaped(&ranges, false);
                write!(out, "{escaped}")?;
            } else {
                queue!(out, SetForegroundColor(Color::Grey))?;
                write!(out, "{line}")?;
                queue!(out, ResetColor)?;
            }
            return Ok(());
        }

        // 普通文本：标题加粗 + 行内 code/加粗（原样直出保动态感）
        write!(out, "{}", style_plain_line(line))?;
        Ok(())
    }
}

/// 非代码行样式：标题行加粗亮白；行内 `code` 青色、`**加粗**` 加粗。
/// 配对不完整（跨 chunk 截断）时原样输出，绝不丢字符。
fn style_plain_line(line: &str) -> std::borrow::Cow<'_, str> {
    if is_heading_line(line) {
        return std::borrow::Cow::Owned(format!("\x1b[1;97m{line}\x1b[0m"));
    }
    if line.contains('`') || line.contains("**") {
        return std::borrow::Cow::Owned(highlight_inline(line));
    }
    std::borrow::Cow::Borrowed(line)
}

/// 行内扫描：`code`（反引号配对）青色、`**bold**` 加粗，其余原样。
fn highlight_inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 32);
    let mut rest = line;
    loop {
        let nb = rest.find('`');
        let nd = rest.find("**");
        let (pos, kind) = match (nb, nd) {
            (Some(b), Some(d)) if b < d => (b, 'c'),
            (Some(_), Some(d)) => (d, 'b'),
            (Some(b), None) => (b, 'c'),
            (None, Some(d)) => (d, 'b'),
            (None, None) => {
                out.push_str(rest);
                break;
            }
        };
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        match kind {
            'c' => {
                // 配对反引号：rest[1..] 内相对偏移 d，绝对位置 d+1
                if let Some(d) = rest[1..].find('`') {
                    let pair_pos = d + 1;
                    // 含首尾反引号一起上色，反引号保留
                    out.push_str(&format!("\x1b[36m{}\x1b[0m", &rest[0..=pair_pos]));
                    rest = &rest[pair_pos + 1..];
                } else {
                    out.push('`');
                    rest = &rest[1..];
                }
            }
            _ => {
                if let Some(end) = rest[2..].find("**") {
                    out.push_str(&format!("\x1b[1m{}\x1b[0m", &rest[2..end + 2]));
                    rest = &rest[end + 4..];
                } else {
                    out.push_str("**");
                    rest = &rest[2..];
                }
            }
        }
    }
    out
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

    /// 剥离 ANSI CSI 转义（\x1b[...m / \x1b[38;2;r;g;bm），断言用。
    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_esc = false;
        for c in s.chars() {
            if in_esc {
                if c == 'm' {
                    in_esc = false;
                }
            } else if c == '\x1b' {
                in_esc = true;
            } else {
                out.push(c);
            }
        }
        out
    }

    /// 流式增量渲染：围栏开 → 代码行（│ 前缀 + 内容）→ 围栏关（└─）。
    #[test]
    fn stream_fence_open_code_close() {
        let mut buf = StreamBuf::default();
        let mut out = Vec::new();

        buf.render_line(&mut out, "```rust\n").unwrap();
        assert!(buf.in_code);
        assert!(buf.code_lang == "rust");
        let s = String::from_utf8(out.clone()).unwrap();
        assert!(s.contains("┌─ rust"), "顶边框应带语言: {s}");
        out.clear();

        buf.render_line(&mut out, "fn main() {}\n").unwrap();
        let s = String::from_utf8(out.clone()).unwrap();
        assert!(s.contains("│ "), "代码行应有 │ 前缀: {s}");
        let plain = strip_ansi(&s);
        assert!(plain.contains("fn main() {}"), "代码内容保留: {plain}");
        out.clear();

        buf.render_line(&mut out, "```\n").unwrap();
        assert!(!buf.in_code, "围栏闭合后退出代码块");
        let s = String::from_utf8(out.clone()).unwrap();
        assert!(s.contains("└─"), "应有底边框: {s}");
    }

    /// 普通文本直出（无样式前缀），保持逐 token 动态感。
    #[test]
    fn stream_plain_text_passes_through() {
        let mut buf = StreamBuf::default();
        let mut out = Vec::new();
        buf.render_line(&mut out, "你好，这是一段普通文本\n")
            .unwrap();
        let s = String::from_utf8(out.clone()).unwrap();
        assert_eq!(s, "你好，这是一段普通文本\n");
    }

    /// 行内样式：`code` 青色、**加粗** 加粗、标题行加粗亮白。
    #[test]
    fn inline_code_bold_and_heading_styled() {
        assert!(style_plain_line("- 使用 `fs::read_to_string` 一步完成\n").contains("\x1b[36m"));
        assert!(style_plain_line("**重要**：勿删\n").contains("\x1b[1m"));
        assert!(style_plain_line("## 读取文件\n").contains("\x1b[1;97m"));
        // 剥离 ANSI 后内容一字不丢
        assert_eq!(
            strip_ansi(&style_plain_line("- 使用 `fs::read_to_string` 一步完成\n")),
            "- 使用 `fs::read_to_string` 一步完成\n"
        );
        // 非标题（# 后无空格）与无标记行不加样式
        assert!(!style_plain_line("#1 问题优先\n").contains('\x1b'));
        assert!(!style_plain_line("普通行\n").contains('\x1b'));
        // 奇数反引号不 panic、原样
        assert_eq!(strip_ansi(&style_plain_line("a`b\n")), "a`b\n");
    }

    /// 围栏行跨 chunk 分片：第一个分片 pending 等待，拼完整后渲染顶边框。
    #[test]
    fn stream_fence_split_across_chunks() {
        let mut buf = StreamBuf::default();
        let mut out = Vec::new();

        buf.pending_line.push_str("```rus");
        buf.process_available_lines(&mut out).unwrap();
        // 无换行 → 未渲染，仍 pending
        assert!(out.is_empty(), "未完整行不得输出: {out:?}");
        assert_eq!(buf.pending_line, "```rus");

        buf.pending_line.push_str("t\n");
        buf.process_available_lines(&mut out).unwrap();
        let s = String::from_utf8(out.clone()).unwrap();
        assert!(s.contains("┌─ rust"), "分片拼合后识别语言: {s}");
        assert!(buf.in_code);
    }

    /// 未闭合代码块 finish 时补底边框并复位状态。
    #[test]
    fn stream_unclosed_fence_finish_closes() {
        let mut buf = StreamBuf::default();
        let mut out = Vec::new();
        buf.render_line(&mut out, "```python\n").unwrap();
        out.clear();
        // finish 直接写 stdout——只验证状态复位
        buf.finish().unwrap();
        assert!(!buf.in_code);
        assert!(buf.highlighter.is_none());
    }

    /// 转义注入防护：语言名消毒 + 内容控制字符剥离（安全属性断言——
    /// 输出中不可能出现 ESC 开头的注入序列）。
    #[test]
    fn escape_injection_is_stripped() {
        // 围栏语言名：注入载荷中的 ESC 全部剔除，合法名不受影响
        let clean = sanitize_lang("\x1b]0;http://evil\x07\x1b[31mrust");
        assert!(!clean.contains('\x1b'), "语言名不得含 ESC: {clean:?}");
        assert!(clean.contains("rust"));
        assert_eq!(sanitize_lang("c++"), "c++");
        assert_eq!(sanitize_lang("objective-c"), "objective-c");
        assert_eq!(sanitize_lang("x86_64"), "x86_64");

        // 代码块顶边框：模型侧注入语言名 → 消毒后渲染，无 OSC 序列
        let mut buf = StreamBuf::default();
        let mut out = Vec::new();
        buf.render_line(&mut out, "```\x1b[31mrust\x1b[0m\n")
            .unwrap();
        let s = String::from_utf8(out.clone()).unwrap();
        assert!(!s.contains("\x1b]"), "顶边框不得含 OSC 注入: {s}");
        assert!(strip_ansi(&s).contains("┌─ "), "顶边框形态: {s}");

        // 内容控制字符剥离（\t\n\r 保留），ESC 是一切转义序列的前缀
        assert_eq!(strip_control("a\x1b[2Jb\n\tc"), "a[2Jb\n\tc");
        assert!(!strip_control("ok\x1b]0;x\x07").contains('\x1b'));

        // push 路径的过滤 = strip_control（唯一入口；push 的直出目标是
        // 真实 stdout，此处置换验证即可覆盖）
        assert_eq!(strip_control("ok\x1b[?25l"), "ok[?25l");
        assert_eq!(strip_control("\x1b]0;x\x07ok"), "]0;xok");
    }
}
