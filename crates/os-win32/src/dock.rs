//! `ShellDock` on Win32 (ADR-0001 §5.2), promoted from spike S3 (PASS: re-registers after an Explorer restart; the
//! shell reclaims a force-killed bar's space within 1 s).
//!
//! Bars are WS_POPUP + TOOLWINDOW | TOPMOST | NOACTIVATE | NOREDIRECTIONBITMAP (DirectComposition draws them, S0).
//! The creating thread owns the windows and must pump messages. Every exit path runs ABM_REMOVE: `undock_all`, Drop,
//! WM_ENDSESSION and `emergency_undock` (for panic hooks and console handlers).

use std::num::NonZeroIsize;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use bezel_core::{Edge, MonitorId, MonitorInfo, OsEvent, Px, RectPx, SurfaceId};
use bezel_os_api::{DockMode, OsError, ShellDock};
use raw_window_handle::{RawWindowHandle, Win32WindowHandle};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETTASKBARPOS, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS,
    ABN_FULLSCREENAPP, ABN_POSCHANGED, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::session;

const CALLBACK: u32 = WM_APP + 1;
const BAR_CLASS: PCWSTR = w!("BezelBar");

/// Registered bar HWNDs, for exit paths that can't reach the dock object.
static REGISTERED: Mutex<Vec<isize>> = Mutex::new(Vec::new());
static POSCHANGED: AtomicBool = AtomicBool::new(false);
static REREGISTER: AtomicBool = AtomicBool::new(false);
static FULLSCREEN: Mutex<Vec<bool>> = Mutex::new(Vec::new());
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static CLASS: std::sync::Once = std::sync::Once::new();

fn abd(h: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: h, uCallbackMessage: CALLBACK, ..Default::default() }
}

/// ABM_REMOVE every registered bar. Safe from any thread and safe to repeat; for panic hooks and console handlers.
pub fn emergency_undock() -> usize {
    let hwnds: Vec<isize> = std::mem::take(&mut *REGISTERED.lock().unwrap_or_else(|p| p.into_inner()));
    for h in &hwnds {
        let mut d = abd(HWND(*h as *mut _));
        // SAFETY: shell message with a stack struct; works cross-thread.
        unsafe { SHAppBarMessage(ABM_REMOVE, &mut d) };
    }
    hwnds.len()
}

