//! `DesktopLayer` on Win32 (ADR-0001 §5.3), promoted from spike S9.
//!
//! Two shell structures: Legacy (a WorkerW behind the icons, spawned by Progman message 0x052C) and Raised (Windows 11
//! 24H2+: Progman has WS_EX_NOREDIRECTIONBITMAP, the icons are its child, the wallpaper WorkerW sits below them).
//! Underlays are WS_CHILD windows of the host, placed directly below the icons (raised) or on top inside the WorkerW
//! (legacy). They take no input (D23).
//!
//! The thread that creates the layer owns the windows and must pump messages: a child of another process's window
//! shares Explorer's input queue, so a stalled loop stalls the desktop.
//!
//! Invariant 6: never writes the wallpaper. Reads it through IDesktopWallpaper only.

use std::collections::HashMap;
use std::num::NonZeroIsize;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use bezel_core::{MonitorId, RectPx};
use bezel_os_api::{DesktopLayer, DesktopTopology, LayerHealth, OsError, UnderlayMode, WallpaperDesc, WallpaperFit};
use raw_window_handle::{RawWindowHandle, Win32WindowHandle};
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, MapWindowPoints, RedrawWindow, PAINTSTRUCT, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    DesktopWallpaper, IDesktopWallpaper, DESKTOP_WALLPAPER_POSITION, DWPOS_CENTER, DWPOS_FILL, DWPOS_FIT, DWPOS_SPAN, DWPOS_STRETCH,
    DWPOS_TILE,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Underlay HWNDs, so every exit path (panic hook, console handler, WM_ENDSESSION) can find them.
static UNDERLAYS: Mutex<Vec<isize>> = Mutex::new(Vec::new());
static DETACHING: AtomicBool = AtomicBool::new(false);
static TORN_DOWN: AtomicBool = AtomicBool::new(false);
static SHELL_RESTARTED: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static CLASSES: std::sync::Once = std::sync::Once::new();

const UNDERLAY_CLASS: PCWSTR = w!("BezelUnderlay");
const WATCH_CLASS: PCWSTR = w!("BezelShellWatch");

/// Destroy every underlay window. For panic hooks and console handlers. Only works on the owning thread; elsewhere the
/// windows die with the process.
pub fn emergency_detach() -> usize {
    let hwnds: Vec<isize> = std::mem::take(&mut *UNDERLAYS.lock().unwrap_or_else(|p| p.into_inner()));
    DETACHING.store(true, Ordering::SeqCst);
    // SAFETY: DestroyWindow fails harmlessly off the owning thread.
    let n = hwnds.iter().filter(|h| unsafe { DestroyWindow(HWND(**h as *mut _)) }.is_ok()).count();
    DETACHING.store(false, Ordering::SeqCst);
    n
}

#[derive(Clone, Copy, Debug)]
struct Shell {
    topo: DesktopTopology,
    progman: HWND,
    host: HWND,
    icons: HWND,
}

struct Attached {
    hwnd: HWND,
    rect: RectPx,
    mode: UnderlayMode,
}

pub struct Win32DesktopLayer {
    hinst: HINSTANCE,
    shell: Option<Shell>,
    under: HashMap<MonitorId, Attached>,
    /// Attach order, for z-order placement.
    order: Vec<MonitorId>,
    wp: Option<IDesktopWallpaper>,
    watch: Option<HWND>,
}

fn class_is(h: HWND, name: &str) -> bool {
    let mut buf = [0u16; 64];
    // SAFETY: local buffer; matching only, never logged.
    let n = unsafe { GetClassNameW(h, &mut buf) };
    n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == name
}

unsafe extern "system" fn underlay_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
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
        WM_DESTROY => {
            if !DETACHING.load(Ordering::SeqCst) {
                TORN_DOWN.store(true, Ordering::SeqCst);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
    }
}

