//! Tauri entry point — bootstraps the desktop window and registers IPC commands.
//!
//! `serde_json::json!` and `tauri::generate_context!` macros use `unwrap()` — allowed.
#![allow(clippy::disallowed_methods)]
#![windows_subsystem = "windows"]

mod account_connect;
mod commands;
mod tray;

use commands::apply_startup_window_state;

#[cfg(target_os = "windows")]
mod splash_win;

mod platform;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
use tracing_subscriber::EnvFilter;

fn main() {
    // ── Single-instance lock ────────────────────────────────────
    if !platform::try_acquire() {
        std::process::exit(0);
    }

    // Enable WebView2 remote debugging for CDP automation
    #[cfg(target_os = "windows")]
    unsafe {
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "--remote-debugging-port=9222",
        );
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .without_time()
        .init();

    let builder = {
        let b = tauri::Builder::default()
            .plugin(tauri_plugin_shell::init())
            .plugin(tauri_plugin_updater::Builder::new().build());
        // Embedded WebDriver HTTP server for WDIO smoke (feature `webdriver` only).
        #[cfg(feature = "webdriver")]
        let b = b.plugin(tauri_plugin_wdio_webdriver::init());
        b
    };

    let app = builder
        .setup(|app| {
            // Restore the user's last window geometry before first paint.
            apply_startup_window_state(app.handle());

            // Build system tray
            tray::build_tray(app.handle())?;

            #[cfg(target_os = "windows")]
            if let Some(webview) = app.get_webview_window("main")
                && let Ok(hwnd) = webview.hwnd()
            {
                platform::start_animation(hwnd.0 as isize);
                splash_win::show(hwnd.0 as isize);
            }

            // ── Shared cleanup gate ────────────────────────────────
            let window_shown = Arc::new(AtomicBool::new(false));

            // ── Title-bar spinner animation ─────────────────────────
            let finish_flag = Arc::new(AtomicBool::new(false));
            let spinner_flag = finish_flag.clone();
            let spinner_handle = app.handle().clone();

            std::thread::spawn(move || {
                let spinner = [
                    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}",
                    "\u{2826}", "\u{2827}", "\u{2807}", "\u{280f}",
                ];
                let mut i = 0;
                while !spinner_flag.load(Ordering::SeqCst) {
                    if let Some(w) = spinner_handle.get_webview_window("main") {
                        let _ = w.set_title(&format!(
                            "Stitch \u{2014} PromptStdio Agent  {}",
                            spinner[i]
                        ));
                    }
                    i = (i + 1) % spinner.len();
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
                if let Some(w) = spinner_handle.get_webview_window("main") {
                    let _ = w.set_title("Stitch \u{2014} PromptStdio Agent");
                }
            });

            // ── Auto-show safety timeout ───────────────────────────
            let auto_handle = app.handle().clone();
            let auto_shown = window_shown.clone();
            let auto_finish = finish_flag.clone();

            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));

                if auto_shown.load(Ordering::SeqCst) {
                    return;
                }

                // Stop the title-bar spinner
                auto_finish.store(true, Ordering::SeqCst);

                // platform::finish_splash_and_show is idempotent via
                // window_shown compare_exchange — if JS already won
                // the race, this becomes a no-op.
                if let Err(e) = platform::finish_splash_and_show(&auto_handle, false, &auto_shown) {
                    tracing::warn!("safety timeout show failed: {e}");
                    // Force-show as last resort
                    auto_shown.store(true, Ordering::SeqCst);
                    if let Some(window) = auto_handle.get_webview_window("main") {
                        let _ = window.show();
                    }
                }

                platform::clear(&auto_handle);
                tracing::info!("auto-show timeout: window revealed (JS-independent fallback)");
            });

            app.manage(commands::StartupState {
                finished: finish_flag,
                window_shown,
            });

            Ok(())
        })
        .manage(commands::CancelState::default())
        .manage(commands::ConfirmState::default())
        .manage(commands::PlanState::default())
        .manage(commands::WorkDirState::new())
        .manage(commands::AgentSessionStore::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::upsert_llm_profile,
            commands::delete_llm_profile,
            commands::set_active_llm_profile,
            commands::upsert_mcp_profile,
            commands::delete_mcp_profile,
            commands::set_active_mcp_profile,
            commands::clear_mcp_profile_token,
            commands::upsert_mcp_server,
            commands::import_mcp_servers,
            commands::add_promptstdio_mcp_preset,
            commands::delete_mcp_server,
            commands::set_mcp_server_enabled,
            commands::test_mcp_server,
            commands::get_work_dir,
            commands::set_work_dir,
            commands::browse_work_dir,
            commands::open_folder_path,
            commands::send_message,
            commands::clear_agent_session,
            commands::drop_agent_memory,
            commands::list_session_checkpoints,
            commands::diff_session_checkpoints,
            commands::rollback_session_epoch,
            commands::gc_orphan_agent_sessions,
            commands::latest_workspace_checkpoint,
            commands::cancel_generation,
            commands::respond_confirmation,
            commands::get_allow_rules,
            commands::remove_allow_rule,
            commands::clear_allow_rules,
            commands::respond_plan,
            commands::list_suites,
            commands::list_agents,
            commands::create_prompt,
            commands::submit_explore,
            commands::track_usage,
            commands::get_membership,
            commands::open_external_url,
            account_connect::start_account_connect,
            commands::run_suite,
            commands::run_agent,
            commands::test_connection,
            commands::test_promptstdio,
            commands::list_skills,
            commands::list_local_skills,
            commands::export_skill,
            commands::set_titlebar_theme,
            commands::clear_taskbar_progress,
            commands::finish_startup,
            commands::check_update,
            commands::install_update,
            commands::frontend_log,
            commands::save_window_state,
            commands::set_compact_mode,
            commands::snap_compact_window,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Stitch desktop");

    app.run(|_app_handle, _event| {});
}
