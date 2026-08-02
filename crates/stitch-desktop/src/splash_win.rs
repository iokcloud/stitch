//! Win32 splash overlay — transparent logo surrounded by a dot-ring
//! track (24 white dots) with a rotating blue arc (8 dots, tail-faded).
//! Logo breathes in sync with the HTML loader's pulse animation.
//!
//! Filled-circle dots avoid the aliasing inherent in GDI line primitives.
//! Chroma-key: magenta (0xFF00FF) → transparent.  Pure GDI — no gdiplus.

#![allow(unsafe_op_in_unsafe_fn, clippy::upper_case_acronyms)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};

// ── FFI ──────────────────────────────────────────────────────────

#[link(name = "user32")]
unsafe extern "system" {
    fn CreateWindowExW(
        exstyle: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: isize,
        menu: isize,
        instance: isize,
        param: *const c_void,
    ) -> isize;
    fn DestroyWindow(hwnd: isize) -> i32;
    fn GetClientRect(hwnd: isize, rect: *mut RECT) -> i32;
    fn BeginPaint(hwnd: isize, ps: *mut PAINTSTRUCT) -> isize;
    fn EndPaint(hwnd: isize, ps: *const PAINTSTRUCT) -> i32;
    fn FillRect(hdc: isize, rect: *const RECT, brush: isize) -> i32;
    fn InvalidateRect(hwnd: isize, rect: *const RECT, erase: i32) -> i32;
    fn RegisterClassW(cls: *const WNDCLASSW) -> u16;
    fn DefWindowProcW(hwnd: isize, msg: u32, wp: usize, lp: isize) -> isize;
    fn GetModuleHandleW(name: *const u16) -> isize;
    fn GetSystemMetrics(index: i32) -> i32;
    fn SetLayeredWindowAttributes(hwnd: isize, cr_key: u32, b_alpha: u8, dw_flags: u32) -> i32;
    fn SetTimer(hwnd: isize, id: usize, elapse: u32, cb: isize) -> usize;
    fn KillTimer(hwnd: isize, id: usize) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> isize;
    fn DeleteObject(obj: isize) -> i32;
    fn CreatePen(style: i32, width: i32, color: u32) -> isize;
    fn SelectObject(hdc: isize, obj: isize) -> isize;
    fn MoveToEx(hdc: isize, x: i32, y: i32, pt: *mut POINT) -> i32;
    fn LineTo(hdc: isize, x: i32, y: i32) -> i32;
    fn Polygon(hdc: isize, pts: *const POINT, count: i32) -> i32;
    fn Ellipse(hdc: isize, left: i32, top: i32, right: i32, bottom: i32) -> i32;
    fn GetStockObject(index: i32) -> isize;
}

// ── Types & constants ─────────────────────────────────────────────

#[repr(C)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct POINT {
    x: i32,
    y: i32,
}

#[repr(C)]
struct PAINTSTRUCT {
    hdc: isize,
    f_erase: i32,
    rc_paint: RECT,
    f_restore: i32,
    f_inc_update: i32,
    rgb_reserved: [u8; 32],
}

#[repr(C)]
struct WNDCLASSW {
    style: u32,
    lpfn_wnd_proc: isize,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: isize,
    h_icon: isize,
    h_cursor: isize,
    hbr_background: isize,
    menu_name: *const u16,
    class_name: *const u16,
}

const WS_POPUP: u32 = 0x80000000;
const WS_VISIBLE: u32 = 0x10000000;
const WS_EX_LAYERED: u32 = 0x00080000;
const WS_EX_TOOLWINDOW: u32 = 0x00000080;
const WS_EX_TOPMOST: u32 = 0x00000008;
const LWA_COLORKEY: u32 = 0x1;
const COLORKEY: u32 = 0x00FF00FF;
const WM_PAINT: u32 = 0x000F;
const WM_TIMER: u32 = 0x0113;
const WM_DESTROY: u32 = 0x0002;
const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;
const NULL_PEN: i32 = 8;

/// Total dots on the ring track.
const DOT_COUNT: usize = 24;
/// How many dots are lit blue (the rotating arc).
const ARC_DOTS: usize = 8;
/// Dot diameter in pixels.
const DOT_SZ: i32 = 6;
/// Logo breathing cycle length in timer ticks (200 ms each → ~3 s).
const BREATHE_TICKS: f64 = 15.0;