unsafe extern "system" fn bar_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let tbc = TASKBAR_CREATED.load(Ordering::SeqCst);
    if tbc != 0 && msg == tbc {
        REREGISTER.store(true, Ordering::SeqCst);
        return LRESULT(0);
    }
    match msg {
        CALLBACK => {
            match wp.0 as u32 {
                ABN_POSCHANGED => POSCHANGED.store(true, Ordering::SeqCst),
                ABN_FULLSCREENAPP => FULLSCREEN.lock().unwrap_or_else(|p| p.into_inner()).push(lp.0 != 0),
                _ => {}
            }
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe {
                BeginPaint(h, &mut ps);
                let _ = EndPaint(h, &ps);
            }
            LRESULT(0)
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wp.0 != 0 {
                emergency_undock();
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
    }
}

struct Bar {
    s: SurfaceId,
    hwnd: HWND,
    m: MonitorId,
    edge: Edge,
    thickness: Px,
    mode: DockMode,
    registered: bool,
    rect: RectPx,
    hidden: bool,
}

pub struct Win32ShellDock {
    hinst: HINSTANCE,
    /// Dock order: re-docking replays it (TRENCH before SHAFT).
    bars: Vec<Bar>,
}

fn ab_edge(e: Edge) -> u32 {
    match e {
        Edge::Left => ABE_LEFT,
        Edge::Top => ABE_TOP,
        Edge::Right => ABE_RIGHT,
        Edge::Bottom => ABE_BOTTOM,
    }
}

fn rpx(r: RECT) -> RectPx {
    RectPx::new(r.left, r.top, r.right, r.bottom)
}

fn rc(r: RectPx) -> RECT {
    RECT { left: r.l, top: r.t, right: r.r, bottom: r.b }
}

impl Win32ShellDock {
    pub fn new() -> Result<Self, OsError> {
        let hinst: HINSTANCE = unsafe { GetModuleHandleW(PCWSTR::null()) }.map_err(|e| OsError::Win32(format!("GetModuleHandle: {e}")))?.into();
        CLASS.call_once(|| unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(bar_proc),
                hInstance: hinst,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: BAR_CLASS,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::SeqCst);
        });
        Ok(Self { hinst, bars: Vec::new() })
    }

    /// Compute, negotiate and apply one bar's rect. A left/right bar stops at the top of a bottom bar on the same
    /// monitor docked before it, so the corner belongs to the bottom bar (§2.5).
    fn place(&mut self, i: usize, mons: &[MonitorInfo]) -> Result<RectPx, OsError> {
        let (m, edge, t, mode, hwnd) = {
            let b = &self.bars[i];
            (b.m, b.edge, b.thickness, b.mode, b.hwnd)
        };
        let mon = mons.iter().find(|x| x.id == m).ok_or(OsError::UnknownMonitor(m))?;
        let base = if mode == DockMode::Reserve { mon.bounds } else { mon.work };
        let mut want = match edge {
            Edge::Bottom => RectPx::new(base.l, base.b - t, base.r, base.b),
            Edge::Top => RectPx::new(base.l, base.t, base.r, base.t + t),
            Edge::Left => RectPx::new(base.l, base.t, base.l + t, base.b),
            Edge::Right => RectPx::new(base.r - t, base.t, base.r, base.b),
        };
        let floor = self.bars[..i].iter().filter(|b| b.m == m && b.edge == Edge::Bottom).map(|b| b.rect.t).min();
        let got = if mode == DockMode::Reserve {
            let mut d = abd(hwnd);
            d.uEdge = ab_edge(edge);
            d.rc = rc(want);
            unsafe { SHAppBarMessage(ABM_QUERYPOS, &mut d) };
            match edge {
                Edge::Bottom => d.rc.top = d.rc.bottom - t,
                Edge::Top => d.rc.bottom = d.rc.top + t,
                Edge::Left => d.rc.right = d.rc.left + t,
                Edge::Right => d.rc.left = d.rc.right - t,
            }
            if matches!(edge, Edge::Left | Edge::Right) {
                if let Some(f) = floor {
                    d.rc.bottom = d.rc.bottom.min(f);
                }
            }
            unsafe { SHAppBarMessage(ABM_SETPOS, &mut d) };
            rpx(d.rc)
        } else {
            if matches!(edge, Edge::Left | Edge::Right) {
                if let Some(f) = floor {
                    want.b = want.b.min(f);
                }
            }
            want
        };
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), got.l, got.t, got.width(), got.height(), SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        self.bars[i].rect = got;
        Ok(got)
    }

    fn register(&mut self, i: usize) {
        let b = &mut self.bars[i];
        if b.mode == DockMode::Reserve && !b.registered {
            let mut d = abd(b.hwnd);
            unsafe { SHAppBarMessage(ABM_NEW, &mut d) };
            b.registered = true;
            REGISTERED.lock().unwrap_or_else(|p| p.into_inner()).push(b.hwnd.0 as isize);
        }
    }

    fn redock_all(&mut self, events: &mut Vec<OsEvent>) {
        let mons = session::monitors();
        for i in 0..self.bars.len() {
            self.register(i);
            let before = self.bars[i].rect;
            if let Ok(r) = self.place(i, &mons) {
                if r != before {
                    events.push(OsEvent::Docked { surface: self.bars[i].s, rect: r });
                }
            }
        }
    }
}

impl ShellDock for Win32ShellDock {
    fn monitors(&self) -> Vec<MonitorInfo> {
        session::monitors()
    }

