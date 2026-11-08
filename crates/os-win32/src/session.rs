//! Process plumbing the app needs without touching `windows` itself: DPI awareness, monitor list, message pump,
//! console exit handler.

use std::sync::atomic::{AtomicBool, Ordering};

use bezel_core::{MonitorId, MonitorInfo, RectPx};
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_QUIT,
};

/// Invariant 5. Call once at startup, before any window exists. Returns false if it was already set (e.g. by manifest).
pub fn set_per_monitor_v2() -> bool {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }.is_ok()
}

fn r(rc: RECT) -> RectPx {
    RectPx::new(rc.left, rc.top, rc.right, rc.bottom)
}

unsafe extern "system" fn mon_cb(h: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    let out = unsafe { &mut *(lp.0 as *mut Vec<MonitorInfo>) };
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if unsafe { GetMonitorInfoW(h, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO) }.as_bool() {
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
        out.push(MonitorInfo {
            id: MonitorId(0),
            bounds: r(mi.monitorInfo.rcMonitor),
            work: r(mi.monitorInfo.rcWork),
            dpi: dx,
            primary: mi.monitorInfo.dwFlags & 1 != 0,
        });
    }
    BOOL(1)
}

/// Monitors left-to-right then top-to-bottom; ids are indexes in that order (stable for a given layout).
pub fn monitors() -> Vec<MonitorInfo> {
    let mut v: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(mon_cb), LPARAM(&mut v as *mut Vec<MonitorInfo> as isize));
    }
    v.sort_by_key(|m| (m.bounds.l, m.bounds.t));
    for (i, m) in v.iter_mut().enumerate() {
        m.id = MonitorId(i as u64);
    }
    v
}

/// Wait up to `timeout_ms` for input, then dispatch everything queued. Returns false on WM_QUIT.
pub fn pump(timeout_ms: u32) -> bool {
    unsafe {
        MsgWaitForMultipleObjectsEx(None, timeout_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return false;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}

static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static CLEANED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn ctrl(kind: u32) -> BOOL {
    EXIT_REQUESTED.store(true, Ordering::SeqCst);
    // Close / logoff / shutdown: Windows kills us when this returns, so wait for the main loop to detach (≤ 4.5 s).
    if matches!(kind, 2 | 5 | 6) {
        let t = std::time::Instant::now();
        while !CLEANED.load(Ordering::SeqCst) && t.elapsed() < std::time::Duration::from_millis(4500) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    BOOL(1)
}

/// Ctrl+C, Ctrl+Break, console close, logoff and shutdown set `exit_requested()`. Call `mark_cleaned()` after detaching.
pub fn install_console_exit() -> bool {
    unsafe { SetConsoleCtrlHandler(Some(ctrl), true) }.is_ok()
}

pub fn exit_requested() -> bool {
    EXIT_REQUESTED.load(Ordering::SeqCst)
}

pub fn mark_cleaned() {
    CLEANED.store(true, Ordering::SeqCst);
}
