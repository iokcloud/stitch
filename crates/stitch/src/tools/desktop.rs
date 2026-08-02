//! Desktop automation tools — screenshot, click, type, window list.
//!
//! Let Stitch operate the Windows desktop like a real person:
//! see the screen, list windows, click, and type.
//!
//! All APIs are raw Win32 FFI — zero new crate dependencies.

use std::path::PathBuf; // 跨平台（struct 字段在非 Windows 也使用）

use super::{ToolDef, ToolResult};

// ── desktop_screenshot ──────────────────────────────────────────

/// Capture the full desktop and save as BMP.
#[derive(Clone)]
pub struct DesktopScreenshot {
    output_dir: PathBuf,
}

impl DesktopScreenshot {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "desktop_screenshot".into(),
            description: "Capture a screenshot of the entire desktop and save as a BMP file. \
                 Set ocr=true to extract readable text from the screenshot (useful for \
                 understanding browser/terminal content without seeing the image)."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "ocr": {
                        "type": "boolean",
                        "description": "If true, run OCR to extract visible text from the screenshot"
                    }
                },
                "required": []
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        #[cfg(windows)]
        {
            let ocr = arguments
                .get("ocr")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            screenshot_windows(&self.output_dir, ocr)
        }
        #[cfg(target_os = "linux")]
        {
            let ocr = arguments
                .get("ocr")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            screenshot_linux(&self.output_dir, ocr)
        }
        #[cfg(target_os = "macos")]
        {
            let ocr = arguments
                .get("ocr")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            screenshot_macos(&self.output_dir, ocr)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_screenshot is only supported on Windows/macOS/Linux".into(),
            })
        }
    }
}

/// 跨平台截图：Linux 用 `import`（ImageMagick）或 `gnome-screenshot`；macOS 用 `screencapture`。
/// 输出 PNG 到 output_dir，带时间戳文件名。
#[cfg(target_os = "linux")]
fn screenshot_linux(output_dir: &std::path::PathBuf, ocr: bool) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    std::fs::create_dir_all(output_dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = output_dir.join(format!("screenshot-{stamp}.png"));
    let result = Command::new("import")
        .arg("-window")
        .arg("root")
        .arg(&path)
        .output();
    let (success, err) = match result {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stderr).to_string(),
        ),
        Err(e) => (false, e.to_string()),
    };
    if !success {
        // 回退：gnome-screenshot
        let fb = Command::new("gnome-screenshot")
            .arg("-f")
            .arg(&path)
            .output();
        match fb {
            Ok(o) if o.status.success() => (),
            Ok(o) => {
                return Ok(ToolResult {
                    metrics: None,
                    success: false,
                    output: format!(
                        "截图失败（需安装 imagemagick 或 gnome-screenshot）：{}",
                        String::from_utf8_lossy(&o.stderr)
                    ),
                });
            }
            Err(e) => {
                return Ok(ToolResult {
                    metrics: None,
                    success: false,
                    output: format!("截图失败：{e}（需安装 imagemagick 或 gnome-screenshot）"),
                });
            }
        }
        let _ = err;
    }
    let text = if ocr {
        // OCR：优先 tesseract（中文需 chi_sim 语言包）
        let out = Command::new("tesseract")
            .arg(&path)
            .arg("stdout")
            .arg("-l")
            .arg("chi_sim+eng")
            .output();
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => String::new(),
        }
    } else {
        String::new()
    };
    let mut output = format!("截图已保存：{}", path.display());
    if !text.trim().is_empty() {
        output.push_str(&format!(
            "
屏幕文字（OCR）：
{text}"
        ));
    }
    Ok(ToolResult {
        metrics: None,
        success: true,
        output,
    })
}

#[cfg(target_os = "macos")]
fn screenshot_macos(output_dir: &std::path::PathBuf, ocr: bool) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    std::fs::create_dir_all(output_dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = output_dir.join(format!("screenshot-{stamp}.png"));
    let result = Command::new("screencapture").arg("-x").arg(&path).output();
    match result {
        Ok(o) if o.status.success() => (),
        Ok(o) => {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!("截图失败：{}", String::from_utf8_lossy(&o.stderr)),
            });
        }
        Err(e) => {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!("截图失败：{e}（screencapture 不可用）"),
            });
        }
    }
    let text = if ocr {
        let out = Command::new("tesseract")
            .arg(&path)
            .arg("stdout")
            .arg("-l")
            .arg("chi_sim+eng")
            .output();
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => String::new(),
        }
    } else {
        String::new()
    };
    let mut output = format!("截图已保存：{}", path.display());
    if !text.trim().is_empty() {
        output.push_str(&format!(
            "
屏幕文字（OCR）：
{text}"
        ));
    }
    Ok(ToolResult {
        metrics: None,
        success: true,
        output,
    })
}

#[cfg(windows)]
fn screenshot_windows(output_dir: &std::path::Path, ocr: bool) -> anyhow::Result<ToolResult> {
    let t0 = std::time::Instant::now();
    let screen_w = unsafe { GetSystemMetrics(0) }; // SM_CXSCREEN
    let screen_h = unsafe { GetSystemMetrics(1) }; // SM_CYSCREEN

    let pixels = unsafe { capture_screen_gdi(screen_w, screen_h) }?;

    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let path = output_dir.join(format!("stitch_desktop_{ts}.bmp"));
    write_bmp(&path, screen_w as u32, screen_h as u32, &pixels)?;

    let mut metrics = std::collections::HashMap::new();
    metrics.insert("duration_ms".into(), t0.elapsed().as_secs_f64() * 1000.0);

    Ok(ToolResult {
        success: true,
        metrics: Some(metrics),
        output: format!(
            "Screenshot saved: {}\nDimensions: {}×{}\nSize: {} bytes{ocr_text}",
            path.display(),
            screen_w,
            screen_h,
            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            ocr_text = if ocr {
                match ocr_from_bmp(&path) {
                    Ok(ref ocr) if !ocr.full_text.is_empty() => {
                        let mut out = format!("\n\n--- OCR Text ---\n{}", ocr.full_text);
                        if !ocr.words.is_empty() {
                            out.push_str("\n\n--- OCR Words (click targets) ---");
                            for w in &ocr.words {
                                let cx = w.x + w.w / 2;
                                let cy = w.y + w.h / 2;
                                out.push_str(&format!(
                                    "\n\"{}\" bbox=({},{},{},{}) center=({},{})",
                                    w.text, w.x, w.y, w.w, w.h, cx, cy
                                ));
                            }
                        }
                        out
                    }
                    Ok(_) => "\n(OCR: no text found)".to_string(),
                    Err(ref e) => format!("\n(OCR failed: {e})"),
                }
            } else {
                String::new()
            }
        ),
    })
}

// ── desktop_click ───────────────────────────────────────────────

/// Move mouse to (x, y) and perform a click.
#[derive(Clone)]
pub struct DesktopClick;

impl DesktopClick {
    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "desktop_click".into(),
            description: "Move the mouse cursor to (x, y) screen coordinates and click. \
                 Coordinates are absolute pixels from the top-left of the primary monitor. \
                 Use after desktop_window_list to click on a specific window. \
                 Default button: left."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "x": { "type": "integer", "description": "X coordinate in screen pixels" },
                    "y": { "type": "integer", "description": "Y coordinate in screen pixels" },
                    "button": {
                        "type": "string",
                        "enum": ["left", "right", "middle"],
                        "description": "Mouse button, defaults to left"
                    }
                },
                "required": ["x", "y"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        #[cfg(windows)]
        {
            let x = arguments["x"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'x' argument"))?;
            let y = arguments["y"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'y' argument"))?;
            let button = arguments["button"].as_str().unwrap_or("left");

            click_windows(x, y, button)
        }
        #[cfg(target_os = "linux")]
        {
            let x = arguments["x"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'x' argument"))?;
            let y = arguments["y"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'y' argument"))?;
            let button = arguments["button"].as_str().unwrap_or("left");
            click_linux(x, y, button)
        }
        #[cfg(target_os = "macos")]
        {
            let x = arguments["x"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'x' argument"))?;
            let y = arguments["y"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'y' argument"))?;
            let button = arguments["button"].as_str().unwrap_or("left");
            click_macos(x, y, button)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_click is only supported on Windows/macOS/Linux".into(),
            })
        }
    }
}

/// Linux 点击：xdotool mousemove + click。
#[cfg(target_os = "linux")]
fn click_linux(x: i64, y: i64, button: &str) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let btn = match button {
        "right" => "3",
        "middle" => "2",
        _ => "1",
    };
    let (xs, ys) = (x.to_string(), y.to_string());
    let ok = Command::new("xdotool")
        .args(["mousemove", xs.as_str(), ys.as_str(), "click", btn])
        .status();
    match ok {
        Ok(st) if st.success() => Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!("Clicked ({x}, {y}) {button}"),
        }),
        Ok(_) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: "点击失败（xdotool 不可用？）".into(),
        }),
        Err(e) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("点击失败：{e}（需安装 xdotool）"),
        }),
    }
}

/// macOS 点击：AppleScript System Events `click at`（需辅助功能权限）。
#[cfg(target_os = "macos")]
fn click_macos(x: i64, y: i64, button: &str) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let _ = button; // System Events click 默认左键；右键需 CGEvent 级别，先支持左键
    let script = format!("tell application \"System Events\" to click at {{{x}, {y}}}");
    let out = Command::new("osascript").arg("-e").arg(&script).output();
    match out {
        Ok(o) if o.status.success() => Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!("Clicked ({x}, {y})"),
        }),
        Ok(o) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!(
                "点击失败：{}（需辅助功能权限）",
                String::from_utf8_lossy(&o.stderr)
            ),
        }),
        Err(e) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("点击失败：{e}"),
        }),
    }
}

