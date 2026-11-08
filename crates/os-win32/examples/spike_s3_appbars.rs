//! Spike S3: two AppBars, bottom (TRENCH) + left (SHAFT), next to the Windows 11 taskbar (ADR-0001 §5.2, §9).
//!
//! TRENCH registers first and takes the home monitor's full width above the taskbar; SHAFT registers second on the
//! left and stops at TRENCH's top. Handles ABN_POSCHANGED / ABN_FULLSCREENAPP, re-registers on TaskbarCreated, and runs
//! ABM_REMOVE on every exit path it can (quit, Ctrl+C, console close, WM_ENDSESSION, panic). A force-kill can't run
//! cleanup: that's the case S3 measures.
//!
//! Modes:  (none)               run until q / Ctrl+C
//!         --quit-after <secs>  run, then take the normal exit path
//!         --print-workarea     print each monitor's work area as one line and exit (the harness's probe)
//! Logs to spike_s3.log (rects and counts only).

#[cfg(windows)]
#[macro_use]
mod common;

#[cfg(not(windows))]
fn main() {
    eprintln!("spike_s3_appbars is Windows-only");
}

#[cfg(windows)]
fn main() {
    imp::main();
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, SetBkMode, SetTextColor, DT_CENTER,
        DT_SINGLELINE, DT_VCENTER, HGDIOBJ, PAINTSTRUCT, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABN_FULLSCREENAPP,
        ABN_POSCHANGED, APPBARDATA,
    };
    use windows::Win32::UI::WindowsAndMessaging::*;

    use crate::common::{self, fmt_rect, Mon, CLEANED, EXIT};

    const LOG_PATH: &str = "spike_s3.log";
    const TRENCH_PX: i32 = 112; // D17, at 100% scaling
    const SHAFT_PX: i32 = 144;
    const CALLBACK: u32 = WM_APP + 1;

    static TRENCH: AtomicIsize = AtomicIsize::new(0);
    static SHAFT: AtomicIsize = AtomicIsize::new(0);
    static REGISTERED: AtomicBool = AtomicBool::new(false);
    static REPOSITION: AtomicBool = AtomicBool::new(false);
    static REREGISTER: AtomicBool = AtomicBool::new(false);
    static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

    fn hwnd(a: &AtomicIsize) -> HWND {
        HWND(a.load(Ordering::SeqCst) as *mut _)
    }

    fn abd(h: HWND) -> APPBARDATA {
        APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: h, uCallbackMessage: CALLBACK, ..Default::default() }
    }

    /// ABM_REMOVE on both bars. Safe to call more than once and from any thread.
    fn remove_both(why: &str) {
        if !REGISTERED.swap(false, Ordering::SeqCst) {
            return;
        }
        for (name, a) in [("SHAFT", &SHAFT), ("TRENCH", &TRENCH)] {
            let mut d = abd(hwnd(a));
            // SAFETY: plain shell message with a stack APPBARDATA.
            unsafe { SHAppBarMessage(ABM_REMOVE, &mut d) };
            common::log_line(&format!("ABM_REMOVE {name} ({why})"));
        }
    }

    fn register_both() {
        for (name, a) in [("TRENCH", &TRENCH), ("SHAFT", &SHAFT)] {
            let mut d = abd(hwnd(a));
            let ok = unsafe { SHAppBarMessage(ABM_NEW, &mut d) };
            log!("ABM_NEW {name}: {}", if ok != 0 { "ok" } else { "FAILED (already registered?)" });
        }
        REGISTERED.store(true, Ordering::SeqCst);
    }

    /// QUERYPOS → fix thickness → SETPOS → move the window. TRENCH first so SHAFT can stop at its top.
    fn position(home: &Mon) {
        let m = home.rect;
        let mut t = abd(hwnd(&TRENCH));
        t.uEdge = ABE_BOTTOM;
        t.rc = RECT { left: m.left, top: m.bottom - TRENCH_PX, right: m.right, bottom: m.bottom };
        unsafe { SHAppBarMessage(ABM_QUERYPOS, &mut t) };
        t.rc.top = t.rc.bottom - TRENCH_PX;
        unsafe { SHAppBarMessage(ABM_SETPOS, &mut t) };
        let tr = t.rc;

        let mut s = abd(hwnd(&SHAFT));
        s.uEdge = ABE_LEFT;
        s.rc = RECT { left: m.left, top: m.top, right: m.left + SHAFT_PX, bottom: tr.top };
        unsafe { SHAppBarMessage(ABM_QUERYPOS, &mut s) };
        s.rc.right = s.rc.left + SHAFT_PX;
        s.rc.bottom = s.rc.bottom.min(tr.top);
        unsafe { SHAppBarMessage(ABM_SETPOS, &mut s) };
        let sr = s.rc;

        unsafe {
            let flags = SWP_NOACTIVATE | SWP_SHOWWINDOW;
            let _ = SetWindowPos(hwnd(&TRENCH), Some(HWND_TOPMOST), tr.left, tr.top, tr.right - tr.left, tr.bottom - tr.top, flags);
            let _ = SetWindowPos(hwnd(&SHAFT), Some(HWND_TOPMOST), sr.left, sr.top, sr.right - sr.left, sr.bottom - sr.top, flags);
        }
        log!("positioned: TRENCH {} | SHAFT {}", fmt_rect(&tr), fmt_rect(&sr));
    }

    unsafe extern "system" fn bar_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let tbc = TASKBAR_CREATED.load(Ordering::SeqCst);
        if tbc != 0 && msg == tbc {
            if h == hwnd(&TRENCH) {
                common::log_line("TaskbarCreated: Explorer restarted, re-registering");
                REREGISTER.store(true, Ordering::SeqCst);
            }
            return LRESULT(0);
        }
        match msg {
            CALLBACK => {
                match wp.0 as u32 {
                    ABN_POSCHANGED => {
                        REPOSITION.store(true, Ordering::SeqCst);
                    }
                    ABN_FULLSCREENAPP => {
                        let on = lp.0 != 0;
                        if h == hwnd(&TRENCH) {
                            common::log_line(&format!("ABN_FULLSCREENAPP {}: bars {}", on, if on { "hidden" } else { "shown" }));
                            for a in [&TRENCH, &SHAFT] {
                                unsafe {
                                    let _ = ShowWindow(hwnd(a), if on { SW_HIDE } else { SW_SHOWNOACTIVATE });
                                }
                            }
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                unsafe {
                    let hdc = BeginPaint(h, &mut ps);
                    let mut rc = RECT::default();
                    let _ = GetClientRect(h, &mut rc);
                    let trench = h == hwnd(&TRENCH);
                    let br = CreateSolidBrush(COLORREF(if trench { 0x00203048 } else { 0x00302030 }));
                    FillRect(hdc, &rc, br);
                    let _ = DeleteObject(HGDIOBJ(br.0));
                    SetBkMode(hdc, TRANSPARENT);
                    SetTextColor(hdc, COLORREF(0x00a0d0ff));
                    let mut text: Vec<u16> = if trench { "BEZEL S3 · TRENCH (q in console to quit)" } else { "SHAFT" }.encode_utf16().collect();
                    DrawTextW(hdc, &mut text, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                    let _ = EndPaint(h, &ps);
                }
                LRESULT(0)
            }
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                if wp.0 != 0 {
                    EXIT.store(true, Ordering::SeqCst);
                    remove_both("WM_ENDSESSION");
                    CLEANED.store(true, Ordering::SeqCst);
                }
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
        }
    }

    fn workarea_line() -> String {
        common::monitors()
            .iter()
            .map(|m| format!("m{}{} work=({},{},{},{})", m.index, if m.primary { "*" } else { "" }, m.work.left, m.work.top, m.work.right, m.work.bottom))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "--print-workarea") {
            common::set_per_monitor_v2_quiet();
            println!("{}", workarea_line());
            return;
        }
        let quit_after = args.iter().position(|a| a == "--quit-after").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<u64>().ok());

        common::log_init(LOG_PATH);
        log!("BEZEL spike S3 (AppBars).{}", quit_after.map(|s| format!(" Quitting after {s} s.")).unwrap_or_default());
        common::log_os_version();
        common::set_per_monitor_v2();
        common::install_ctrl_handler();
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            common::log_line(&format!("PANIC: {info}"));
            remove_both("panic");
            prev(info);
        }));

        let mons = common::monitors();
        let Some(home) = mons.iter().find(|m| m.primary).cloned() else {
            log!("no primary monitor");
            return;
        };
        log!("before: {}", workarea_line());
        log!("home monitor {}: rect {} work {}", home.index, fmt_rect(&home.rect), fmt_rect(&home.work));

        let hinst: windows::Win32::Foundation::HINSTANCE = unsafe { GetModuleHandleW(PCWSTR::null()) }.unwrap_or_default().into();
        unsafe {
            TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::SeqCst);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(bar_proc),
                hInstance: hinst,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: w!("BezelS3Bar"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            for (a, title) in [(&TRENCH, w!("BEZEL TRENCH")), (&SHAFT, w!("BEZEL SHAFT"))] {
                match CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                    w!("BezelS3Bar"),
                    title,
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    None,
                    None,
                    Some(hinst),
                    None,
                ) {
                    Ok(h) => a.store(h.0 as isize, Ordering::SeqCst),
                    Err(e) => {
                        log!("CreateWindowEx failed: {e}");
                        return;
                    }
                }
            }
        }
        register_both();
        position(&home);
        std::thread::sleep(Duration::from_millis(300));
        log!("docked: {}", workarea_line());

        let keys = common::console_keys();
        let start = Instant::now();
        let mut last_wa = workarea_line();
        let mut last_poll = Instant::now();
        'main: loop {
            unsafe {
                MsgWaitForMultipleObjectsEx(None, 100, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'main;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if EXIT.load(Ordering::SeqCst) {
                break;
            }
            if keys.try_iter().any(|k| k == 'q') {
                break;
            }
            if quit_after.is_some_and(|s| start.elapsed() >= Duration::from_secs(s)) {
                log!("--quit-after reached");
                break;
            }
            if REREGISTER.swap(false, Ordering::SeqCst) {
                // The old registrations died with the old Explorer; register fresh and dock again.
                REGISTERED.store(false, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(500));
                register_both();
                position(&home);
            }
            if REPOSITION.swap(false, Ordering::SeqCst) {
                log!("ABN_POSCHANGED: re-docking");
                position(&home);
            }
            if last_poll.elapsed() >= Duration::from_secs(1) {
                last_poll = Instant::now();
                let wa = workarea_line();
                if wa != last_wa {
                    log!("work area changed: {wa}");
                    last_wa = wa;
                }
            }
        }

        remove_both("quit");
        unsafe {
            let _ = DestroyWindow(hwnd(&SHAFT));
            let _ = DestroyWindow(hwnd(&TRENCH));
        }
        std::thread::sleep(Duration::from_millis(300));
        CLEANED.store(true, Ordering::SeqCst);
        log!("after quit: {}", workarea_line());
        log!("exit.");
    }
}