unsafe extern "system" fn watch_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let tbc = TASKBAR_CREATED.load(Ordering::SeqCst);
    if tbc != 0 && msg == tbc {
        SHELL_RESTARTED.store(true, Ordering::SeqCst);
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

unsafe extern "system" fn find_icons_host(h: HWND, lp: LPARAM) -> BOOL {
    if let Ok(dv) = unsafe { FindWindowExW(Some(h), None, w!("SHELLDLL_DefView"), PCWSTR::null()) } {
        unsafe { *(lp.0 as *mut (isize, isize)) = (h.0 as isize, dv.0 as isize) };
        return BOOL(0);
    }
    BOOL(1)
}

unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> BOOL {
    unsafe { (*(lp.0 as *mut Vec<isize>)).push(h.0 as isize) };
    BOOL(1)
}

fn discover() -> Result<Shell, OsError> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).map_err(|_| OsError::NotReady("no Progman".into()))?;
        let mut res = 0usize;
        SendMessageTimeoutW(progman, 0x052C, WPARAM(0), LPARAM(0), SMTO_NORMAL, 1000, Some(&mut res));
        let ex = GetWindowLongPtrW(progman, GWL_EXSTYLE) as u32;
        let icons_in_progman = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null()).ok();
        if let (true, Some(icons)) = (ex & WS_EX_NOREDIRECTIONBITMAP.0 != 0, icons_in_progman) {
            return Ok(Shell { topo: DesktopTopology::Raised, progman, host: progman, icons });
        }
        let mut found: (isize, isize) = (0, 0);
        let _ = EnumWindows(Some(find_icons_host), LPARAM(&mut found as *mut (isize, isize) as isize));
        if found.0 == 0 {
            match icons_in_progman {
                Some(icons) => found = (progman.0 as isize, icons.0 as isize),
                None => return Err(OsError::NotReady("no desktop icons window".into())),
            }
        }
        let worker = FindWindowExW(None, Some(HWND(found.0 as *mut _)), w!("WorkerW"), PCWSTR::null())
            .map_err(|_| OsError::NotReady("legacy: no WorkerW behind the icons".into()))?;
        Ok(Shell { topo: DesktopTopology::Legacy, progman, host: worker, icons: HWND(found.1 as *mut _) })
    }
}

fn to_rect(r: RectPx) -> RECT {
    RECT { left: r.l, top: r.t, right: r.r, bottom: r.b }
}

impl Win32DesktopLayer {
    /// Initializes COM (STA) on this thread, registers classes and a hidden shell-watch window, and probes the desktop.
    /// A failed probe isn't fatal: `attach` retries.
    pub fn new() -> Result<Self, OsError> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let hinst: HINSTANCE = GetModuleHandleW(PCWSTR::null()).map_err(|e| OsError::Win32(format!("GetModuleHandle: {e}")))?.into();
            CLASSES.call_once(|| {
                for (name, proc_) in [
                    (UNDERLAY_CLASS, underlay_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT),
                    (WATCH_CLASS, watch_proc),
                ] {
                    let wc = WNDCLASSEXW {
                        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                        lpfnWndProc: Some(proc_),
                        hInstance: hinst,
                        lpszClassName: name,
                        ..Default::default()
                    };
                    RegisterClassExW(&wc);
                }
                TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::SeqCst);
            });
            // Top-level (not message-only): TaskbarCreated is broadcast to top-level windows.
            let watch = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, WATCH_CLASS, w!("BEZEL shell watch"), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst), None).ok();
            let wp = CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL).ok();
            Ok(Self { hinst, shell: discover().ok(), under: HashMap::new(), order: Vec::new(), wp, watch })
        }
    }

    fn shell(&mut self) -> Result<Shell, OsError> {
        if let Some(s) = self.shell {
            if unsafe { IsWindow(Some(s.host)).as_bool() } {
                return Ok(s);
            }
        }
        let s = discover()?;
        self.shell = Some(s);
        Ok(s)
    }

    fn place(&self, s: &Shell) {
        let mut after = match s.topo {
            DesktopTopology::Raised => s.icons,
            _ => HWND_TOP,
        };
        for m in &self.order {
            if let Some(a) = self.under.get(m) {
                unsafe {
                    let _ = SetWindowPos(a.hwnd, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
                }
                after = a.hwnd;
            }
        }
    }

    fn z_ok(&self, s: &Shell) -> bool {
        let ours: Vec<HWND> = self.order.iter().filter_map(|m| self.under.get(m).map(|a| a.hwnd)).collect();
        let Some(first) = ours.first().copied() else { return true };
        let mut h = unsafe {
            match s.topo {
                DesktopTopology::Raised => GetWindow(s.icons, GW_HWNDNEXT).ok(),
                _ => GetWindow(first, GW_HWNDFIRST).ok(),
            }
        };
        let mut seen = 0;
        while let Some(x) = h {
            if !ours.contains(&x) {
                break;
            }
            seen += 1;
            h = unsafe { GetWindow(x, GW_HWNDNEXT).ok() };
        }
        seen == ours.len()
    }

    /// Screen rects of visible windows in the desktop layer that are neither the shell's nor ours.
    fn foreign_rects(&self) -> Vec<RECT> {
        let Some(s) = self.shell else { return Vec::new() };
        let ours: Vec<isize> = self.under.values().map(|a| a.hwnd.0 as isize).collect();
        let mut all: Vec<isize> = Vec::new();
        for root in [s.host, s.progman] {
            unsafe {
                let _ = EnumChildWindows(Some(root), Some(collect), LPARAM(&mut all as *mut Vec<isize> as isize));
            }
        }
        all.sort_unstable();
        all.dedup();
        all.into_iter()
            .map(|x| HWND(x as *mut _))
            .filter(|h| !ours.contains(&(h.0 as isize)) && *h != s.icons)
            .filter(|h| unsafe { IsWindowVisible(*h).as_bool() && !IsChild(s.icons, *h).as_bool() })
            .filter(|h| !class_is(*h, "WorkerW") && !class_is(*h, "SHELLDLL_DefView"))
            .filter_map(|h| {
                let mut rc = RECT::default();
                (unsafe { GetWindowRect(h, &mut rc) }.is_ok() && rc.right > rc.left && rc.bottom > rc.top).then_some(rc)
            })
            .collect()
    }
}