#[cfg(windows)]
fn click_windows(x: i64, y: i64, button: &str) -> anyhow::Result<ToolResult> {
    let t0 = std::time::Instant::now();
    unsafe {
        send_mouse_move(x as i32, y as i32)?;
        std::thread::sleep(std::time::Duration::from_millis(30));
        // Verify actual cursor position
        let mut pt = POINT { x: 0, y: 0 };
        let got_pos = GetCursorPos(&mut pt);
        send_mouse_button(button, true)?;
        std::thread::sleep(std::time::Duration::from_millis(20));
        send_mouse_button(button, false)?;

        let pos_info = if got_pos != 0 {
            format!(" | actual cursor: ({}, {})", pt.x, pt.y)
        } else {
            String::new()
        };

        let mut metrics = std::collections::HashMap::new();
        metrics.insert("duration_ms".into(), t0.elapsed().as_secs_f64() * 1000.0);

        Ok(ToolResult {
            success: true,
            output: format!("Clicked ({}, {}) with {} button{}", x, y, button, pos_info),
            metrics: Some(metrics),
        })
    }
}

// ── desktop_type ────────────────────────────────────────────────

/// Type a string of text via keyboard simulation.
#[derive(Clone)]
pub struct DesktopType;

impl DesktopType {
    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "desktop_type".into(),
            description: "Type a string of text by simulating keyboard input. \
                     The text is sent to whichever window currently has focus. \
                     Use desktop_window_list to find and focus the target window first, \
                     then use desktop_click to focus it before typing. \
                     Supports ASCII characters and common symbols."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The text to type. Keep it reasonably short (< 500 chars)."
                    }
                },
                "required": ["text"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        #[cfg(windows)]
        {
            let text = arguments["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'text' argument"))?;

            let t0 = std::time::Instant::now();
            type_text_windows(text).map(|r| r.with_duration_ms(t0))
        }
        #[cfg(target_os = "linux")]
        {
            let text = arguments["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'text' argument"))?;
            type_text_linux(text)
        }
        #[cfg(target_os = "macos")]
        {
            let text = arguments["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'text' argument"))?;
            type_text_macos(text)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_type is only supported on Windows/macOS/Linux".into(),
            })
        }
    }
}

/// Linux 键入：xdotool type（unicode 需 xdotool ≥ 3.20160805）。
#[cfg(target_os = "linux")]
fn type_text_linux(text: &str) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let out = Command::new("xdotool")
        .args(["type", "--delay", "30"])
        .arg(text)
        .output();
    match out {
        Ok(o) if o.status.success() => Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!("Typed {} chars", text.chars().count()),
        }),
        Ok(o) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("键入失败：{}", String::from_utf8_lossy(&o.stderr)),
        }),
        Err(e) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("键入失败：{e}（需安装 xdotool）"),
        }),
    }
}

/// macOS 键入：System Events keystroke（需辅助功能权限）。
#[cfg(target_os = "macos")]
fn type_text_macos(text: &str) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let script = format!(
        "tell application \"System Events\" to keystroke {}",
        serde_json::to_string(text).unwrap_or_default()
    );
    let out = Command::new("osascript").arg("-e").arg(&script).output();
    match out {
        Ok(o) if o.status.success() => Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!("Typed {} chars", text.chars().count()),
        }),
        Ok(o) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!(
                "键入失败：{}（需辅助功能权限）",
                String::from_utf8_lossy(&o.stderr)
            ),
        }),
        Err(e) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("键入失败：{e}"),
        }),
    }
}

#[cfg(windows)]
fn type_text_windows(text: &str) -> anyhow::Result<ToolResult> {
    if text.len() > 500 {
        return Ok(ToolResult {
            metrics: None,
            success: false,
            output: "Text too long (max 500 characters for safety)".into(),
        });
    }

    for ch in text.chars() {
        unsafe {
            send_key_char(ch)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    Ok(ToolResult {
        metrics: None,
        success: true,
        output: format!("Typed {} characters", text.len()),
    })
}

// ── desktop_window_list ─────────────────────────────────────────

/// Enumerate visible windows with titles, positions, and sizes.
#[derive(Clone)]
pub struct DesktopWindowList;

impl DesktopWindowList {
    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "desktop_window_list".into(),
            description: "List all visible top-level windows on the desktop. \
                 Returns window title, position (x, y), size (w, h), class name, and window ID. \
                 Use this to find target windows before clicking or typing into them, \
                 or to identify windows that need to be minimized/closed with desktop_window_action."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    pub async fn execute(&self, _arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        #[cfg(windows)]
        {
            let t0 = std::time::Instant::now();
            window_list_windows().map(|r| r.with_duration_ms(t0))
        }
        #[cfg(target_os = "linux")]
        {
            window_list_linux()
        }
        #[cfg(target_os = "macos")]
        {
            window_list_macos()
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_window_list is only supported on Windows/macOS/Linux".into(),
            })
        }
    }
}

/// Linux 窗口列表：xdotool search + getwindowname。
#[cfg(target_os = "linux")]
fn window_list_linux() -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let out = Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", ""])
        .output();
    let ids = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(_) => {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: "窗口列表失败".into(),
            });
        }
        Err(e) => {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!("窗口列表失败：{e}（需安装 xdotool）"),
            });
        }
    };
    let mut rows: Vec<String> = Vec::new();
    for line in ids.lines() {
        let id = line.trim();
        if id.is_empty() {
            continue;
        }
        let name = Command::new("xdotool").args(["getwindowname", id]).output();
        let title = match name {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => String::new(),
        };
        rows.push(format!("{id}	{title}"));
    }
    Ok(ToolResult {
        metrics: None,
        success: true,
        output: format!(
            "Windows ({}):
{}",
            rows.len(),
            rows.join(
                "
"
            )
        ),
    })
}

/// macOS 窗口列表：System Events 前台应用（macOS 无全局窗口标题枚举——简化版）。
#[cfg(target_os = "macos")]
fn window_list_macos() -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let script = "tell application \"System Events\" to get name of every process whose background only is false";
    let out = Command::new("osascript").arg("-e").arg(script).output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            let apps: Vec<&str> = text
                .split(", ")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: format!(
                    "Frontmost apps ({}):
{}",
                    apps.len(),
                    apps.join(
                        "
"
                    )
                ),
            })
        }
        Ok(o) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!(
                "窗口列表失败：{}（需辅助功能权限）",
                String::from_utf8_lossy(&o.stderr)
            ),
        }),
        Err(e) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("窗口列表失败：{e}"),
        }),
    }
}

// ── desktop_window_action ───────────────────────────────────────

/// Manage windows: minimize, close, restore, maximize, or bring to foreground.
#[derive(Clone)]
pub struct DesktopWindowAction;

impl DesktopWindowAction {
    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "desktop_window_action".into(),
            description: "Perform an action on a window found by partial title match. \
                     Actions: 'minimize' (hide to taskbar), 'close' (send close signal), \
                     'restore' (bring back from minimized), 'maximize' (full screen), \
                     'focus' (bring to foreground). \
                     Use this to clear overlapping windows before taking screenshots, \
                     or to bring a target window to the front for interaction. \
                     Matches the first window whose title contains the given text (case-insensitive)."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Partial window title to match (case-insensitive). First match wins."
                    },
                    "action": {
                        "type": "string",
                        "enum": ["minimize", "close", "restore", "maximize", "focus"],
                        "description": "Action to perform on the matched window"
                    }
                },
                "required": ["title", "action"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        #[cfg(windows)]
        {
            let title = arguments["title"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'title' argument"))?;
            let action = arguments["action"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'action' argument"))?;

            let t0 = std::time::Instant::now();
            window_action_windows(title, action).map(|r| r.with_duration_ms(t0))
        }
        #[cfg(target_os = "linux")]
        {
            let title = arguments["title"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'title' argument"))?;
            let action = arguments["action"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'action' argument"))?;
            window_action_linux(title, action)
        }
        #[cfg(target_os = "macos")]
        {
            let title = arguments["title"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'title' argument"))?;
            let action = arguments["action"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'action' argument"))?;
            let _ = (title, action);
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_window_action is not supported on macOS yet".into(),
            })
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_window_action is only supported on Windows/macOS/Linux".into(),
            })
        }
    }
}

/// Linux 窗口操作：xdotool search --name 匹配全部 → 操作（close/minimize/maximize/focus）。
#[cfg(target_os = "linux")]
fn window_action_linux(title_part: &str, action: &str) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let out = Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", title_part])
        .output();
    let ids = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(_) => {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: "窗口查找失败".into(),
            });
        }
        Err(e) => {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!("窗口查找失败：{e}（需安装 xdotool）"),
            });
        }
    };
    let id_list: Vec<&str> = ids
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if id_list.is_empty() {
        return Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("No visible window with title containing '{title_part}'"),
        });
    }
    let sub = match action {
        "close" => "windowclose",
        "minimize" => "windowminimize",
        "maximize" => "windowmaximize",
        "restore" | "focus" => "windowactivate",
        _ => {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!("Unknown action: {action}"),
            });
        }
    };
    let mut done = 0usize;
    for id in id_list {
        let r = Command::new("xdotool").args([sub, id]).output();
        if let Ok(o) = r
            && o.status.success()
        {
            done += 1;
            }
        }
    }
    // close 自查：窗口是否真的关闭
    if action == "close" {
        std::thread::sleep(std::time::Duration::from_millis(350));
        let check = Command::new("xdotool")
            .args(["search", "--onlyvisible", "--name", title_part])
            .output();
        let remaining = match check {
            Ok(o) => String::from_utf8_lossy(&o.stdout).lines().count(),
            _ => 0,
        };
        if remaining > 0 {
            return Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!(
                    "Closed {done} window(s); {remaining} still open (可能被对话框拦截)"
                ),
            });
        }
    }
    Ok(ToolResult {
        metrics: None,
        success: true,
        output: format!("{action}: {done} window(s)"),
    })
}

