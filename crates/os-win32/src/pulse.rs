//! `InputPulseSource` on Win32 (ADR-0001 §5.6): Raw Input with RIDEV_INPUTSINK on a message-only window, on its own
//! thread. Not a hook (hooks are banned in this codebase).
//!
//! TRUST RULE (CLAUDE.md, re-check on every change to this file): the keyboard path calls `GetRawInputData` with
//! `RID_HEADER` ONLY. Every WM_INPUT is first read as a header; only a mouse header is followed by an `RID_INPUT` read.
//! A key press and its release are two keyboard events and are counted as such. Key identity never enters memory.
//! Counts are bucketed at 100 ms, never written to disk and never sent anywhere but the channel.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::JoinHandle;

use bezel_core::PulseBatch;
use bezel_os_api::{InputPulseSource, OsError};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICE_FLAGS, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RID_HEADER, RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::*;

const BUCKET_MS: u32 = 100;
const SINK_CLASS: PCWSTR = w!("BezelPulseSink");
// RI_MOUSE_* button-down flags: left, right, middle, X1, X2.
const BUTTON_DOWNS: u16 = 0x0001 | 0x0004 | 0x0010 | 0x0040 | 0x0100;
const WHEEL: u16 = 0x0400;
const HWHEEL: u16 = 0x0800;

thread_local! {
    static KEYS: Cell<u32> = const { Cell::new(0) };
    static CLICKS: Cell<u32> = const { Cell::new(0) };
    static WHEEL_TICKS: Cell<u32> = const { Cell::new(0) };
    static TRAVEL: Cell<u64> = const { Cell::new(0) };
}

fn bump(c: &'static std::thread::LocalKey<Cell<u32>>, n: u32) {
    c.with(|x| x.set(x.get().saturating_add(n)));
}

/// One WM_INPUT. Header first; the body is read only for the mouse.
fn on_input(lp: LPARAM) {
    let h = HRAWINPUT(lp.0 as *mut _);
    let mut header = RAWINPUTHEADER::default();
    let mut size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
    let hsz = std::mem::size_of::<RAWINPUTHEADER>() as u32;
    // SAFETY: writes at most `size` bytes into `header`.
    let got = unsafe { GetRawInputData(h, RID_HEADER, Some(&mut header as *mut _ as *mut _), &mut size, hsz) };
    if got == u32::MAX {
        return;
    }
    if header.dwType == RIM_TYPEKEYBOARD.0 {
        bump(&KEYS, 1); // RID_HEADER only: which key never reaches this process's memory.
        return;
    }
    if header.dwType != RIM_TYPEMOUSE.0 {
        return;
    }
    let mut raw = RAWINPUT::default();
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    // SAFETY: a mouse RAWINPUT fits in RAWINPUT; GetRawInputData refuses if the buffer is too small.
    let got = unsafe { GetRawInputData(h, RID_INPUT, Some(&mut raw as *mut _ as *mut _), &mut size, hsz) };
    if got == u32::MAX || raw.header.dwType != RIM_TYPEMOUSE.0 {
        return;
    }
    // SAFETY: dwType says the union holds a mouse record.
    let m = unsafe { raw.data.mouse };
    let (flags, data) = unsafe { (m.Anonymous.Anonymous.usButtonFlags, m.Anonymous.Anonymous.usButtonData) };
    bump(&CLICKS, (flags & BUTTON_DOWNS).count_ones());
    if flags & (WHEEL | HWHEEL) != 0 {
        let ticks = ((data as i16).unsigned_abs() as u32 / 120).max(1);
        bump(&WHEEL_TICKS, ticks);
    }
    if m.usFlags.0 & MOUSE_MOVE_ABSOLUTE.0 == 0 {
        let d = (m.lLastX.unsigned_abs() as u64) + (m.lLastY.unsigned_abs() as u64);
        TRAVEL.with(|t| t.set(t.get().saturating_add(d)));
    }
}

