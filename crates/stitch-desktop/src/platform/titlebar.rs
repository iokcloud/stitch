//! DWM title-bar theme synchronisation.

use std::ffi::c_void;
use tauri::{AppHandle, Manager};

unsafe extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: isize,
        dwAttribute: u32,
        pvAttribute: *const c_void,
        cbAttribute: u32,
    ) -> i32;
}

pub fn set_theme(app: &AppHandle, dark: bool) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd_raw = hwnd.0 as isize;

    let dark_val: i32 = if dark { 1 } else { 0 };
    // Try DWMWA_USE_IMMERSIVE_DARK_MODE (20) first, fall back to
    // DWMWA_USE_IMMERSIVE_DARK_MODE_BEFORE_20H1 (19).
    for attr in [20u32, 19u32] {
        let r = unsafe {
            DwmSetWindowAttribute(
                hwnd_raw,
                attr,
                &dark_val as *const i32 as *const c_void,
                std::mem::size_of::<i32>() as u32,
            )
        };
        if r == 0 {
            break;
        }
    }

    // DWMWA_CAPTION_COLOR (35) — 深色标题栏底色与主题统一（纯黑基底）
    let color: u32 = if dark { 0x00000000 } else { 0x00ffffff };
    unsafe {
        DwmSetWindowAttribute(
            hwnd_raw,
            35u32,
            &color as *const u32 as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }

    if dark {
        let _ = window.set_background_color(Some(tauri::webview::Color(0x00, 0x00, 0x00, 0xff)));
    } else {
        let _ = window.set_background_color(Some(tauri::webview::Color(0xff, 0xff, 0xff, 0xff)));
    }

    tracing::info!(dark, "title bar + background synced");
}