#[cfg(windows)]
fn window_list_windows() -> anyhow::Result<ToolResult> {
    use std::sync::Mutex;

    #[derive(Debug)]
    struct WinInfo {
        title: String,
        class: String,
        id: isize,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    }

    let windows: Mutex<Vec<WinInfo>> = Mutex::new(Vec::new());

    unsafe extern "system" fn enum_cb(hwnd: isize, lparam: isize) -> i32 {
        let windows: &Mutex<Vec<WinInfo>> = unsafe { &*(lparam as *const Mutex<Vec<WinInfo>>) };

        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
        }
        let mut title = [0u16; 256];
        let title_len = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
        if title_len == 0 {
            return 1;
        }

        let mut class = [0u16; 128];
        unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe { GetWindowRect(hwnd, &mut rect) };

        let info = WinInfo {
            title: String::from_utf16_lossy(&title[..title_len as usize]),
            class: String::from_utf16_lossy(
                &class[..class.iter().position(|&c| c == 0).unwrap_or(class.len())],
            ),
            id: hwnd,
            x: rect.left,
            y: rect.top,
            w: rect.right - rect.left,
            h: rect.bottom - rect.top,
        };

        if let Ok(mut guard) = windows.lock() {
            guard.push(info);
        }
        1
    }

    unsafe {
        EnumWindows(Some(enum_cb), &windows as *const _ as isize);
    }

    let mut guard = windows.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
    guard.sort_by(|a, b| a.title.cmp(&b.title));

    let mut out = String::from("Visible windows:\n");
    for w in guard.iter() {
        out.push_str(&format!(
            "  [{}] \"{}\" id={} pos=({},{}) size={}×{}\n",
            w.class, w.title, w.id, w.x, w.y, w.w, w.h
        ));
    }

    if guard.is_empty() {
        out.push_str("  (no visible windows with titles found)\n");
    }

    Ok(ToolResult {
        metrics: None,
        success: true,
        output: out,
    })
}

// ── desktop_key ─────────────────────────────────────────────────

// Virtual key codes for common keys
#[cfg(windows)]
mod vk {
    pub const SHIFT: u16 = 0x10;
    pub const CONTROL: u16 = 0x11;
    pub const MENU: u16 = 0x12; // Alt
    pub const LWIN: u16 = 0x5B;
    pub const RWIN: u16 = 0x5C;
    pub const RETURN: u16 = 0x0D;
    pub const TAB: u16 = 0x09;
    pub const ESCAPE: u16 = 0x1B;
    pub const SPACE: u16 = 0x20;
    pub const BACK: u16 = 0x08;
    pub const DELETE: u16 = 0x2E;
    pub const HOME: u16 = 0x24;
    pub const END: u16 = 0x23;
    pub const PRIOR: u16 = 0x21; // Page Up
    pub const NEXT: u16 = 0x22; // Page Down
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const INSERT: u16 = 0x2D;
    pub const SNAPSHOT: u16 = 0x2C; // Print Screen
    pub const F1: u16 = 0x70;
    pub const F2: u16 = 0x71;
    pub const F3: u16 = 0x72;
    pub const F4: u16 = 0x73;
    pub const F5: u16 = 0x74;
    pub const F6: u16 = 0x75;
    pub const F7: u16 = 0x76;
    pub const F8: u16 = 0x77;
    pub const F9: u16 = 0x78;
    pub const F10: u16 = 0x79;
    pub const F11: u16 = 0x7A;
    pub const F12: u16 = 0x7B;
    pub const A: u16 = 0x41;
    pub const C: u16 = 0x43;
    pub const V: u16 = 0x56;
    pub const X: u16 = 0x58;
    pub const Z: u16 = 0x5A;
    pub const Y: u16 = 0x59;
    pub const L: u16 = 0x4C;
    pub const R: u16 = 0x52;
    pub const T: u16 = 0x54;
    pub const N: u16 = 0x4E;
    pub const W: u16 = 0x57;
    pub const D: u16 = 0x44;
    pub const E: u16 = 0x45;
    pub const S: u16 = 0x53;
    pub const F: u16 = 0x46;
    pub const O: u16 = 0x4F;
    pub const P: u16 = 0x50;
    pub const Q: u16 = 0x51;
}

/// Send keyboard shortcuts with modifier keys (Ctrl, Alt, Shift, Win).
#[derive(Clone)]
pub struct DesktopKey;

impl DesktopKey {
    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "desktop_key".into(),
            description: "Send a keyboard shortcut (key combination with modifiers). \
                     Supports: ctrl, alt, shift, win as modifiers plus any named key \
                     (enter, tab, escape, space, backspace, delete, home, end, \
                     pageup, pagedown, left, up, right, down, f1-f12, a-z, 0-9). \
                     Examples: ['ctrl','l'] for Ctrl+L, ['alt','tab'] for Alt+Tab, \
                     ['ctrl','shift','escape'] for Ctrl+Shift+Esc. \
                     Use after desktop_window_list to find and focus the target window."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Ordered list of key names. All but the last are held as modifiers; the last key is pressed and released."
                    }
                },
                "required": ["keys"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        let keys: Vec<String> = arguments["keys"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing 'keys' array"))?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
            .collect();

        #[cfg(windows)]
        {
            if keys.is_empty() {
                return Ok(ToolResult {
                    metrics: None,
                    success: false,
                    output: "No keys provided".into(),
                });
            }

            let t0 = std::time::Instant::now();
            send_key_combo(&keys).map(|r| r.with_duration_ms(t0))
        }
        #[cfg(target_os = "linux")]
        {
            key_linux(&keys)
        }
        #[cfg(target_os = "macos")]
        {
            key_macos(&keys)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_key is only supported on Windows/macOS/Linux".into(),
            })
        }
    }
}

/// Linux 按键：xdotool key（组合键如 "ctrl+c"）。
#[cfg(target_os = "linux")]
fn key_linux(keys: &[String]) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let combo = keys.join("+");
    let out = Command::new("xdotool")
        .args(["key", combo.as_str()])
        .output();
    match out {
        Ok(o) if o.status.success() => Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!("Pressed {combo}"),
        }),
        Ok(o) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("按键失败：{}", String::from_utf8_lossy(&o.stderr)),
        }),
        Err(e) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("按键失败：{e}（需安装 xdotool）"),
        }),
    }
}

/// macOS 按键：System Events key code / keystroke（组合键需 key code 映射，先支持简单键）。
#[cfg(target_os = "macos")]
fn key_macos(keys: &[String]) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    // 简单键名直发；组合键（含 ctrl/cmd/alt/shift）用 keystroke + using
    let (base, mods): (Vec<&String>, Vec<&String>) = keys.iter().partition(|k| {
        !matches!(
            k.as_str(),
            "ctrl" | "cmd" | "alt" | "shift" | "option" | "control" | "command"
        )
    });
    let base_joined: String = base.iter().map(|s| s.as_str()).collect();
    if mods.is_empty() {
        let script = format!(
            "tell application \"System Events\" to keystroke \"{}\"",
            base_joined
        );
        let out = Command::new("osascript").arg("-e").arg(&script).output();
        return match out {
            Ok(o) if o.status.success() => Ok(ToolResult {
                metrics: None,
                success: true,
                output: format!("Pressed {}", keys.join("+")),
            }),
            Ok(o) => Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!("按键失败：{}", String::from_utf8_lossy(&o.stderr)),
            }),
            Err(e) => Ok(ToolResult {
                metrics: None,
                success: false,
                output: format!("按键失败：{e}"),
            }),
        };
    }
    let mods_joined = mods
        .iter()
        .map(|m| match m.as_str() {
            "ctrl" | "control" => "control down",
            "cmd" | "command" => "command down",
            "alt" | "option" => "option down",
            "shift" => "shift down",
            _ => "command down",
        })
        .collect::<Vec<_>>()
        .join(", ");
    let script = format!(
        "tell application \"System Events\" to keystroke \"{}\" using {{{mods_joined}}}",
        base_joined
    );
    let out = Command::new("osascript").arg("-e").arg(&script).output();
    match out {
        Ok(o) if o.status.success() => Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!("Pressed {}", keys.join("+")),
        }),
        Ok(o) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!(
                "按键失败：{}（需辅助功能权限）",
                String::from_utf8_lossy(&o.stderr)
            ),
        }),
        Err(e) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("按键失败：{e}"),
        }),
    }
}

// ── desktop_scroll ───────────────────────────────────────────────

/// Scroll the mouse wheel at the current cursor position.
#[derive(Clone)]
pub struct DesktopScroll;

impl DesktopScroll {
    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "desktop_scroll".into(),
            description: "Scroll the mouse wheel at the current cursor position. \
                     Use a positive amount to scroll down, negative to scroll up. \
                     Each unit is roughly one 'notch' of the wheel (one line). \
                     Useful for scrolling through long pages after taking a screenshot."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "amount": {
                        "type": "integer",
                        "description": "Number of scroll notches. Positive = down, negative = up. Max ±20."
                    }
                },
                "required": ["amount"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        #[cfg(windows)]
        {
            let amount = arguments["amount"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'amount' argument"))?;

            if amount == 0 {
                return Ok(ToolResult {
                    metrics: None,
                    success: true,
                    output: "No scroll needed (amount=0)".into(),
                });
            }

            let capped = amount.clamp(-20, 20);
            let t0 = std::time::Instant::now();
            send_mouse_wheel(capped as i32).map(|r| r.with_duration_ms(t0))
        }
        #[cfg(target_os = "linux")]
        {
            let amount = arguments["amount"].as_i64().unwrap_or(1);
            scroll_linux(amount)
        }
        #[cfg(target_os = "macos")]
        {
            let amount = arguments["amount"].as_i64().unwrap_or(1);
            let _ = amount;
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_scroll is not supported on macOS yet".into(),
            })
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_scroll is only supported on Windows/macOS/Linux".into(),
            })
        }
    }
}

