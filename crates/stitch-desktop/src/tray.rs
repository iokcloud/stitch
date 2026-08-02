//! System tray with minimize-to-tray and right-click menu.
//!
//! On window close, the app hides to tray instead of quitting.
//! Right-click the tray icon to show/hide or quit.

use tauri::{
    AppHandle, Manager,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

/// Build the system tray icon and menu.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItemBuilder::with_id("show", "显示 Stitch").build(app)?;
    let hide_item = MenuItemBuilder::with_id("hide", "隐藏到托盘").build(app)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .item(&hide_item)
        .item(&separator)
        .item(&quit_item)
        .build()?;

    // Load and decode the tray icon from embedded PNG bytes
    let icon_bytes = include_bytes!("../icons/icon.png");
    let img = image::load_from_memory(icon_bytes)
        .expect("Failed to decode tray icon")
        .into_rgba8();
    let (width, height) = img.dimensions();
    let rgba = img.into_raw();

    let icon = tauri::image::Image::new_owned(rgba, width, height);

    let _tray = TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("Stitch \u{2014} PromptStdio Agent")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            "quit" => {
                app.exit(0);
                // Belt and suspenders: force exit after a short grace
                // period in case the event loop blocks.
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    std::process::exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}
