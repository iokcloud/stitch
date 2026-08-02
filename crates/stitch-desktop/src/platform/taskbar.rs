//! Windows taskbar progress bar (ITaskbarList3 COM interface).

use std::ffi::c_void;
use tauri::{AppHandle, Manager};
use windows::Win32::UI::Shell::{ITaskbarList3, TBPFLAG, TaskbarList};
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor};

const TBPF_NOPROGRESS: i32 = 0x0;
const TBPF_NORMAL: i32 = 0x2;

/// Resolve the root owner HWND from a child HWND.
fn root_hwnd(hwnd: isize) -> isize {
    let child = windows::Win32::Foundation::HWND(hwnd as *mut c_void);
    let root = unsafe { GetAncestor(child, GA_ROOT) };
    if root.0.is_null() {
        hwnd
    } else {
        root.0 as isize
    }
}

/// Set taskbar progress state (indeterminate / normal / off).
pub fn set_progress(hwnd: isize, state: i32) {
    let target = root_hwnd(hwnd);
    init_com();
    if let Ok(tb) = create_taskbar() {
        let tw32 = windows::Win32::Foundation::HWND(target as *mut c_void);
        let _ = unsafe { tb.SetProgressState(tw32, TBPFLAG(state)) };
    }
}

/// Animate the taskbar progress bar from 0 % → 90 % over ~2 s,
/// bridging the WebView2 cold-start gap.
pub fn start_animation(hwnd: isize) {
    let target_hwnd = root_hwnd(hwnd);

    std::thread::spawn(move || {
        init_com();

        for i in 0..=90u64 {
            std::thread::sleep(std::time::Duration::from_millis(20));

            if let Ok(tb) = create_taskbar() {
                let tw32 = windows::Win32::Foundation::HWND(target_hwnd as *mut c_void);
                if i == 0 {
                    let _ = unsafe { tb.SetProgressState(tw32, TBPFLAG(TBPF_NORMAL)) };
                }
                let _ = unsafe { tb.SetProgressValue(tw32, i, 100) };
            }
        }
    });
}

/// Clear the taskbar progress (called after WebView2 is ready).
pub fn clear(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    set_progress(hwnd.0 as isize, TBPF_NOPROGRESS);
}

fn init_com() {
    let _ = unsafe {
        windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        )
    };
}

fn create_taskbar() -> Result<ITaskbarList3, windows::core::Error> {
    unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &TaskbarList,
            None,
            windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
        )
    }
}
