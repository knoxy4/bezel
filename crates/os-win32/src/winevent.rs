//! `WindowTracker` on Win32 (ADR-0001 §5.4), promoted from spike S10 (PASS: 364 rescans, 0 mismatched cells).
//!
//! Global WinEvent hooks with WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS (no injection) on a shell-watch thread,
//! filtered to top-level windows. Rects are DWM extended frame bounds, passed through the S10 coverage filter.
//! Window class names are compared in memory to skip the shell's own desktop windows; they are never logged or sent.
//! Only rects and opaque ids leave this module.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::JoinHandle;

use bezel_core::{ExtWindowId, MonitorId, MonitorInfo, OsEvent, RectCause, RectPx, TargetWindow};
use bezel_os_api::{OsError, WindowTracker};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::session;

fn id_of(h: HWND) -> ExtWindowId {
    ExtWindowId(h.0 as usize as u64)
}

fn hwnd_of(id: ExtWindowId) -> HWND {
    HWND(id.0 as usize as *mut _)
}

fn class_is(h: HWND, name: &str) -> bool {
    let mut buf = [0u16; 32];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == name
}

fn frame_rect(h: HWND) -> Option<RectPx> {
    let mut r = RECT::default();
    let ok = unsafe { DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32) }.is_ok();
    if !ok && unsafe { GetWindowRect(h, &mut r) }.is_err() {
        return None;
    }
    (r.right > r.left && r.bottom > r.top).then(|| RectPx::new(r.left, r.top, r.right, r.bottom))
}

/// The S10 coverage filter: visible, top-level, not cloaked, not minimized, not ours, not the shell's desktop, not
/// click-through, not a fully transparent layered window. Returns its DWM frame if it counts.
fn counts(h: HWND, own_pid: u32) -> Option<RectPx> {
    unsafe {
        if !IsWindow(Some(h)).as_bool() || !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
            return None;
        }
        if GetAncestor(h, GA_ROOT) != h || h == GetShellWindow() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        if pid == own_pid || class_is(h, "WorkerW") || class_is(h, "Progman") {
            return None;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
        if cloaked != 0 {
            return None;
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TRANSPARENT.0 != 0 {
            return None;
        }
        if ex & WS_EX_LAYERED.0 != 0 {
            let mut alpha = 255u8;
            let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
            if GetLayeredWindowAttributes(h, None, Some(&mut alpha), Some(&mut flags)).is_ok() && flags.0 & LWA_ALPHA.0 != 0 && alpha == 0 {
                return None;
            }
        }
    }
    frame_rect(h)
}

fn target_of(h: HWND, own_pid: u32, mons: &[MonitorInfo]) -> Option<TargetWindow> {
    // Taskbars cover desktop (they count for the grid) but are never a canopy target.
    if class_is(h, "Shell_TrayWnd") || class_is(h, "Shell_SecondaryTrayWnd") {
        return None;
    }
    let frame = counts(h, own_pid)?;
    let (cx, cy) = ((frame.l + frame.r) / 2, (frame.t + frame.b) / 2);
    let monitor = mons.iter().find(|m| m.bounds.contains(cx, cy)).or_else(|| mons.iter().find(|m| m.primary)).map(|m| m.id)?;
    Some(TargetWindow { id: id_of(h), frame, monitor, maximized: unsafe { IsZoomed(h).as_bool() } })
}

struct HookState {
    tx: Sender<OsEvent>,
    own_pid: u32,
    mons: Vec<MonitorInfo>,
    known: HashSet<isize>,
    drag: Option<isize>,
}

thread_local! {
    static STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

unsafe extern "system" fn win_event(_: HWINEVENTHOOK, event: u32, h: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    if id_object != OBJID_WINDOW.0 || id_child != CHILDID_SELF as i32 || h.is_invalid() {
        return;
    }
    STATE.with(|s| {
        let Ok(mut guard) = s.try_borrow_mut() else { return };
        let Some(st) = guard.as_mut() else { return };
        let key = h.0 as isize;
        if event == EVENT_OBJECT_DESTROY {
            if st.known.remove(&key) {
                let _ = st.tx.send(OsEvent::WindowRect { id: id_of(h), rect: None, cause: RectCause::Destroyed });
            }
            return;
        }
        if unsafe { GetAncestor(h, GA_ROOT) } != h {
            return;
        }
        if event == EVENT_SYSTEM_FOREGROUND {
            let _ = st.tx.send(OsEvent::Foreground(target_of(h, st.own_pid, &st.mons)));
            return;
        }
        let cause = match event {
            EVENT_SYSTEM_MOVESIZESTART => {
                st.drag = Some(key);
                RectCause::DragStart
            }
            EVENT_SYSTEM_MOVESIZEEND => {
                if st.drag == Some(key) {
                    st.drag = None;
                }
                RectCause::DragEnd
            }
            EVENT_SYSTEM_MINIMIZESTART => RectCause::Minimized,
            EVENT_SYSTEM_MINIMIZEEND => RectCause::Restored,
            EVENT_OBJECT_CREATE | EVENT_OBJECT_SHOW => RectCause::Shown,
            EVENT_OBJECT_HIDE => RectCause::Hidden,
            EVENT_OBJECT_CLOAKED => RectCause::Cloaked,
            EVENT_OBJECT_UNCLOAKED => RectCause::Uncloaked,
            EVENT_OBJECT_LOCATIONCHANGE if st.drag == Some(key) => RectCause::DragSample,
            EVENT_OBJECT_LOCATIONCHANGE => RectCause::Jumped,
            _ => return,
        };
        let rect = counts(h, st.own_pid);
        if rect.is_none() && !st.known.contains(&key) {
            return; // never counted, still doesn't: nothing to tell the grid
        }
        match rect {
            Some(_) => st.known.insert(key),
            None => st.known.remove(&key),
        };
        let _ = st.tx.send(OsEvent::WindowRect { id: id_of(h), rect, cause });
    });
}

unsafe extern "system" fn enum_top(h: HWND, lp: LPARAM) -> BOOL {
    unsafe { (*(lp.0 as *mut Vec<isize>)).push(h.0 as isize) };
    BOOL(1)
}

fn all_counted(own_pid: u32) -> Vec<(HWND, RectPx)> {
    let mut v: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_top), LPARAM(&mut v as *mut Vec<isize> as isize));
    }
    v.into_iter().map(|x| HWND(x as *mut _)).filter_map(|h| counts(h, own_pid).map(|r| (h, r))).collect()
}