/// Linux 滚动：xdotool click 4（上）/ 5（下），amount 次。
#[cfg(target_os = "linux")]
fn scroll_linux(amount: i64) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let btn = if amount >= 0 { "5" } else { "4" };
    let times = amount.abs().clamp(1, 50);
    let mut ok = true;
    for _ in 0..times {
        let r = Command::new("xdotool").args(["click", btn]).output();
        if !matches!(r, Ok(o) if o.status.success()) {
            ok = false;
            break;
        }
    }
    Ok(ToolResult {
        metrics: None,
        success: ok,
        output: if ok {
            format!("Scrolled {times} step(s)")
        } else {
            "滚动失败（xdotool 不可用？）".into()
        },
    })
}

// ═══════════════════════════════════════════════════════════════
// Helper: send keyboard shortcut combo
// ═══════════════════════════════════════════════════════════════

#[cfg(windows)]
fn vk_from_name(name: &str) -> Option<u16> {
    match name {
        "shift" => Some(vk::SHIFT),
        "ctrl" | "control" => Some(vk::CONTROL),
        "alt" | "menu" => Some(vk::MENU),
        "win" | "lwin" | "windows" => Some(vk::LWIN),
        "rwin" => Some(vk::RWIN),
        "enter" | "return" => Some(vk::RETURN),
        "tab" => Some(vk::TAB),
        "escape" | "esc" => Some(vk::ESCAPE),
        "space" => Some(vk::SPACE),
        "backspace" | "back" => Some(vk::BACK),
        "delete" | "del" => Some(vk::DELETE),
        "home" => Some(vk::HOME),
        "end" => Some(vk::END),
        "pageup" | "pgup" => Some(vk::PRIOR),
        "pagedown" | "pgdn" => Some(vk::NEXT),
        "left" => Some(vk::LEFT),
        "up" => Some(vk::UP),
        "right" => Some(vk::RIGHT),
        "down" => Some(vk::DOWN),
        "insert" | "ins" => Some(vk::INSERT),
        "printscreen" | "prtsc" => Some(vk::SNAPSHOT),
        "f1" => Some(vk::F1),
        "f2" => Some(vk::F2),
        "f3" => Some(vk::F3),
        "f4" => Some(vk::F4),
        "f5" => Some(vk::F5),
        "f6" => Some(vk::F6),
        "f7" => Some(vk::F7),
        "f8" => Some(vk::F8),
        "f9" => Some(vk::F9),
        "f10" => Some(vk::F10),
        "f11" => Some(vk::F11),
        "f12" => Some(vk::F12),
        // Single letters a-z
        s if s.len() == 1 => {
            let ch = s.chars().next().unwrap();
            match ch {
                'a'..='z' => Some(0x41 + (ch as u16 - 'a' as u16)),
                '0'..='9' => Some(0x30 + (ch as u16 - '0' as u16)),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(windows)]
fn send_key_combo(keys: &[String]) -> anyhow::Result<ToolResult> {
    if keys.is_empty() {
        return Ok(ToolResult {
            metrics: None,
            success: false,
            output: "No keys provided".into(),
        });
    }

    // Resolve all key names to VK codes
    let vks: Vec<(String, u16)> = keys
        .iter()
        .map(|k| {
            vk_from_name(k)
                .map(|v| (k.clone(), v))
                .ok_or_else(|| anyhow::anyhow!("Unknown key: {k}"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    // All but the last are modifiers (held down)
    let (modifiers, main) = vks.split_at(vks.len().saturating_sub(1));
    let main_key = if main.is_empty() {
        // If all keys are modifiers? unlikely but handle it
        return Ok(ToolResult {
            metrics: None,
            success: false,
            output: "Need at least one non-modifier key".into(),
        });
    } else {
        &main[0]
    };

    // Press modifiers down (in order)
    for (_name, vk) in modifiers {
        unsafe {
            send_vk_key(*vk, false)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Press and release main key
    unsafe {
        send_vk_key(main_key.1, false)?;
        std::thread::sleep(std::time::Duration::from_millis(20));
        send_vk_key(main_key.1, true)?;
    }

    // Release modifiers (in reverse order)
    for (_name, vk) in modifiers.iter().rev() {
        std::thread::sleep(std::time::Duration::from_millis(10));
        unsafe {
            send_vk_key(*vk, true)?;
        }
    }

    let combo_desc: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
    Ok(ToolResult {
        metrics: None,
        success: true,
        output: format!("Sent key combo: {}", combo_desc.join("+")),
    })
}

#[cfg(windows)]
unsafe fn send_vk_key(vk: u16, key_up: bool) -> anyhow::Result<()> {
    let flags = if key_up { KEYEVENTF_KEYUP } else { 0 };
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        u: INPUT_DATA {
            ki: std::mem::ManuallyDrop::new(KEYBDINPUT {
                w_vk: vk,
                w_scan: 0,
                dw_flags: flags,
                time: 0,
                dw_extra_info: 0,
            }),
        },
    };
    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        return Err(anyhow::anyhow!("SendInput (vk key) failed"));
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// Helper: mouse wheel scroll
// ═══════════════════════════════════════════════════════════════

#[cfg(windows)]
const MOUSEEVENTF_WHEEL: u32 = 0x0800;
#[cfg(windows)]
const WHEEL_DELTA: i32 = 120;

// ShowWindow constants
#[cfg(windows)]
const SW_MINIMIZE: i32 = 6;
#[cfg(windows)]
const SW_RESTORE: i32 = 9;
#[cfg(windows)]
const SW_MAXIMIZE: i32 = 3;

#[cfg(windows)]
fn send_mouse_wheel(amount: i32) -> anyhow::Result<ToolResult> {
    let wheel_move = (amount * WHEEL_DELTA) as u32;

    let input = INPUT {
        r#type: INPUT_MOUSE,
        u: INPUT_DATA {
            mi: std::mem::ManuallyDrop::new(MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouse_data: wheel_move,
                dw_flags: MOUSEEVENTF_WHEEL,
                time: 0,
                dw_extra_info: 0,
            }),
        },
    };

    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        return Err(anyhow::anyhow!("SendInput (mouse wheel) failed"));
    }

    let direction = if amount >= 0 { "down" } else { "up" };
    Ok(ToolResult {
        metrics: None,
        success: true,
        output: format!("Scrolled {direction} by {} notch(es)", amount.abs()),
    })
}

// ═══════════════════════════════════════════════════════════════
// Helper: OCR from BMP screenshot (Windows 10+)
// ═══════════════════════════════════════════════════════════════

/// Run Windows built-in OCR on a BMP file and return recognized text.
#[cfg(windows)]
/// A single recognized word with its bounding box in screen coordinates.
#[derive(Debug, Clone)]
#[cfg(windows)]
struct OcrWord {
    text: String,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Structured OCR result: full text + per-word bounding boxes.
#[cfg(windows)]
struct OcrOutput {
    full_text: String,
    words: Vec<OcrWord>,
}

#[cfg(windows)]
fn ocr_from_bmp(path: &std::path::Path) -> anyhow::Result<OcrOutput> {
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::FileAccessMode;
    use windows::Storage::Streams::FileRandomAccessStream;
    use windows::core::HSTRING;

    // Open BMP file via Windows Runtime stream
    let path_hstring = HSTRING::from(
        path.to_str()
            .ok_or_else(|| anyhow::anyhow!("BMP path is not valid UTF-8"))?,
    );
    let file = FileRandomAccessStream::OpenAsync(&path_hstring, FileAccessMode::Read)?
        .get()
        .map_err(|e| anyhow::anyhow!("Failed to open BMP for OCR: {e}"))?;
    let decoder = BitmapDecoder::CreateAsync(&file)?
        .get()
        .map_err(|e| anyhow::anyhow!("Failed to decode BMP: {e}"))?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()?
        .get()
        .map_err(|e| anyhow::anyhow!("Failed to get bitmap for OCR: {e}"))?;

    // Try Chinese OCR first, fall back to English
    let zh = HSTRING::from("zh-Hans");
    let en = HSTRING::from("en-US");
    let engine = OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&zh)?)
        .or_else(|_| OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&en)?))
        .map_err(|e| anyhow::anyhow!("OCR engine unavailable: {e}"))?;

    let result = engine
        .RecognizeAsync(&bitmap)?
        .get()
        .map_err(|e| anyhow::anyhow!("OCR recognition failed: {e}"))?;

    let full_text = result
        .Text()
        .map_err(|e| anyhow::anyhow!("OCR text extraction failed: {e}"))?
        .to_string();

    // Extract per-word bounding boxes for click-target mapping
    let mut words = Vec::new();
    let lines = result.Lines()?;
    for line in &lines {
        let line_words = line.Words()?;
        for word in &line_words {
            let rect = word.BoundingRect()?;
            let word_text = word.Text()?.to_string();
            if !word_text.trim().is_empty() {
                words.push(OcrWord {
                    text: word_text,
                    x: rect.X as u32,
                    y: rect.Y as u32,
                    w: rect.Width as u32,
                    h: rect.Height as u32,
                });
            }
        }
    }

    Ok(OcrOutput { full_text, words })
}

// ═══════════════════════════════════════════════════════════════
// Win32 FFI — screen capture
// ═══════════════════════════════════════════════════════════════

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)] // Win32 类型名
#[repr(C)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)] // Win32 类型名
#[repr(C)]
struct POINT {
    x: i32,
    y: i32,
}

#[cfg(windows)]
#[repr(C)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_pels_per_meter: i32,
    bi_y_pels_per_meter: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

#[cfg(windows)]
unsafe fn capture_screen_gdi(w: i32, h: i32) -> anyhow::Result<Vec<u8>> {
    // Gather all unsafe calls in one block for readability
    let hdc_screen = unsafe { GetDC(0) };
    if hdc_screen == 0 {
        return Err(anyhow::anyhow!("GetDC failed"));
    }
    let hdc_mem = unsafe { CreateCompatibleDC(hdc_screen) };
    if hdc_mem == 0 {
        unsafe { ReleaseDC(0, hdc_screen) };
        return Err(anyhow::anyhow!("CreateCompatibleDC failed"));
    }
    let hbm = unsafe { CreateCompatibleBitmap(hdc_screen, w, h) };
    if hbm == 0 {
        unsafe {
            DeleteDC(hdc_mem);
            ReleaseDC(0, hdc_screen);
        }
        return Err(anyhow::anyhow!("CreateCompatibleBitmap failed"));
    }
    let old_bm = unsafe { SelectObject(hdc_mem, hbm) };
    let blt_ok = unsafe { BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, 0, 0, 0x00CC0020) };
    if blt_ok == 0 {
        unsafe {
            SelectObject(hdc_mem, old_bm);
            DeleteObject(hbm);
            DeleteDC(hdc_mem);
            ReleaseDC(0, hdc_screen);
        }
        return Err(anyhow::anyhow!("BitBlt failed"));
    }

    // Get pixel data via GetDIBits — BGRA, bottom-up
    let row_size = ((w * 32 + 31) / 32) * 4;
    let img_size = (row_size * h) as usize;
    let mut pixels: Vec<u8> = vec![0u8; img_size];

    let mut bi = BitmapInfoHeader {
        bi_size: 40,
        bi_width: w,
        bi_height: h,
        bi_planes: 1,
        bi_bit_count: 32,
        bi_compression: 0,
        bi_size_image: img_size as u32,
        bi_x_pels_per_meter: 0,
        bi_y_pels_per_meter: 0,
        bi_clr_used: 0,
        bi_clr_important: 0,
    };

    let result = unsafe { GetDIBits(hdc_mem, hbm, 0, h as u32, pixels.as_mut_ptr(), &mut bi, 0) };

    // Cleanup
    unsafe {
        SelectObject(hdc_mem, old_bm);
        DeleteObject(hbm);
        DeleteDC(hdc_mem);
        ReleaseDC(0, hdc_screen);
    }

    if result == 0 {
        return Err(anyhow::anyhow!("GetDIBits failed"));
    }

    Ok(pixels)
}

#[cfg(windows)]
fn write_bmp(path: &std::path::Path, w: u32, h: u32, pixels: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;

    let row_size = (w * 32).div_ceil(32) * 4;
    let img_size = row_size * h;
    let file_size = 14 + 40 + img_size;

    let mut f = std::fs::File::create(path)?;

    // BITMAPFILEHEADER
    f.write_all(&0x4D42u16.to_le_bytes())?; // bfType = "BM"
    f.write_all(&file_size.to_le_bytes())?; // bfSize
    f.write_all(&[0u8; 4])?; // reserved
    f.write_all(&(54u32).to_le_bytes())?; // bfOffBits

    // BITMAPINFOHEADER
    f.write_all(&40u32.to_le_bytes())?;
    f.write_all(&(w as i32).to_le_bytes())?;
    f.write_all(&(h as i32).to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&32u16.to_le_bytes())?;
    f.write_all(&0u32.to_le_bytes())?;
    f.write_all(&img_size.to_le_bytes())?;
    f.write_all(&0u32.to_le_bytes())?;
    f.write_all(&0u32.to_le_bytes())?;
    f.write_all(&0u32.to_le_bytes())?;
    f.write_all(&0u32.to_le_bytes())?;

    // Pixel data
    f.write_all(&pixels[..img_size as usize])?;

    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// Win32 FFI — mouse input
// ═══════════════════════════════════════════════════════════════

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)] // Win32 类型名
#[repr(C)]
struct MOUSEINPUT {
    dx: i32,
    dy: i32,
    mouse_data: u32,
    dw_flags: u32,
    time: u32,
    dw_extra_info: usize,
}

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)] // Win32 类型名
#[repr(C)]
struct KEYBDINPUT {
    w_vk: u16,
    w_scan: u16,
    dw_flags: u32,
    time: u32,
    dw_extra_info: usize,
}