impl DesktopLayer for Win32DesktopLayer {
    fn topology(&self) -> DesktopTopology {
        self.shell.map(|s| s.topo).unwrap_or(DesktopTopology::Unavailable)
    }

    fn attach(&mut self, m: MonitorId, rect: RectPx) -> Result<RawWindowHandle, OsError> {
        let s = self.shell()?;
        if let Some(old) = self.under.remove(&m) {
            DETACHING.store(true, Ordering::SeqCst);
            unsafe {
                let _ = DestroyWindow(old.hwnd);
            }
            DETACHING.store(false, Ordering::SeqCst);
        }
        let mut pts = [POINT { x: rect.l, y: rect.t }, POINT { x: rect.r, y: rect.b }];
        unsafe { MapWindowPoints(None, Some(s.host), &mut pts) };
        let (x, y, w, h) = (pts[0].x, pts[0].y, (pts[1].x - pts[0].x).max(1), (pts[1].y - pts[0].y).max(1));
        let style = WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_DISABLED;
        let mk = |ex: WINDOW_EX_STYLE| unsafe { CreateWindowExW(ex, UNDERLAY_CLASS, w!("BEZEL underlay"), style, x, y, w, h, Some(s.host), None, Some(self.hinst), None) };
        let hwnd = mk(WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP)
            .or_else(|_| mk(WS_EX_NOACTIVATE))
            .map_err(|e| OsError::Win32(format!("CreateWindowEx underlay: {e}")))?;
        UNDERLAYS.lock().unwrap_or_else(|p| p.into_inner()).push(hwnd.0 as isize);
        self.under.insert(m, Attached { hwnd, rect, mode: UnderlayMode::Opaque });
        self.order.retain(|x| *x != m);
        self.order.push(m);
        self.place(&s);
        let nz = NonZeroIsize::new(hwnd.0 as isize).ok_or_else(|| OsError::Win32("null hwnd".into()))?;
        Ok(RawWindowHandle::Win32(Win32WindowHandle::new(nz)))
    }

    fn detach_all(&mut self) {
        DETACHING.store(true, Ordering::SeqCst);
        for (_, a) in self.under.drain() {
            unsafe {
                if IsWindow(Some(a.hwnd)).as_bool() {
                    let _ = DestroyWindow(a.hwnd);
                }
            }
        }
        self.order.clear();
        UNDERLAYS.lock().unwrap_or_else(|p| p.into_inner()).clear();
        if let Some(s) = self.shell {
            unsafe {
                if IsWindow(Some(s.host)).as_bool() {
                    // A repaint request only; the wallpaper setting is never touched.
                    let _ = RedrawWindow(Some(s.host), None, None, RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN);
                }
            }
        }
        DETACHING.store(false, Ordering::SeqCst);
        TORN_DOWN.store(false, Ordering::SeqCst);
    }

