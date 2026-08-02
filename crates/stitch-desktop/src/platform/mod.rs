//! Platform abstraction layer.
//!
//! Windows implementations live in sub-modules gated behind
//! `#[cfg(target_os = "windows")]`.  Non-Windows targets get
//! no-op stubs so calling code remains platform-agnostic.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
#[cfg_attr(not(target_os = "windows"), allow(unused_imports))]
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
mod single_instance;
#[cfg(target_os = "windows")]
pub use single_instance::try_acquire;

#[cfg(not(target_os = "windows"))]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))] // 非 Windows stub（调用点在 Windows 分支）
pub fn try_acquire() -> bool {
    true
}

#[cfg(target_os = "windows")]
mod titlebar;
#[cfg(target_os = "windows")]
pub use titlebar::set_theme;

#[cfg(not(target_os = "windows"))]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn set_theme(_app: &AppHandle, _dark: bool) {}

#[cfg(target_os = "windows")]
mod taskbar;
#[cfg(target_os = "windows")]
pub use taskbar::{clear, start_animation};

#[cfg(not(target_os = "windows"))]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn set_progress(_hwnd: isize, _state: i32) {}
#[cfg(not(target_os = "windows"))]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn start_animation(_hwnd: isize) {}
#[cfg(not(target_os = "windows"))]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn clear(_app: &AppHandle) {}

/// Hide the native splash overlay and reveal the main window.
/// Idempotent: only the first caller performs the transition.
#[cfg(target_os = "windows")]
pub fn finish_splash_and_show(
    app: &AppHandle,
    dark: bool,
    window_shown: &Arc<AtomicBool>,
) -> Result<(), String> {
    use std::ffi::c_void;
    use std::sync::atomic::Ordering;
    use windows::Win32::UI::WindowsAndMessaging::{AW_BLEND, AW_HIDE, AnimateWindow};

    if window_shown
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        tracing::info!("finish_startup: window already shown (timeout path)");
        return Ok(());
    }

    // Set background colour before showing
    if let Some(window) = app.get_webview_window("main") {
        if dark {
            let _ =
                window.set_background_color(Some(tauri::webview::Color(0x0f, 0x17, 0x2a, 0xff)));
        } else {
            let _ =
                window.set_background_color(Some(tauri::webview::Color(0xff, 0xff, 0xff, 0xff)));
        }
    }

    let splash_hwnd_val = crate::splash_win::splash_hwnd();
    if splash_hwnd_val != 0 {
        let splash = windows::Win32::Foundation::HWND(splash_hwnd_val as *mut c_void);
        unsafe {
            let _ = AnimateWindow(splash, 300, AW_BLEND | AW_HIDE);
        };
        std::thread::sleep(std::time::Duration::from_millis(350));
        crate::splash_win::hide();
    }

    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn finish_splash_and_show(
    app: &AppHandle,
    dark: bool,
    window_shown: &Arc<AtomicBool>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    if window_shown
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = dark;
        window.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}