#[cfg(windows)]
#[repr(C)]
union INPUT_DATA {
    mi: std::mem::ManuallyDrop<MOUSEINPUT>,
    ki: std::mem::ManuallyDrop<KEYBDINPUT>,
}

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)] // Win32 类型名
#[repr(C)]
struct INPUT {
    r#type: u32,
    u: INPUT_DATA,
}

#[cfg(windows)]
const INPUT_MOUSE: u32 = 0;
#[cfg(windows)]
const INPUT_KEYBOARD: u32 = 1;
#[cfg(windows)]
const MOUSEEVENTF_MOVE: u32 = 0x0001;
#[cfg(windows)]
const MOUSEEVENTF_ABSOLUTE: u32 = 0x8000;
#[cfg(windows)]
const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
#[cfg(windows)]
const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
#[cfg(windows)]
const MOUSEEVENTF_RIGHTDOWN: u32 = 0x0008;
#[cfg(windows)]
const MOUSEEVENTF_RIGHTUP: u32 = 0x0010;
#[cfg(windows)]
const KEYEVENTF_KEYUP: u32 = 0x0002;
#[cfg(windows)]
const KEYEVENTF_UNICODE: u32 = 0x0004;

#[cfg(windows)]
unsafe fn send_mouse_move(x: i32, y: i32) -> anyhow::Result<()> {
    // Use SetCursorPos for reliable positioning (bypasses UIPI issues)
    let ok = unsafe { SetCursorPos(x, y) };
    if ok == 0 {
        return Err(anyhow::anyhow!("SetCursorPos failed"));
    }
    Ok(())
}

#[cfg(windows)]
unsafe fn send_mouse_button(button: &str, down: bool) -> anyhow::Result<()> {
    let (down_flag, up_flag) = match button {
        "left" => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
    };

    let flags = if down { down_flag } else { up_flag };

    // Prefer mouse_event (older API, less likely blocked by UIPI)
    // over SendInput for button events.
    unsafe {
        mouse_event(flags, 0, 0, 0, 0);
    }

    // Also send via SendInput as belt-and-suspenders.
    let input = INPUT {
        r#type: INPUT_MOUSE,
        u: INPUT_DATA {
            mi: std::mem::ManuallyDrop::new(MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouse_data: 0,
                dw_flags: flags,
                time: 0,
                dw_extra_info: 0,
            }),
        },
    };

    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        return Err(anyhow::anyhow!("SendInput (button) failed"));
    }
    Ok(())
}

#[cfg(windows)]
unsafe fn send_key_char(ch: char) -> anyhow::Result<()> {
    let mut ch_buf = [0u16; 2];
    let len = ch.encode_utf16(&mut ch_buf).len();

    for &code in &ch_buf[..len] {
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            u: INPUT_DATA {
                ki: std::mem::ManuallyDrop::new(KEYBDINPUT {
                    w_vk: 0,
                    w_scan: code,
                    dw_flags: KEYEVENTF_UNICODE,
                    time: 0,
                    dw_extra_info: 0,
                }),
            },
        };
        if unsafe { SendInput(1, &down, std::mem::size_of::<INPUT>() as i32) } == 0 {
            return Err(anyhow::anyhow!("SendInput (key down) failed"));
        }

        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            u: INPUT_DATA {
                ki: std::mem::ManuallyDrop::new(KEYBDINPUT {
                    w_vk: 0,
                    w_scan: code,
                    dw_flags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dw_extra_info: 0,
                }),
            },
        };
        if unsafe { SendInput(1, &up, std::mem::size_of::<INPUT>() as i32) } == 0 {
            return Err(anyhow::anyhow!("SendInput (key up) failed"));
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// Win32 FFI — extern declarations (unsafe for Rust 2024)
// ═══════════════════════════════════════════════════════════════

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)] // Win32 类型名遵循系统命名（RECT/INPUT/POINT…）
unsafe extern "system" {
    // Screen
    fn GetSystemMetrics(n_index: i32) -> i32;
    fn GetDC(h_wnd: isize) -> isize;
    fn ReleaseDC(h_wnd: isize, h_dc: isize) -> i32;
    fn CreateCompatibleDC(h_dc: isize) -> isize;
    fn CreateCompatibleBitmap(h_dc: isize, width: i32, height: i32) -> isize;
    fn SelectObject(h_dc: isize, h_gdi_obj: isize) -> isize;
    fn BitBlt(
        h_dc: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        h_dc_src: isize,
        x1: i32,
        y1: i32,
        rop: u32,
    ) -> i32;
    fn GetDIBits(
        h_dc: isize,
        h_bmp: isize,
        start: u32,
        lines: u32,
        bits: *mut u8,
        bi: *mut BitmapInfoHeader,
        usage: u32,
    ) -> i32;
    fn DeleteDC(h_dc: isize) -> i32;
    fn DeleteObject(h_obj: isize) -> i32;

    // Windows
    fn EnumWindows(
        cb: Option<unsafe extern "system" fn(isize, isize) -> i32>,
        lparam: isize,
    ) -> i32;
    fn IsWindowVisible(h_wnd: isize) -> i32;
    fn IsWindow(h_wnd: isize) -> i32;
    fn GetWindowTextW(h_wnd: isize, lp_string: *mut u16, n_max_count: i32) -> i32;
    fn GetClassNameW(h_wnd: isize, lp_class_name: *mut u16, n_max_count: i32) -> i32;
    fn GetWindowRect(h_wnd: isize, lp_rect: *mut RECT) -> i32;

    // Input
    fn SendInput(c_inputs: u32, p_inputs: *const INPUT, cb_size: i32) -> u32;
    fn mouse_event(dw_flags: u32, dx: i32, dy: i32, dw_data: u32, dw_extra_info: usize);
    // Window management
    fn ShowWindow(h_wnd: isize, n_cmd_show: i32) -> i32;
    fn SetForegroundWindow(h_wnd: isize) -> i32;
    fn IsIconic(h_wnd: isize) -> i32;
    fn PostMessageW(h_wnd: isize, msg: u32, w_param: usize, l_param: isize) -> i32;
    fn GetCursorPos(lp_point: *mut POINT) -> i32;
    fn SetCursorPos(x: i32, y: i32) -> i32;
}