    /// The primary taskbar's edge. Windows 11 puts every monitor's taskbar on the same edge.
    fn taskbar_edge(&self, _m: MonitorId) -> Edge {
        let mut d = abd(HWND::default());
        if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut d) } == 0 {
            return Edge::Bottom;
        }
        match d.uEdge {
            ABE_LEFT => Edge::Left,
            ABE_TOP => Edge::Top,
            ABE_RIGHT => Edge::Right,
            _ => Edge::Bottom,
        }
    }

    fn dock(&mut self, s: SurfaceId, m: MonitorId, edge: Edge, thickness: Px, mode: DockMode) -> Result<RectPx, OsError> {
        let i = match self.bars.iter().position(|b| b.s == s) {
            Some(i) => {
                let b = &mut self.bars[i];
                if b.mode != mode && b.registered {
                    let mut d = abd(b.hwnd);
                    unsafe { SHAppBarMessage(ABM_REMOVE, &mut d) };
                    b.registered = false;
                    REGISTERED.lock().unwrap_or_else(|p| p.into_inner()).retain(|h| *h != b.hwnd.0 as isize);
                }
                (b.m, b.edge, b.thickness, b.mode) = (m, edge, thickness, mode);
                i
            }
            None => {
                let hwnd = unsafe {
                    CreateWindowExW(
                        WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
                        BAR_CLASS,
                        w!("BEZEL bar"),
                        WS_POPUP,
                        0,
                        0,
                        1,
                        1,
                        None,
                        None,
                        Some(self.hinst),
                        None,
                    )
                }
                .map_err(|e| OsError::Win32(format!("CreateWindowEx bar: {e}")))?;
                self.bars.push(Bar { s, hwnd, m, edge, thickness, mode, registered: false, rect: RectPx::new(0, 0, 0, 0), hidden: false });
                self.bars.len() - 1
            }
        };
        self.register(i);
        let mons = session::monitors();
        self.place(i, &mons)
    }

    fn undock_all(&mut self) {
        for b in self.bars.drain(..) {
            if b.registered {
                let mut d = abd(b.hwnd);
                unsafe { SHAppBarMessage(ABM_REMOVE, &mut d) };
            }
            unsafe {
                let _ = DestroyWindow(b.hwnd);
            }
        }
        REGISTERED.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    fn surface_handle(&self, s: SurfaceId) -> Option<RawWindowHandle> {
        let b = self.bars.iter().find(|b| b.s == s)?;
        NonZeroIsize::new(b.hwnd.0 as isize).map(|nz| RawWindowHandle::Win32(Win32WindowHandle::new(nz)))
    }

    fn service(&mut self) -> Vec<OsEvent> {
        let mut ev = Vec::new();
        if REREGISTER.swap(false, Ordering::SeqCst) {
            // The old registrations died with the old Explorer.
            for b in &mut self.bars {
                b.registered = false;
            }
            REGISTERED.lock().unwrap_or_else(|p| p.into_inner()).clear();
            POSCHANGED.store(false, Ordering::SeqCst);
            self.redock_all(&mut ev);
        } else if POSCHANGED.swap(false, Ordering::SeqCst) {
            self.redock_all(&mut ev);
        }
        let fs: Vec<bool> = std::mem::take(&mut *FULLSCREEN.lock().unwrap_or_else(|p| p.into_inner()));
        if let Some(&on) = fs.last() {
            // Invariant 3: bars hide while a fullscreen app is in front.
            let mut mons: Vec<MonitorId> = Vec::new();
            for b in &mut self.bars {
                if b.hidden != on {
                    b.hidden = on;
                    unsafe {
                        let _ = ShowWindow(b.hwnd, if on { SW_HIDE } else { SW_SHOWNOACTIVATE });
                    }
                }
                if !mons.contains(&b.m) {
                    mons.push(b.m);
                }
            }
            ev.extend(mons.into_iter().map(|monitor| OsEvent::StandDown { monitor, on }));
        }
        ev
    }
}

impl Drop for Win32ShellDock {
    fn drop(&mut self) {
        self.undock_all();
    }
}