// ── State ────────────────────────────────────────────────────────

static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);
/// Tick counter — drives rotation + breathing animation.
static ARC_TICK: AtomicIsize = AtomicIsize::new(0);

// ── Public API ───────────────────────────────────────────────────

pub fn show(_parent_hwnd: isize) {
    let class_name: Vec<u16> = "StitchSplashOverlay\0".encode_utf16().collect();
    let inst = unsafe { GetModuleHandleW(std::ptr::null()) };
    let wc = WNDCLASSW {
        style: 0,
        lpfn_wnd_proc: overlay_wnd_proc as *const () as isize,
        cb_cls_extra: 0,
        cb_wnd_extra: 0,
        h_instance: inst,
        h_icon: 0,
        h_cursor: 0,
        hbr_background: 0,
        menu_name: std::ptr::null(),
        class_name: class_name.as_ptr(),
    };
    let _atom = unsafe { RegisterClassW(&wc) };

    let o_w = 220i32;
    let o_h = 250i32;

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let x = (screen_w - o_w) / 2;
    let y = (screen_h - o_h) / 2;

    let title: Vec<u16> = "\0".encode_utf16().collect();
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            x,
            y,
            o_w,
            o_h,
            0,
            0,
            inst,
            std::ptr::null(),
        )
    };

    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, COLORKEY, 0, LWA_COLORKEY);
    }

    OVERLAY_HWND.store(hwnd, Ordering::SeqCst);
    // One dot advance per tick (200 ms) → full rotation in ~4.8 s
    unsafe { SetTimer(hwnd, 1, 200, 0) };
}

pub fn splash_hwnd() -> isize {
    OVERLAY_HWND.load(Ordering::SeqCst)
}

pub fn hide() {
    let hwnd = OVERLAY_HWND.swap(0, Ordering::SeqCst);
    if hwnd != 0 {
        unsafe { DestroyWindow(hwnd) };
    }
}