// ═══════════════════════════════════════════════════════════════
// Helper: window action (minimize/close/restore/maximize/focus)
// ═══════════════════════════════════════════════════════════════

/// Callback used by window_action_windows to enumerate window titles.
#[cfg(windows)]
unsafe extern "system" fn enum_window_titles_cb(hwnd: isize, lparam: isize) -> i32 {
    use std::sync::Mutex;
    let windows: &Mutex<Vec<(isize, String)>> =
        unsafe { &*(lparam as *const Mutex<Vec<(isize, String)>>) };
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }
    let mut title = [0u16; 256];
    let title_len = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
    if title_len == 0 {
        return 1;
    }
    let t = String::from_utf16_lossy(&title[..title_len as usize]);
    if let Ok(mut guard) = windows.lock() {
        guard.push((hwnd, t));
    }
    1
}

#[cfg(windows)]
fn window_action_windows(title_part: &str, action: &str) -> anyhow::Result<ToolResult> {
    use std::sync::Mutex;

    let title_lower = title_part.to_lowercase();
    let windows: Mutex<Vec<(isize, String)>> = Mutex::new(Vec::new());
    unsafe {
        EnumWindows(Some(enum_window_titles_cb), &windows as *const _ as isize);
    }
    // 匹配**所有**窗口（多实例场景：不止第一个）——benchmark 发现按标题首窗
    // 匹配在多实例时只关第一个、且从不复查结果。
    let matches: Vec<(isize, String)> = {
        let guard = windows.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        guard
            .iter()
            .filter(|(_, t)| t.to_lowercase().contains(&title_lower))
            .cloned()
            .collect()
    };

    if matches.is_empty() {
        return Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("No visible window with title containing '{title_part}'"),
        });
    }
    let hwnd = matches[0].0;

    match action {
        "minimize" => {
            unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: format!("Minimized '{title_part}'"),
            })
        }
        "close" => {
            // 逐个发 WM_CLOSE（多实例全关），随后复查窗口是否真的关闭——
            // 模态对话框（如记事本保存提示）会拦截 WM_CLOSE，窗口仍在。
            let mut closed = 0usize;
            let mut remaining: Vec<String> = Vec::new();
            for (h, _) in &matches {
                unsafe { PostMessageW(*h, 0x0010, 0, 0) };
            }
            // 给窗口处理消息的时间
            std::thread::sleep(std::time::Duration::from_millis(350));
            for (h, title) in &matches {
                let alive = unsafe { IsWindow(*h) != 0 };
                if alive {
                    remaining.push(title.clone());
                } else {
                    closed += 1;
                }
            }
            let output = if remaining.is_empty() {
                format!("Closed {closed} window(s) matching '{title_part}'")
            } else {
                format!(
                    "Closed {closed} window(s); {} still open (可能被模态对话框拦截): {}",
                    remaining.len(),
                    remaining.join(" | ")
                )
            };
            Ok(ToolResult {
                metrics: None,
                success: remaining.is_empty(),
                output,
            })
        }
        "restore" => {
            unsafe { ShowWindow(hwnd, SW_RESTORE) };
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: format!("Restored '{title_part}'"),
            })
        }
        "maximize" => {
            unsafe { ShowWindow(hwnd, SW_MAXIMIZE) };
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: format!("Maximized '{title_part}'"),
            })
        }
        "focus" => {
            unsafe {
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                SetForegroundWindow(hwnd);
            }
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: format!("Focused '{title_part}'"),
            })
        }
        _ => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("Unknown action: {action}"),
        }),
    }
}

// ── desktop_hover ───────────────────────────────────────────

/// Move the mouse cursor to a screen position **without clicking**.
/// Use this to trigger hover-revealed UI (tooltips, dropdown buttons, menus)
/// that only appear when the mouse is over a specific element.
/// Follow up with desktop_screenshot ocr=true to see what appeared.
#[derive(Clone)]
pub struct DesktopHover;

impl DesktopHover {
    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "desktop_hover".into(),
            description: "Move the mouse cursor to (x, y) on screen WITHOUT clicking. \
                Use this to reveal hover-triggered UI elements like tooltips, \
                dropdown menus, or hidden buttons. After hovering, use \
                desktop_screenshot with ocr=true to see what appeared. \
                Combine with desktop_click afterwards to click revealed elements."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "x": {
                        "type": "integer",
                        "description": "X screen coordinate to move the mouse to"
                    },
                    "y": {
                        "type": "integer",
                        "description": "Y screen coordinate to move the mouse to"
                    }
                },
                "required": ["x", "y"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        #[cfg(windows)]
        {
            let x = arguments["x"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'x' argument"))?;
            let y = arguments["y"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'y' argument"))?;

            let t0 = std::time::Instant::now();
            unsafe {
                send_mouse_move(x as i32, y as i32)?;
            }
            // Give the UI time to react to hover
            std::thread::sleep(std::time::Duration::from_millis(600));

            Ok(ToolResult {
                metrics: None,
                success: true,
                output: format!("Mouse moved to ({x}, {y}) — no click, hover only"),
            }
            .with_duration_ms(t0))
        }
        #[cfg(target_os = "linux")]
        {
            let x = arguments["x"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'x' argument"))?;
            let y = arguments["y"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'y' argument"))?;
            hover_linux(x, y)
        }
        #[cfg(target_os = "macos")]
        {
            let x = arguments["x"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'x' argument"))?;
            let y = arguments["y"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'y' argument"))?;
            let _ = (x, y);
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_hover is not supported on macOS yet".into(),
            })
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_hover is only supported on Windows/macOS/Linux".into(),
            })
        }
    }
}

/// Linux 悬停：xdotool mousemove。
#[cfg(target_os = "linux")]
fn hover_linux(x: i64, y: i64) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let (xs, ys) = (x.to_string(), y.to_string());
    let out = Command::new("xdotool")
        .args(["mousemove", xs.as_str(), ys.as_str()])
        .output();
    match out {
        Ok(o) if o.status.success() => Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!("Moved cursor to ({x}, {y})"),
        }),
        Ok(_) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: "移动失败".into(),
        }),
        Err(e) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("移动失败：{e}（需安装 xdotool）"),
        }),
    }
}

// ── desktop_browser ──────────────────────────────────────────

/// High-level browser control: navigate, click text on page, find-and-click.
/// Combines keyboard shortcuts, OCR coordinates, and mouse clicks.
#[derive(Clone)]
pub struct DesktopBrowser;

impl DesktopBrowser {
    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "desktop_browser".into(),
            description: "Control the active browser window with common actions. \
                Use this instead of chaining desktop_key/desktop_type/desktop_click \
                manually. Keyboard/OCR actions work in any browser. CDP actions \
                require Chrome running with --remote-debugging-port=9222.\n\n\
                Keyboard/OCR actions:\n\
                - navigate: Go to a URL (Ctrl+L, type url, Enter, then screenshot+OCR)\n\
                - click_text: Find visible text on the page via OCR and click its center\n\
                - find_and_click: Ctrl+F to search for text, then click the first match\n\
                - new_tab: Open a new browser tab (Ctrl+T)\n\
                - close_tab: Close the current tab (Ctrl+W)\n\
                - refresh: Reload the page (F5)\n\
                - go_back: Navigate back (Alt+Left)\n\
                - go_forward: Navigate forward (Alt+Right)\n\
                - scroll_down / scroll_up: Scroll the page (PageDown/PageUp)\n\
                - read_page: Take a screenshot and run OCR to read page content\n\
                - hover: Move mouse to (x,y) without clicking, then screenshot+OCR\n\
                - hover_text: Find text via OCR, move mouse to it, screenshot\n\n\
                CDP actions (Chrome DevTools Protocol — precise DOM-level ops):\n\
                - cdp_navigate: Navigate via CDP Page.navigate (reliable on SPAs)\n\
                - cdp_click: Click an element by CSS selector (e.g. 'button.ant-btn-primary'). \
                  Works on React/Angular/Vue SPAs where coordinate clicks fail.\n\
                - cdp_read_page: Get full page text via document.body.innerText. \
                  More accurate than OCR for reading content.\n\
                - cdp_evaluate: Execute arbitrary JavaScript in the page and return result.\n\
                - cdp_type: Focus an element by selector and type text into it."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "navigate", "click_text", "find_and_click",
                            "new_tab", "close_tab", "refresh",
                            "go_back", "go_forward",
                            "scroll_down", "scroll_up",
                            "read_page", "hover", "hover_text",
                            "cdp_navigate", "cdp_click", "cdp_read_page",
                            "cdp_evaluate", "cdp_type"
                        ],
                        "description": "Browser action to perform"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL for 'navigate' or 'cdp_navigate' action"
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to find and click (for 'click_text', 'find_and_click', 'hover_text'), or text to type (for 'cdp_type')"
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector for 'cdp_click' (e.g. 'button.submit', '#login-btn') or 'cdp_type' (target input element)"
                    },
                    "js": {
                        "type": "string",
                        "description": "JavaScript expression to evaluate (for 'cdp_evaluate')"
                    },
                    "x": {
                        "type": "integer",
                        "description": "X coordinate for 'hover' action"
                    },
                    "y": {
                        "type": "integer",
                        "description": "Y coordinate for 'hover' action"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        #[cfg(windows)]
        {
            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let t0 = std::time::Instant::now();
            browser_action_windows(action, &arguments)
                .await
                .map(|r| r.with_duration_ms(t0))
        }
        #[cfg(target_os = "linux")]
        {
            let _ = arguments;
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_browser is not supported on Linux yet".into(),
            })
        }
        #[cfg(target_os = "macos")]
        {
            let _ = arguments;
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_browser is not supported on macOS yet".into(),
            })
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_browser is only supported on Windows/macOS/Linux".into(),
            })
        }
    }
}

