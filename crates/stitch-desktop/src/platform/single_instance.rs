//! Single-instance enforcement via named Win32 mutex.
//!
//! On second launch, brings the existing window to front and
//! shows a message box, then exits.
//!
//! WebDriver e2e builds skip the lock — a leftover tray instance would otherwise
//! show a modal MessageBox and kill the WDIO session before any assertion runs.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(attrs: *mut std::ffi::c_void, owner: i32, name: *const u16) -> isize;
    fn GetLastError() -> u32;
    fn CloseHandle(handle: isize) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, flags: u32) -> i32;
    fn FindWindowW(class: *const u16, title: *const u16) -> isize;
    fn SetForegroundWindow(hwnd: isize) -> i32;
    fn IsIconic(hwnd: isize) -> i32;
    fn ShowWindowAsync(hwnd: isize, cmd: i32) -> i32;
}

const ERROR_ALREADY_EXISTS: u32 = 183;
const SW_RESTORE: i32 = 9;
const MB_OK: u32 = 0;
const MB_ICONINFORMATION: u32 = 0x40;

fn bring_existing_to_front() {
    let wtitle: Vec<u16> = OsStr::new("Stitch")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), wtitle.as_ptr());
        if hwnd != 0 {
            if IsIconic(hwnd) != 0 {
                ShowWindowAsync(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
    }
}

pub fn try_acquire() -> bool {
    // WDIO / accept harness: never block on a modal or exit for a second instance.
    if cfg!(feature = "webdriver") {
        return true;
    }

    let mutex_name: Vec<u16> = OsStr::new("StitchPromptStdioAgent_SingleInstance")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let handle = CreateMutexW(std::ptr::null_mut(), 1, mutex_name.as_ptr());
        if handle == 0 {
            return true;
        }

        if GetLastError() == ERROR_ALREADY_EXISTS {
            bring_existing_to_front();

            let title: Vec<u16> = OsStr::new("Stitch")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let msg: Vec<u16> = OsStr::new("Stitch 已在运行中，请查看系统托盘。")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            MessageBoxW(0, msg.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);

            CloseHandle(handle);
            return false;
        }

        let _ = handle;
    }

    true
}