// ── Window proc ──────────────────────────────────────────────────

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: isize,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    match msg {
        WM_PAINT => {
            draw_overlay(hwnd);
            return 0;
        }
        WM_TIMER => {
            ARC_TICK.fetch_add(1, Ordering::SeqCst);
            unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
            return 0;
        }
        WM_DESTROY => {
            unsafe { KillTimer(hwnd, 1) };
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

// ── Drawing ──────────────────────────────────────────────────────

/// Breathing factor — oscillates 0.75 → 1.0 with a ~3 s period,
/// matching the HTML loader's `splash-pulse` keyframe.
fn breathe_factor(tick: usize) -> f64 {
    let phase = tick as f64 * 2.0 * std::f64::consts::PI / BREATHE_TICKS;
    let s = (phase.sin() + 1.0) / 2.0; // 0.0 → 1.0
    0.75 + 0.25 * s // 0.75 → 1.0
}

/// Blend a colour channel by a 0.0–1.0 factor.
fn ch(v: u8, f: f64) -> u8 {
    (v as f64 * f).round() as u8
}

/// Pack R, G, B into a GDI COLORREF (0x00_BB_GG_RR).
fn bgr(r: u8, g: u8, b: u8) -> u32 {
    (b as u32) << 16 | (g as u32) << 8 | r as u32
}

fn draw_overlay(hwnd: isize) {
    let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };

    let mut rc: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(hwnd, &mut rc) };

    // Fill with chroma-key (magenta → transparent)
    let bg = unsafe { CreateSolidBrush(COLORKEY) };
    unsafe { FillRect(hdc, &rc, bg) };
    unsafe { DeleteObject(bg) };

    let w = rc.right - rc.left; // 220
    let h = rc.bottom - rc.top; // 250
    let cx = w / 2; // 110
    let cy = h / 2; // 125

    let tick = ARC_TICK.load(Ordering::SeqCst) as usize;

    // ── Dot ring track + rotating arc ──────────────────────────
    let ring_r: i32 = 92;
    let start_dot = tick % DOT_COUNT;
    let half = DOT_SZ / 2; // 3

    // Pre-compute dot positions (12 o'clock origin)
    let mut dots: [(i32, i32); DOT_COUNT] = [(0, 0); DOT_COUNT];
    for (i, dot) in dots.iter_mut().enumerate() {
        let theta =
            2.0 * std::f64::consts::PI * i as f64 / DOT_COUNT as f64 - std::f64::consts::PI / 2.0;
        *dot = (
            (cx as f64 + ring_r as f64 * theta.cos()) as i32,
            (cy as f64 + ring_r as f64 * theta.sin()) as i32,
        );
    }

    // Null pen → filled dots with no outline
    let null_pen = unsafe { GetStockObject(NULL_PEN) };
    let pen_saved = unsafe { SelectObject(hdc, null_pen) };

    // --- all 24 white track dots ---
    let white_brush = unsafe { CreateSolidBrush(0x00ffffff) };
    let brush_saved = unsafe { SelectObject(hdc, white_brush) };
    for (dx, dy) in &dots {
        unsafe {
            Ellipse(hdc, dx - half, dy - half, dx + half + 1, dy + half + 1);
        }
    }
    unsafe { SelectObject(hdc, brush_saved) };
    unsafe { DeleteObject(white_brush) };

    // --- 8 blue arc dots with tail fade ---
    // Dot 0 = leading edge (brightest), Dot 7 = trailing edge (dimmest).
    for i in 0..ARC_DOTS {
        let idx = (start_dot + i) % DOT_COUNT;
        let (dx, dy) = dots[idx];

        // Tail fade: 1.0 → 0.35
        let t = i as f64 / (ARC_DOTS - 1) as f64;
        let factor = 1.0 - 0.65 * t;

        let r = 0u8;
        let g = ch(123, factor);
        let b = ch(255, factor);
        let dot_brush = unsafe { CreateSolidBrush(bgr(r, g, b)) };
        let prev = unsafe { SelectObject(hdc, dot_brush) };
        unsafe {
            Ellipse(hdc, dx - half, dy - half, dx + half + 1, dy + half + 1);
        }
        unsafe { SelectObject(hdc, prev) };
        unsafe { DeleteObject(dot_brush) };
    }

    // Restore pen
    unsafe { SelectObject(hdc, pen_saved) };

    // ── Logo icon (GDI, with breathing pulse) ─────────────────
    let breathe = breathe_factor(tick);

    let logo_sz: i32 = 170;
    let lx = (w - logo_sz) / 2;
    let ly = (h - logo_sz) / 2;
    let scale = 5.3125f64;
    let sx = |v: i32| -> i32 { lx + (v as f64 * scale) as i32 };
    let sy = |v: i32| -> i32 { ly + (v as f64 * scale) as i32 };

    // White text lines (breathing)
    let wv = ch(255, breathe);
    let white_pen = unsafe { CreatePen(0, 5, bgr(wv, wv, wv)) };
    let p_saved = unsafe { SelectObject(hdc, white_pen) };
    unsafe { MoveToEx(hdc, sx(8), sy(11), std::ptr::null_mut()) };
    unsafe { LineTo(hdc, sx(20), sy(11)) };
    unsafe { MoveToEx(hdc, sx(8), sy(16), std::ptr::null_mut()) };
    unsafe { LineTo(hdc, sx(17), sy(16)) };
    unsafe { MoveToEx(hdc, sx(8), sy(21), std::ptr::null_mut()) };
    unsafe { LineTo(hdc, sx(14), sy(21)) };
    unsafe { SelectObject(hdc, p_saved) };
    unsafe { DeleteObject(white_pen) };

    // Blue play button (breathing, filled)
    let bv = ch(255, breathe);
    let gv = ch(123, breathe);
    let blue_pen = unsafe { CreatePen(0, 5, bgr(0, gv, bv)) };
    unsafe { SelectObject(hdc, blue_pen) };
    let blue_brush = unsafe { CreateSolidBrush(bgr(0, gv, bv)) };
    let b_saved = unsafe { SelectObject(hdc, blue_brush) };
    let tri = [
        POINT {
            x: sx(21),
            y: sy(19),
        },
        POINT {
            x: sx(26),
            y: sy(22),
        },
        POINT {
            x: sx(21),
            y: sy(25),
        },
    ];
    unsafe { Polygon(hdc, tri.as_ptr(), 3) };
    unsafe { SelectObject(hdc, b_saved) };
    unsafe { DeleteObject(blue_brush) };
    unsafe { SelectObject(hdc, p_saved) };
    unsafe { DeleteObject(blue_pen) };

    unsafe { EndPaint(hwnd, &ps) };
}