#[cfg(windows)]
async fn browser_action_windows(
    action: &str,
    args: &serde_json::Value,
) -> anyhow::Result<ToolResult> {
    use std::thread;
    use std::time::Duration;

    let sleep = |ms: u64| thread::sleep(Duration::from_millis(ms));

    /// Helper: press key combo via desktop_key-style names (e.g. ["ctrl", "l"])
    fn combo(keys: &[&str]) -> anyhow::Result<ToolResult> {
        let owned: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
        send_key_combo(&owned)
    }

    /// Helper: screenshot + OCR, return (path, OcrOutput)
    fn snap_ocr(prefix: &str) -> anyhow::Result<(PathBuf, OcrOutput)> {
        let screen_w = unsafe { GetSystemMetrics(0) };
        let screen_h = unsafe { GetSystemMetrics(1) };
        let pixels = unsafe { capture_screen_gdi(screen_w, screen_h) }?;
        // Use absolute path — WinRT FileRandomAccessStream requires absolute paths
        let output_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let path = output_dir.join(format!("stitch_{prefix}_{ts}.bmp"));
        write_bmp(&path, screen_w as u32, screen_h as u32, &pixels)?;
        let ocr = ocr_from_bmp(&path)?;
        Ok((path, ocr))
    }

    /// Helper: click at (cx, cy)
    fn click_at(cx: i32, cy: i32) -> anyhow::Result<()> {
        unsafe {
            send_mouse_move(cx, cy)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        unsafe {
            send_mouse_button("left", true)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
        unsafe {
            send_mouse_button("left", false)?;
        }
        Ok(())
    }

    match action {
        "navigate" => {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'url' for navigate"))?;

            combo(&["ctrl", "l"])?;
            sleep(150);
            type_text_windows(url)?;
            sleep(100);
            unsafe {
                send_vk_key(vk::RETURN, false)?;
            }
            sleep(3000);

            let (_path, ocr) = snap_ocr("browser")?;
            let mut out = format!("Navigated to {url}\n\n--- OCR Text ---\n{}", ocr.full_text);
            format_words(&mut out, &ocr);
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: out,
            })
        }

        "click_text" => {
            let text = args
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'text' for click_text"))?;

            let (_path, ocr) = snap_ocr("click")?;
            let text_lower = text.to_lowercase();

            let target = ocr
                .words
                .iter()
                .find(|w| w.text.to_lowercase() == text_lower)
                .or_else(|| {
                    ocr.words
                        .iter()
                        .find(|w| w.text.to_lowercase().contains(&text_lower))
                });

            match target {
                Some(w) => {
                    let cx = (w.x + w.w / 2) as i32;
                    let cy = (w.y + w.h / 2) as i32;
                    click_at(cx, cy)?;
                    Ok(ToolResult {
                        metrics: None,
                        success: true,
                        output: format!(
                            "Clicked \"{}\" at center=({},{}), bbox=({},{},{},{})",
                            w.text, cx, cy, w.x, w.y, w.w, w.h
                        ),
                    })
                }
                None => {
                    let available: Vec<_> = ocr.words.iter().map(|w| w.text.as_str()).collect();
                    Ok(ToolResult {
                        metrics: None,
                        success: false,
                        output: format!(
                            "Text \"{text}\" not found on screen. Visible words: {}",
                            available.join(", ")
                        ),
                    })
                }
            }
        }

        "find_and_click" => {
            let text = args
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'text' for find_and_click"))?;

            combo(&["ctrl", "f"])?;
            sleep(200);
            type_text_windows(text)?;
            sleep(300);
            unsafe {
                send_vk_key(vk::ESCAPE, false)?;
            }
            sleep(300);

            let (_path, ocr) = snap_ocr("find")?;
            let text_lower = text.to_lowercase();
            let target = ocr
                .words
                .iter()
                .find(|w| w.text.to_lowercase().contains(&text_lower));

            match target {
                Some(w) => {
                    let cx = (w.x + w.w / 2) as i32;
                    let cy = (w.y + w.h / 2) as i32;
                    click_at(cx, cy)?;
                    Ok(ToolResult {
                        metrics: None,
                        success: true,
                        output: format!(
                            "Found and clicked \"{}\" at center=({},{})",
                            w.text, cx, cy
                        ),
                    })
                }
                None => Ok(ToolResult {
                    metrics: None,
                    success: false,
                    output: format!(
                        "After Ctrl+F search, \"{text}\" not found in OCR. Page text: {}",
                        ocr.full_text
                    ),
                }),
            }
        }

        "new_tab" => {
            combo(&["ctrl", "t"])?;
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: "Opened new browser tab (Ctrl+T)".into(),
            })
        }

        "close_tab" => {
            combo(&["ctrl", "w"])?;
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: "Closed current tab (Ctrl+W)".into(),
            })
        }

        "refresh" => {
            unsafe {
                send_vk_key(vk::F5, false)?;
            }
            sleep(1500);
            let (_path, ocr) = snap_ocr("refresh")?;
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: format!("Page refreshed. OCR text:\n{}", ocr.full_text),
            })
        }

        "go_back" => {
            combo(&["alt", "left"])?;
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: "Navigated back (Alt+Left)".into(),
            })
        }

        "go_forward" => {
            combo(&["alt", "right"])?;
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: "Navigated forward (Alt+Right)".into(),
            })
        }

        "scroll_down" => {
            unsafe {
                send_vk_key(vk::NEXT, false)?;
            }
            sleep(200);
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: "Scrolled down (PageDown)".into(),
            })
        }

        "scroll_up" => {
            unsafe {
                send_vk_key(vk::PRIOR, false)?;
            }
            sleep(200);
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: "Scrolled up (PageUp)".into(),
            })
        }

        "read_page" => {
            let (_path, ocr) = snap_ocr("read")?;
            let mut out = format!("--- OCR Text ---\n{}", ocr.full_text);
            format_words(&mut out, &ocr);
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: out,
            })
        }

        "hover" => {
            let x = args
                .get("x")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow::anyhow!("Missing 'x' coordinate for hover"))?;
            let y = args
                .get("y")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow::anyhow!("Missing 'y' coordinate for hover"))?;

            // Move mouse to position without clicking
            unsafe {
                send_mouse_move(x as i32, y as i32)?;
            }
            // Wait for hover-triggered UI to appear (tooltips, dropdowns, etc.)
            sleep(800);

            // Screenshot + OCR to capture any hover-revealed elements
            let (_path, ocr) = snap_ocr("hover")?;
            let mut out = format!(
                "Hovered at ({x}, {y})\n\n--- OCR Text ---\n{}",
                ocr.full_text
            );
            format_words(&mut out, &ocr);
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: out,
            })
        }

        "hover_text" => {
            let text = args
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'text' for hover_text"))?;

            // Screenshot + OCR to find the text position
            let (_path, ocr) = snap_ocr("hoverfind")?;
            let text_lower = text.to_lowercase();
            let target = ocr
                .words
                .iter()
                .find(|w| w.text.to_lowercase() == text_lower)
                .or_else(|| {
                    ocr.words
                        .iter()
                        .find(|w| w.text.to_lowercase().contains(&text_lower))
                });

            match target {
                Some(w) => {
                    let cx = (w.x + w.w / 2) as i32;
                    let cy = (w.y + w.h / 2) as i32;
                    // Move mouse to hover position
                    unsafe {
                        send_mouse_move(cx, cy)?;
                    }
                    sleep(800);
                    // Screenshot to capture hover-triggered UI
                    let (_path2, ocr2) = snap_ocr("hovered")?;
                    let mut out = format!(
                        "Hovered over \"{}\" at ({cx}, {cy})\n\n--- OCR Text ---\n{}",
                        w.text, ocr2.full_text
                    );
                    format_words(&mut out, &ocr2);
                    Ok(ToolResult {
                        metrics: None,
                        success: true,
                        output: out,
                    })
                }
                None => {
                    let available: Vec<_> = ocr.words.iter().map(|w| w.text.as_str()).collect();
                    Ok(ToolResult {
                        metrics: None,
                        success: false,
                        output: format!(
                            "hover_text: \"{text}\" not found. Visible: {}",
                            available.join(", ")
                        ),
                    })
                }
            }
        }

        "cdp_navigate" => {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'url' for cdp_navigate"))?;
            let mut client = crate::tools::cdp::CdpClient::connect(9222, None).await?;
            let out = client.navigate(url).await?;
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: out,
            })
        }

        "cdp_click" => {
            let selector = args
                .get("selector")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'selector' for cdp_click"))?;
            let mut client = crate::tools::cdp::CdpClient::connect(9222, None).await?;
            let out = client.click(selector).await?;
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: out,
            })
        }

        "cdp_read_page" => {
            let mut client = crate::tools::cdp::CdpClient::connect(9222, None).await?;
            let text = client.read_page_text().await?;
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: format!("--- Page DOM Text ---\n{text}"),
            })
        }

        "cdp_evaluate" => {
            let js = args
                .get("js")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'js' for cdp_evaluate"))?;
            let mut client = crate::tools::cdp::CdpClient::connect(9222, None).await?;
            let out = client.evaluate(js).await?;
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: out,
            })
        }

        "cdp_type" => {
            let selector = args
                .get("selector")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'selector' for cdp_type"))?;
            let text = args
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'text' for cdp_type"))?;
            let mut client = crate::tools::cdp::CdpClient::connect(9222, None).await?;
            let out = client.type_into(selector, text).await?;
            Ok(ToolResult {
                metrics: None,
                success: true,
                output: out,
            })
        }

        _ => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("Unknown browser action: {action}"),
        }),
    }
}