    fn wallpaper(&self, m: MonitorId) -> Option<WallpaperDesc> {
        let dw = self.wp.as_ref()?;
        let want = to_rect(self.under.get(&m)?.rect);
        unsafe {
            let count = dw.GetMonitorDevicePathCount().ok()?;
            for i in 0..count {
                let Ok(id) = dw.GetMonitorDevicePathAt(i) else { continue };
                let hit = dw.GetMonitorRECT(PCWSTR(id.0)).map(|r| r == want).unwrap_or(false);
                let out = hit.then(|| {
                    let path = dw.GetWallpaper(PCWSTR(id.0)).ok().and_then(|p| {
                        let s = p.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(p.0 as *const _));
                        (!s.is_empty()).then(|| std::path::PathBuf::from(s))
                    });
                    let fit = fit_of(dw.GetPosition().unwrap_or(DWPOS_FILL));
                    let c = dw.GetBackgroundColor().map(|c| c.0).unwrap_or(0);
                    WallpaperDesc { path, fit, background: [(c & 0xff) as u8, ((c >> 8) & 0xff) as u8, ((c >> 16) & 0xff) as u8, 255] }
                });
                CoTaskMemFree(Some(id.0 as *const _));
                if out.is_some() {
                    return out;
                }
            }
        }
        None
    }

    fn foreign_occupant(&self, m: MonitorId) -> bool {
        let Some(a) = self.under.get(&m) else { return false };
        let r = a.rect;
        let area = (r.width() as i64) * (r.height() as i64);
        self.foreign_rects().iter().any(|f| {
            let w = (f.right.min(r.r) - f.left.max(r.l)).max(0) as i64;
            let h = (f.bottom.min(r.b) - f.top.max(r.t)).max(0) as i64;
            area > 0 && w * h * 2 >= area
        })
    }

    fn set_mode(&mut self, m: MonitorId, mode: UnderlayMode) -> Result<(), OsError> {
        let a = self.under.get_mut(&m).ok_or(OsError::UnknownMonitor(m))?;
        a.mode = mode;
        unsafe {
            let _ = ShowWindow(a.hwnd, if mode == UnderlayMode::Excluded { SW_HIDE } else { SW_SHOWNOACTIVATE });
        }
        Ok(())
    }

    fn health(&mut self) -> LayerHealth {
        if SHELL_RESTARTED.swap(false, Ordering::SeqCst) || TORN_DOWN.load(Ordering::SeqCst) {
            return LayerHealth::ResetNeeded;
        }
        let Some(s) = self.shell else { return LayerHealth::ResetNeeded };
        let progman_now = unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.ok();
        if progman_now != Some(s.progman) || !unsafe { IsWindow(Some(s.host)).as_bool() } {
            return LayerHealth::ResetNeeded;
        }
        for a in self.under.values() {
            let ok = unsafe { IsWindow(Some(a.hwnd)).as_bool() && GetParent(a.hwnd).ok() == Some(s.host) };
            if !ok {
                return LayerHealth::ResetNeeded;
            }
        }
        if !self.z_ok(&s) {
            self.place(&s);
            return LayerHealth::ZOrderFixed;
        }
        LayerHealth::Ok
    }
}

impl Drop for Win32DesktopLayer {
    fn drop(&mut self) {
        self.detach_all();
        if let Some(w) = self.watch.take() {
            unsafe {
                let _ = DestroyWindow(w);
            }
        }
    }
}

fn fit_of(p: DESKTOP_WALLPAPER_POSITION) -> WallpaperFit {
    match p {
        DWPOS_CENTER => WallpaperFit::Center,
        DWPOS_TILE => WallpaperFit::Tile,
        DWPOS_STRETCH => WallpaperFit::Stretch,
        DWPOS_FIT => WallpaperFit::Fit,
        DWPOS_SPAN => WallpaperFit::Span,
        _ => WallpaperFit::Fill,
    }
}