unsafe extern "system" fn sink_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_INPUT {
        on_input(lp);
        // WM_INPUT must still reach DefWindowProc so the system can clean up the raw input buffer.
    }
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

fn take_batch() -> PulseBatch {
    let clamp = |v: u32| v.min(u16::MAX as u32) as u16;
    PulseBatch {
        bucket_ms: BUCKET_MS as u16,
        key_events: clamp(KEYS.with(|c| c.replace(0))),
        mouse_clicks: clamp(CLICKS.with(|c| c.replace(0))),
        wheel_ticks: clamp(WHEEL_TICKS.with(|c| c.replace(0))),
        mouse_travel_px: TRAVEL.with(|c| c.replace(0)).min(u32::MAX as u64) as u32,
    }
}

fn devices(flags: RAWINPUTDEVICE_FLAGS, target: HWND) -> [RAWINPUTDEVICE; 2] {
    // Generic desktop page: 0x06 keyboard, 0x02 mouse.
    [
        RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x06, dwFlags: flags, hwndTarget: target },
        RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x02, dwFlags: flags, hwndTarget: target },
    ]
}

#[derive(Default)]
pub struct Win32Pulses {
    thread: Option<JoinHandle<()>>,
    tid: Arc<AtomicU32>,
    paused: Arc<AtomicBool>,
}

impl Win32Pulses {
    pub fn new() -> Self {
        Self::default()
    }
}

impl InputPulseSource for Win32Pulses {
    fn start(&mut self, tx: Sender<PulseBatch>) -> Result<(), OsError> {
        if self.thread.is_some() {
            return Ok(());
        }
        let tid = self.tid.clone();
        let paused = self.paused.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), OsError>>();
        let handle = std::thread::Builder::new()
            .name("bezel-pulses".into())
            .spawn(move || unsafe {
                tid.store(GetCurrentThreadId(), Ordering::SeqCst);
                let hinst: HINSTANCE = GetModuleHandleW(PCWSTR::null()).map(Into::into).unwrap_or_default();
                let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(sink_proc), hInstance: hinst, lpszClassName: SINK_CLASS, ..Default::default() };
                RegisterClassExW(&wc);
                let hwnd = match CreateWindowExW(WINDOW_EX_STYLE(0), SINK_CLASS, w!("BEZEL pulses"), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(hinst), None) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = ready_tx.send(Err(OsError::Win32(format!("pulse sink window: {e}"))));
                        return;
                    }
                };
                let devs = devices(RIDEV_INPUTSINK, hwnd);
                if let Err(e) = RegisterRawInputDevices(&devs, std::mem::size_of::<RAWINPUTDEVICE>() as u32) {
                    let _ = ready_tx.send(Err(OsError::Win32(format!("RegisterRawInputDevices: {e}"))));
                    let _ = DestroyWindow(hwnd);
                    return;
                }
                SetTimer(Some(hwnd), 1, BUCKET_MS, None);
                let _ = ready_tx.send(Ok(()));
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_TIMER && msg.hwnd == hwnd {
                        let b = take_batch();
                        let busy = b.key_events > 0 || b.mouse_clicks > 0 || b.wheel_ticks > 0 || b.mouse_travel_px > 0;
                        if busy && !paused.load(Ordering::SeqCst) && tx.send(b).is_err() {
                            break;
                        }
                        continue;
                    }
                    DispatchMessageW(&msg);
                }
                let _ = RegisterRawInputDevices(&devices(RIDEV_REMOVE, HWND::default()), std::mem::size_of::<RAWINPUTDEVICE>() as u32);
                let _ = KillTimer(Some(hwnd), 1);
                let _ = DestroyWindow(hwnd);
            })
            .map_err(|e| OsError::Win32(format!("spawn pulse thread: {e}")))?;
        let started = ready_rx.recv().unwrap_or_else(|_| Err(OsError::Win32("pulse thread died".into())));
        self.thread = Some(handle);
        started
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    fn stop(&mut self) {
        let tid = self.tid.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Win32Pulses {
    fn drop(&mut self) {
        self.stop();
    }
}
