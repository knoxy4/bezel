//! Shared spike plumbing: file + console log, monitor enumeration, console keys, Ctrl/close handler.
//! Logs carry rects, ids and counts only: never window titles, class names, process names or file paths.
#![allow(dead_code)]

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::System::Console::{
    GetStdHandle, ReadConsoleInputW, SetConsoleCtrlHandler, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
    INPUT_RECORD, KEY_EVENT, STD_INPUT_HANDLE,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI};

// ── log ──────────────────────────────────────────────────────────

struct Log {
    file: Option<File>,
    start: Instant,
}

static LOG: OnceLock<Mutex<Log>> = OnceLock::new();

pub fn log_init(path: &str) {
    // Append: a second run must never wipe the first run's evidence.
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok();
    let _ = LOG.set(Mutex::new(Log { file, start: Instant::now() }));
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    log_line(&format!("════ run start, unix time {unix}, pid {} ════", std::process::id()));
}

pub fn log_line(msg: &str) {
    let Some(l) = LOG.get() else {
        println!("{msg}");
        return;
    };
    let mut l = l.lock().unwrap_or_else(|p| p.into_inner());
    let t = l.start.elapsed().as_secs_f64();
    let line = format!("[{t:9.3}] {msg}");
    println!("{line}");
    if let Some(f) = l.file.as_mut() {
        let _ = writeln!(f, "{line}");
        let _ = f.flush();
    }
}

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => { $crate::common::log_line(&format!($($t)*)) };
}

pub fn log_os_version() {
    if let Ok(out) = std::process::Command::new("cmd").args(["/c", "ver"]).output() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        log_line(&format!("os: {s}"));
    }
}

// ── DPI ──────────────────────────────────────────────────────────

pub fn set_per_monitor_v2() {
    // SAFETY: process-wide setting, called once before any window exists.
    let r = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    log_line(&format!("dpi awareness per-monitor-v2: {}", if r.is_ok() { "set" } else { "FAILED (already set by manifest?)" }));
}

/// Same as `set_per_monitor_v2`, without logging (for probe modes that print one line).
pub fn set_per_monitor_v2_quiet() {
    // SAFETY: process-wide setting, called once before any window exists.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

// ── monitors ─────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct Mon {
    pub index: usize,
    pub hmon: isize,
    /// "\\.\DISPLAYn": an adapter output name, not user data.
    pub device: String,
    pub rect: RECT,
    pub work: RECT,
    pub dpi: u32,
    pub primary: bool,
}

impl Mon {
    pub fn w(&self) -> i32 {
        self.rect.right - self.rect.left
    }
    pub fn h(&self) -> i32 {
        self.rect.bottom - self.rect.top
    }
}

pub fn fmt_rect(r: &RECT) -> String {
    format!("({},{})-({},{}) {}x{}", r.left, r.top, r.right, r.bottom, r.right - r.left, r.bottom - r.top)
}

unsafe extern "system" fn mon_cb(h: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    let out = unsafe { &mut *(lp.0 as *mut Vec<Mon>) };
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if unsafe { GetMonitorInfoW(h, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO) }.as_bool() {
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
        let n = mi.szDevice.iter().position(|&c| c == 0).unwrap_or(mi.szDevice.len());
        out.push(Mon {
            index: out.len(),
            hmon: h.0 as isize,
            device: String::from_utf16_lossy(&mi.szDevice[..n]),
            rect: mi.monitorInfo.rcMonitor,
            work: mi.monitorInfo.rcWork,
            dpi: dx,
            primary: mi.monitorInfo.dwFlags & 1 != 0,
        });
    }
    BOOL(1)
}

/// Monitors sorted left-to-right, then top-to-bottom, so indexes are stable for a given layout.
pub fn monitors() -> Vec<Mon> {
    let mut v: Vec<Mon> = Vec::new();
    // SAFETY: the callback only writes into `v`, which outlives the call.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(mon_cb), LPARAM(&mut v as *mut Vec<Mon> as isize));
    }
    v.sort_by_key(|m| (m.rect.left, m.rect.top));
    for (i, m) in v.iter_mut().enumerate() {
        m.index = i;
    }
    v
}

pub fn same_layout(a: &[Mon], b: &[Mon]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.rect == y.rect && x.dpi == y.dpi && x.device == y.device)
}

// ── console keys (this console only; not a hook, not system input) ──

pub fn console_keys() -> Receiver<char> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // SAFETY: reads this process's own console input buffer.
        let Ok(h) = (unsafe { GetStdHandle(STD_INPUT_HANDLE) }) else { return };
        let mut buf = [INPUT_RECORD::default(); 16];
        loop {
            let mut n = 0u32;
            if unsafe { ReadConsoleInputW(h, &mut buf, &mut n) }.is_err() {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            for r in &buf[..n as usize] {
                if r.EventType as u32 == KEY_EVENT {
                    let k = unsafe { r.Event.KeyEvent };
                    let ch = unsafe { k.uChar.UnicodeChar };
                    if k.bKeyDown.as_bool() && ch != 0 {
                        if let Some(c) = char::from_u32(ch as u32) {
                            if tx.send(c.to_ascii_lowercase()).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        }
    });
    rx
}

// ── exit paths ───────────────────────────────────────────────────

/// Set by Ctrl+C / Ctrl+Break / console close / logoff / shutdown. The main loop polls it.
pub static EXIT: AtomicBool = AtomicBool::new(false);
/// Set by the main loop once every surface is detached.
pub static CLEANED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn ctrl_handler(kind: u32) -> BOOL {
    EXIT.store(true, Ordering::SeqCst);
    log_line(&format!("console control event {kind}: exiting"));
    if kind == CTRL_CLOSE_EVENT || kind == CTRL_LOGOFF_EVENT || kind == CTRL_SHUTDOWN_EVENT {
        // Windows kills the process when this returns (≈5 s budget): wait for the main loop to detach first.
        let t = Instant::now();
        while !CLEANED.load(Ordering::SeqCst) && t.elapsed() < Duration::from_millis(4500) {
            std::thread::sleep(Duration::from_millis(10));
        }
        log_line(&format!("close handler: cleaned={}", CLEANED.load(Ordering::SeqCst)));
    }
    BOOL(1)
}

pub fn install_ctrl_handler() {
    // SAFETY: registers a static fn for this process.
    let r = unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), true) };
    log_line(&format!("console ctrl handler: {}", if r.is_ok() { "installed" } else { "FAILED" }));
}

pub fn hwnd_id(h: HWND) -> String {
    format!("0x{:x}", h.0 as usize)
}