pub struct Win32WindowTracker {
    own_pid: u32,
    thread: Option<JoinHandle<()>>,
    tid: Arc<AtomicU32>,
}

impl Default for Win32WindowTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Win32WindowTracker {
    pub fn new() -> Self {
        Self { own_pid: unsafe { GetCurrentProcessId() }, thread: None, tid: Arc::new(AtomicU32::new(0)) }
    }

    /// The current foreground window as a target, if it counts.
    pub fn foreground(&self) -> Option<TargetWindow> {
        target_of(unsafe { GetForegroundWindow() }, self.own_pid, &session::monitors())
    }
}

impl WindowTracker for Win32WindowTracker {
    fn start(&mut self, tx: Sender<OsEvent>) -> Result<(), OsError> {
        if self.thread.is_some() {
            return Ok(());
        }
        let own_pid = self.own_pid;
        let tid = self.tid.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), OsError>>();
        let handle = std::thread::Builder::new()
            .name("bezel-shell-watch".into())
            .spawn(move || unsafe {
                tid.store(GetCurrentThreadId(), Ordering::SeqCst);
                let known: HashSet<isize> = all_counted(own_pid).into_iter().map(|(h, _)| h.0 as isize).collect();
                STATE.with(|s| *s.borrow_mut() = Some(HookState { tx, own_pid, mons: session::monitors(), known, drag: None }));
                let ranges = [
                    (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
                    (EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND),
                    (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
                    (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
                    (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
                    (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
                ];
                let hooks: Vec<HWINEVENTHOOK> = ranges
                    .iter()
                    .map(|(a, b)| SetWinEventHook(*a, *b, None, Some(win_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS))
                    .collect();
                if hooks.iter().any(|h| h.is_invalid()) {
                    let _ = ready_tx.send(Err(OsError::Win32("SetWinEventHook failed".into())));
                } else {
                    let _ = ready_tx.send(Ok(()));
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    DispatchMessageW(&msg);
                }
                for h in hooks.into_iter().filter(|h| !h.is_invalid()) {
                    let _ = UnhookWinEvent(h);
                }
                STATE.with(|s| *s.borrow_mut() = None);
            })
            .map_err(|e| OsError::Win32(format!("spawn shell-watch thread: {e}")))?;
        let started = ready_rx.recv().unwrap_or_else(|_| Err(OsError::Win32("shell-watch thread died".into())));
        self.thread = Some(handle);
        started
    }

    fn frame_of(&self, id: ExtWindowId) -> Option<RectPx> {
        counts(hwnd_of(id), self.own_pid)
    }

    fn snapshot(&self, m: MonitorId) -> Vec<(ExtWindowId, RectPx)> {
        let Some(mon) = session::monitors().into_iter().find(|x| x.id == m) else { return Vec::new() };
        all_counted(self.own_pid).into_iter().filter(|(_, r)| r.intersect(&mon.bounds).is_some()).map(|(h, r)| (id_of(h), r)).collect()
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

impl Drop for Win32WindowTracker {
    fn drop(&mut self) {
        self.stop();
    }
}