/// Append per-word bounding-box lines to the output string.
#[cfg(windows)]
fn format_words(out: &mut String, ocr: &OcrOutput) {
    if !ocr.words.is_empty() {
        out.push_str("\n\n--- OCR Words (click targets) ---");
        for w in &ocr.words {
            let cx = w.x + w.w / 2;
            let cy = w.y + w.h / 2;
            out.push_str(&format!(
                "\n\"{}\" bbox=({},{},{},{}) center=({},{})",
                w.text, w.x, w.y, w.w, w.h, cx, cy
            ));
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

// ── desktop_app_launch ──────────────────────────────────────────

/// Launch a Windows application by name or path via ShellExecuteW.
#[derive(Clone)]
pub struct DesktopAppLaunch;

impl DesktopAppLaunch {
    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "desktop_app_launch".into(),
            description: "Launch a Windows application by executable name or full path. \
                 Common names: excel, notepad, calc, mspaint, cmd. \
                 Pass optional command-line arguments via args."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "app": {
                        "type": "string",
                        "description": "Executable name (e.g. excel, notepad) or full path"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional command-line arguments to pass to the app"
                    }
                },
                "required": ["app"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        #[cfg(windows)]
        {
            let app = arguments["app"].as_str().unwrap_or("");
            let args: Vec<String> = arguments["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            launch_app_windows(app, &args)
        }
        #[cfg(target_os = "linux")]
        {
            let app = arguments["app"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'app' argument"))?;
            let args = arguments["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|x| x.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            launch_app_linux(app, &args)
        }
        #[cfg(target_os = "macos")]
        {
            let app = arguments["app"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'app' argument"))?;
            launch_app_macos(app)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult {
                metrics: None,
                success: false,
                output: "desktop_app_launch is only supported on Windows/macOS/Linux".into(),
            })
        }
    }
}

/// Linux 启动应用：xdg-open（按默认应用打开）。
#[cfg(target_os = "linux")]
fn launch_app_linux(app: &str, _args: &[String]) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let out = Command::new("xdg-open").arg(app).output();
    match out {
        Ok(o) if o.status.success() => Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!("Launched {app}"),
        }),
        Ok(o) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("启动失败：{}", String::from_utf8_lossy(&o.stderr)),
        }),
        Err(e) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("启动失败：{e}（xdg-open 不可用）"),
        }),
    }
}

/// macOS 启动应用：open -a。
#[cfg(target_os = "macos")]
fn launch_app_macos(app: &str) -> anyhow::Result<ToolResult> {
    use std::process::Command;
    let out = Command::new("open").args(["-a", app]).output();
    match out {
        Ok(o) if o.status.success() => Ok(ToolResult {
            metrics: None,
            success: true,
            output: format!("Launched {app}"),
        }),
        Ok(o) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("启动失败：{}", String::from_utf8_lossy(&o.stderr)),
        }),
        Err(e) => Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("启动失败：{e}"),
        }),
    }
}

#[cfg(windows)]
fn resolve_app(app: &str) -> Option<String> {
    // If it's a full path to an existing file, use as-is.
    let p = std::path::Path::new(app);
    if p.is_absolute() && p.is_file() {
        return Some(app.to_string());
    }
    // Common aliases.
    let candidates: &[&str] = match app.to_ascii_lowercase().as_str() {
        "excel" => &[
            r"C:\Program Files\Microsoft Office\root\Office16\EXCEL.EXE",
            r"C:\Program Files (x86)\Microsoft Office\root\Office16\EXCEL.EXE",
        ],
        "notepad" => &[
            r"C:\Windows\System32\notepad.exe",
            r"C:\Windows\notepad.exe",
        ],
        "calc" | "calculator" => &[r"C:\Windows\System32\calc.exe"],
        "mspaint" | "paint" => &[r"C:\Windows\System32\mspaint.exe"],
        "cmd" => &[r"C:\Windows\System32\cmd.exe"],
        _ => {
            // Try appending .exe and searching PATH.
            let with_exe = if app.ends_with(".exe") {
                app.to_string()
            } else {
                format!("{app}.exe")
            };
            return Some(with_exe); // Let ShellExecuteW try PATH
        }
    };
    for path in candidates {
        if std::path::Path::new(path).is_file() {
            return Some((*path).to_string());
        }
    }
    // Fall back to the name with .exe appended.
    Some(if app.ends_with(".exe") {
        app.to_string()
    } else {
        format!("{app}.exe")
    })
}

#[cfg(windows)]
#[allow(non_snake_case)]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: isize,
        lpOperation: *const u16,
        lpFile: *const u16,
        lpParameters: *const u16,
        lpDirectory: *const u16,
        nShowCmd: i32,
    ) -> isize;
}

#[cfg(windows)]
fn launch_app_windows(app: &str, args: &[String]) -> anyhow::Result<ToolResult> {
    let t0 = std::time::Instant::now();
    let resolved =
        resolve_app(app).ok_or_else(|| anyhow::anyhow!("Could not resolve application: {app}"))?;
    let params = args.join(" ");
    let app_wide: Vec<u16> = resolved.encode_utf16().chain(std::iter::once(0)).collect();
    let params_wide: Vec<u16> = if params.is_empty() {
        vec![0]
    } else {
        params.encode_utf16().chain(std::iter::once(0)).collect()
    };
    let op_open: Vec<u16> = "open\0".encode_utf16().collect();

    // SAFETY: ShellExecuteW with valid null-terminated UTF-16 strings.
    let ret = unsafe {
        ShellExecuteW(
            0,                    // hwnd
            op_open.as_ptr(),     // operation = "open"
            app_wide.as_ptr(),    // file
            params_wide.as_ptr(), // parameters
            std::ptr::null(),     // directory
            1,                    // SW_SHOWNORMAL
        )
    } as isize;

    if ret > 32 {
        let mut metrics = std::collections::HashMap::new();
        metrics.insert("duration_ms".into(), t0.elapsed().as_secs_f64() * 1000.0);
        Ok(ToolResult {
            success: true,
            output: format!(
                "Launched {app}{extra}",
                extra = if params.is_empty() {
                    String::new()
                } else {
                    format!(" with args: {params}")
                }
            ),
            metrics: Some(metrics),
        })
    } else {
        let hint = match ret {
            2 => "file not found".into(),
            3 => "path not found".into(),
            5 => "access denied".into(),
            11 => "out of memory (invalid exe format)".into(),
            _ => format!("ShellExecuteW returned {ret}"),
        };
        Ok(ToolResult {
            metrics: None,
            success: false,
            output: format!("Could not launch {app}: {hint}"),
        })
    }
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn desktop_window_list_finds_windows() {
        let result = window_list_windows().expect("window_list_windows");
        assert!(result.success, "output={}", result.output);
        // Should find at least the current shell/IDE window
        assert!(
            !result.output.contains("no visible windows"),
            "should find some windows: {}",
            result.output
        );
    }

    #[test]
    fn desktop_screenshot_creates_bmp() {
        let tmp = std::env::temp_dir();
        let result = screenshot_windows(&tmp, false).expect("screenshot_windows");
        assert!(result.success, "output={}", result.output);
        // Find the BMP file that was just created
        let bmps: Vec<_> = std::fs::read_dir(&tmp)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("stitch_desktop_") && n.ends_with(".bmp"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(!bmps.is_empty(), "no BMP found in {:?}", tmp);
        // BMP header: "BM" magic
        let bmp_path = bmps[0].path();
        let header = std::fs::read(&bmp_path).unwrap();
        assert_eq!(&header[0..2], b"BM", "BMP magic missing");
        // Cleanup
        let _ = std::fs::remove_file(&bmp_path);
    }

    #[test]
    fn desktop_click_noop_positions() {
        // Test that click_* functions produce valid input structures without
        // actually moving to dangerous positions (we test the struct construction).
        // Full click test is in integration since it moves the actual cursor.
        let input_size = std::mem::size_of::<INPUT>();
        assert!(input_size > 0, "INPUT struct should have non-zero size");
        assert_eq!(std::mem::size_of::<MOUSEINPUT>(), 32);
        assert_eq!(std::mem::size_of::<KEYBDINPUT>(), 24);
    }
}
