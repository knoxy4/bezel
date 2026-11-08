//! Spike S10: occlusion fidelity (ADR-0001 §2.2, §5.4, §9).
//!
//! Global out-of-context WinEvent hooks (WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS; no injection), filtered to
//! top-level windows, feed `bezel_layout::OcclusionGrid`. Window rects come from DWM extended frame bounds.
//! Once per second it logs raw vs filtered event counts, windows tracked and cells swept during drags. Every 5 s it
//! compares the live grid against a full rescan and logs mismatched cells. A small debug window draws the grid.
//!
//! Logs rects, ids and counts only: never titles, class names or process names. Logs to spike_s10.log.
//! Console keys: q = quit.

#[cfg(windows)]
#[macro_use]
mod common;

#[cfg(not(windows))]
fn main() {
    eprintln!("spike_s10_occlusion is Windows-only");
}

#[cfg(windows)]
fn main() {
    imp::main();
}

#[cfg(windows)]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use bezel_core::{CellId, ExtWindowId, MonitorId, MonitorInfo, RectCause, RectPx};
    use bezel_layout::{art_scale_for, OcclusionGrid};
    use windows::core::{w, BOOL, PCWSTR};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect, SetStretchBltMode, StretchDIBits,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, COLORONCOLOR, DIB_RGB_COLORS, HGDIOBJ, PAINTSTRUCT, SRCCOPY,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::*;

    use crate::common::{self, Mon, CLEANED, EXIT};

    const LOG_PATH: &str = "spike_s10.log";
    const FRAME: Duration = Duration::from_millis(33); // also the drag sampler: ≤ 30 Hz
    const CELL_ART_PX: u16 = 16;
    const DEBUG_W: i32 = 720;

    // ── hook side: the callback only counts, filters and queues ──

    thread_local! {
        static RAW: Cell<u64> = const { Cell::new(0) };
        static FILTERED: Cell<u64> = const { Cell::new(0) };
        static QUEUE: RefCell<Vec<(u32, isize)>> = const { RefCell::new(Vec::new()) };
        static KNOWN: RefCell<HashSet<isize>> = RefCell::new(HashSet::new());
        static IMAGES: RefCell<Vec<MonImage>> = const { RefCell::new(Vec::new()) };
        static VIRT: Cell<(i32, i32, i32, i32)> = const { Cell::new((0, 0, 1, 1)) };
    }

    unsafe extern "system" fn win_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
        RAW.with(|c| c.set(c.get() + 1));
        if id_object != OBJID_WINDOW.0 || id_child != CHILDID_SELF as i32 || hwnd.is_invalid() {
            return;
        }
        let key = hwnd.0 as isize;
        if event == EVENT_OBJECT_DESTROY {
            // The window is gone; GetAncestor can't tell us anything. Keep it only if we were tracking it.
            if !KNOWN.with(|k| k.borrow().contains(&key)) {
                return;
            }
        } else if unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd {
            return;
        }
        FILTERED.with(|c| c.set(c.get() + 1));
        QUEUE.with(|q| q.borrow_mut().push((event, key)));
    }

    // ── which windows count (ADR §2.2) ───────────────────────────

    #[derive(Default, Clone, Copy)]
    struct Excluded {
        click_through: u32,
        transparent_layered: u32,
        cloaked: u32,
    }

    fn class_is(h: HWND, name: &str) -> bool {
        let mut buf = [0u16; 32];
        // SAFETY: local buffer. Used for matching the shell's own desktop windows only; never logged.
        let n = unsafe { GetClassNameW(h, &mut buf) };
        n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == name
    }

    fn frame_rect(h: HWND) -> Option<RECT> {
        let mut r = RECT::default();
        // SAFETY: writes one RECT.
        let ok = unsafe { DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32) }.is_ok();
        if !ok && unsafe { GetWindowRect(h, &mut r) }.is_err() {
            return None;
        }
        (r.right > r.left && r.bottom > r.top).then_some(r)
    }

    struct Filter {
        own_pid: u32,
        shell: HWND,
    }

    impl Filter {
        /// `Some(rect)` if the window covers desktop: visible, top-level, not cloaked, not minimized, not ours, not the
        /// shell's desktop, not click-through, not a fully transparent layered window.
        fn counts(&self, h: HWND, ex_tally: Option<&mut Excluded>) -> Option<RECT> {
            unsafe {
                if !IsWindow(Some(h)).as_bool() || !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                    return None;
                }
                if GetAncestor(h, GA_ROOT) != h || h == self.shell {
                    return None;
                }
                let mut pid = 0u32;
                GetWindowThreadProcessId(h, Some(&mut pid));
                if pid == self.own_pid || class_is(h, "WorkerW") || class_is(h, "Progman") {
                    return None;
                }
                let mut cloaked = 0u32;
                let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
                let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
                let mut tally = ex_tally;
                if cloaked != 0 {
                    if let Some(t) = tally.as_deref_mut() {
                        t.cloaked += 1;
                    }
                    return None;
                }
                if ex & WS_EX_TRANSPARENT.0 != 0 {
                    if let Some(t) = tally.as_deref_mut() {
                        t.click_through += 1;
                    }
                    return None;
                }
                if ex & WS_EX_LAYERED.0 != 0 {
                    let mut alpha = 255u8;
                    let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
                    if GetLayeredWindowAttributes(h, None, Some(&mut alpha), Some(&mut flags)).is_ok() && flags.0 & LWA_ALPHA.0 != 0 && alpha == 0 {
                        if let Some(t) = tally.as_deref_mut() {
                            t.transparent_layered += 1;
                        }
                        return None;
                    }
                }
            }
            frame_rect(h)
        }
    }

    unsafe extern "system" fn enum_top(h: HWND, lp: LPARAM) -> BOOL {
        unsafe { (*(lp.0 as *mut Vec<isize>)).push(h.0 as isize) };
        BOOL(1)
    }

    fn top_levels() -> Vec<HWND> {
        let mut v: Vec<isize> = Vec::new();
        // SAFETY: the callback only appends to `v`.
        unsafe {
            let _ = EnumWindows(Some(enum_top), LPARAM(&mut v as *mut Vec<isize> as isize));
        }
        v.into_iter().map(|x| HWND(x as *mut _)).collect()
    }

    fn rpx(r: &RECT) -> RectPx {
        RectPx::new(r.left, r.top, r.right, r.bottom)
    }

    fn infos(mons: &[Mon]) -> Vec<MonitorInfo> {
        mons.iter()
            .map(|m| MonitorInfo { id: MonitorId(m.index as u64), bounds: rpx(&m.rect), work: rpx(&m.work), dpi: m.dpi, primary: m.primary })
            .collect()
    }

    fn id(h: isize) -> ExtWindowId {
        ExtWindowId(h as usize as u64)
    }

    // ── debug window ─────────────────────────────────────────────

    struct MonImage {
        rect: RECT,
        cols: i32,
        rows: i32,
        px: Vec<u32>,
    }

    const BARE: u32 = 0x00_1e_6a_3a; // 0x00RRGGBB as a BGRA DIB word: dark green
    const COVERED: u32 = 0x00_58_58_58;

    fn render_images(grid: &OcclusionGrid, mons: &[Mon]) {
        let imgs: Vec<MonImage> = mons
            .iter()
            .filter_map(|m| {
                let mid = MonitorId(m.index as u64);
                let (cols, rows) = grid.dims(mid)?;
                let mut px = Vec::with_capacity(cols as usize * rows as usize);
                for cy in 0..rows {
                    for cx in 0..cols {
                        px.push(if grid.is_bare(CellId { monitor: mid, cx, cy }) { BARE } else { COVERED });
                    }
                }
                Some(MonImage { rect: m.rect, cols: cols as i32, rows: rows as i32, px })
            })
            .collect();
        IMAGES.with(|i| *i.borrow_mut() = imgs);
    }

    unsafe extern "system" fn debug_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                unsafe {
                    let hdc = BeginPaint(h, &mut ps);
                    let mut cr = RECT::default();
                    let _ = GetClientRect(h, &mut cr);
                    let bg = CreateSolidBrush(COLORREF(0x000000));
                    FillRect(hdc, &cr, bg);
                    let _ = DeleteObject(HGDIOBJ(bg.0));
                    SetStretchBltMode(hdc, COLORONCOLOR);
                    let (vx, vy, vw, vh) = VIRT.with(|v| v.get());
                    let s = ((cr.right as f32) / vw as f32).min((cr.bottom as f32) / vh as f32);
                    IMAGES.with(|imgs| {
                        for im in imgs.borrow().iter() {
                            let bmi = BITMAPINFO {
                                bmiHeader: BITMAPINFOHEADER {
                                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                                    biWidth: im.cols,
                                    biHeight: -im.rows, // top-down
                                    biPlanes: 1,
                                    biBitCount: 32,
                                    biCompression: BI_RGB.0,
                                    ..Default::default()
                                },
                                ..Default::default()
                            };
                            let dx = ((im.rect.left - vx) as f32 * s) as i32;
                            let dy = ((im.rect.top - vy) as f32 * s) as i32;
                            let dw = ((im.rect.right - im.rect.left) as f32 * s) as i32;
                            let dh = ((im.rect.bottom - im.rect.top) as f32 * s) as i32;
                            StretchDIBits(hdc, dx, dy, dw - 1, dh - 1, 0, 0, im.cols, im.rows, Some(im.px.as_ptr() as *const _), &bmi, DIB_RGB_COLORS, SRCCOPY);
                        }
                    });
                    let _ = EndPaint(h, &ps);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_CLOSE => {
                EXIT.store(true, Ordering::SeqCst);
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
        }
    }

    // ── tracker ──────────────────────────────────────────────────

    #[derive(Default)]
    struct Second {
        applied: u64,
        cells_changed: u64,
        drag_samples: u64,
        cells_swept: u64,
        max_jump: i32,
        foreground: u64,
    }

    struct Tracker {
        filter: Filter,
        mons: Vec<Mon>,
        grid: OcclusionGrid,
        tracked: HashMap<isize, RECT>,
        drag: Option<isize>,
        drag_prev: Option<RECT>,
        sec: Second,
        grid_dirty: bool,
        totals: (u64, u64),
        peak: (u64, u64),
        mismatch_checks: u32,
        mismatch_dirty_checks: u32,
        max_false_covered: usize,
        max_false_bare: usize,
    }

    impl Tracker {
        fn new(mons: Vec<Mon>) -> Self {
            let own_pid = unsafe { GetCurrentProcessId() };
            let shell = unsafe { GetShellWindow() };
            let scale = art_scale_for(mons.iter().find(|m| m.primary).map(|m| m.dpi).unwrap_or(96));
            let grid = OcclusionGrid::new(&infos(&mons), CELL_ART_PX, scale);
            log!("grid: {} monitor(s), cell {} px (art scale {})", mons.len(), grid.cell_px(), scale);
            Self {
                filter: Filter { own_pid, shell },
                mons,
                grid,
                tracked: HashMap::new(),
                drag: None,
                drag_prev: None,
                sec: Second::default(),
                grid_dirty: true,
                totals: (0, 0),
                peak: (0, 0),
                mismatch_checks: 0,
                mismatch_dirty_checks: 0,
                max_false_covered: 0,
                max_false_bare: 0,
            }
        }

        fn set_rect(&mut self, h: isize, r: Option<RECT>, cause: RectCause) {
            let d = self.grid.apply(id(h), r.as_ref().map(rpx), cause);
            self.sec.applied += 1;
            let n = (d.covered.len() + d.bared.len()) as u64;
            if n > 0 {
                self.sec.cells_changed += n;
                self.grid_dirty = true;
            }
            match r {
                Some(r) => {
                    self.tracked.insert(h, r);
                    KNOWN.with(|k| k.borrow_mut().insert(h));
                }
                None => {
                    self.tracked.remove(&h);
                    KNOWN.with(|k| k.borrow_mut().remove(&h));
                }
            }
        }

        fn snapshot(&self, tally: Option<&mut Excluded>) -> HashMap<isize, RECT> {
            let mut out = HashMap::new();
            let mut t = tally;
            for h in top_levels() {
                if let Some(r) = self.filter.counts(h, t.as_deref_mut()) {
                    out.insert(h.0 as isize, r);
                }
            }
            out
        }

        fn boot(&mut self) {
            let mut ex = Excluded::default();
            let snap = self.snapshot(Some(&mut ex));
            let (mut bordered, mut border_sum) = (0, 0);
            for (h, r) in &snap {
                let mut wr = RECT::default();
                if unsafe { GetWindowRect(HWND(*h as *mut _), &mut wr) }.is_ok() && wr != *r {
                    bordered += 1;
                    border_sum += (r.left - wr.left).max(0) + (wr.right - r.right).max(0) + (wr.bottom - r.bottom).max(0);
                }
                self.set_rect(*h, Some(*r), RectCause::Shown);
            }
            log!(
                "boot: {} window(s) count toward coverage; excluded: {} click-through, {} fully transparent layered, {} cloaked",
                snap.len(),
                ex.click_through,
                ex.transparent_layered,
                ex.cloaked
            );
            log!(
                "frame bounds: {bordered} window(s) have invisible borders outside their DWM frame (avg {:.1} px per window, left+right+bottom); the grid uses the DWM frame",
                if bordered > 0 { border_sum as f32 / bordered as f32 } else { 0.0 }
            );
        }

        fn drain(&mut self) {
            let events: Vec<(u32, isize)> = QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()));
            let mut dirty: Vec<(isize, RectCause)> = Vec::new();
            for (ev, h) in events {
                let cause = match ev {
                    EVENT_SYSTEM_FOREGROUND => {
                        self.sec.foreground += 1;
                        continue;
                    }
                    EVENT_SYSTEM_MOVESIZESTART => {
                        self.drag = Some(h);
                        self.drag_prev = self.tracked.get(&h).copied().or_else(|| frame_rect(HWND(h as *mut _)));
                        RectCause::DragStart
                    }
                    EVENT_SYSTEM_MOVESIZEEND => {
                        if self.drag == Some(h) {
                            self.drag = None;
                        }
                        RectCause::DragEnd
                    }
                    EVENT_SYSTEM_MINIMIZESTART => RectCause::Minimized,
                    EVENT_SYSTEM_MINIMIZEEND => RectCause::Restored,
                    EVENT_OBJECT_CREATE | EVENT_OBJECT_SHOW => RectCause::Shown,
                    EVENT_OBJECT_HIDE => RectCause::Hidden,
                    EVENT_OBJECT_DESTROY => RectCause::Destroyed,
                    EVENT_OBJECT_CLOAKED => RectCause::Cloaked,
                    EVENT_OBJECT_UNCLOAKED => RectCause::Uncloaked,
                    EVENT_OBJECT_LOCATIONCHANGE if self.drag == Some(h) => RectCause::DragSample,
                    EVENT_OBJECT_LOCATIONCHANGE => RectCause::Jumped,
                    _ => continue,
                };
                match dirty.iter_mut().find(|(x, _)| *x == h) {
                    // A drag start/end must not be hidden behind a later location event in the same frame.
                    Some((_, c)) if !matches!(*c, RectCause::DragStart | RectCause::DragEnd) => *c = cause,
                    Some(_) => {}
                    None => dirty.push((h, cause)),
                }
            }
            // Events are hints; DWM is the truth: read each dirty window's rect once per frame.
            for (h, cause) in dirty {
                let r = if cause == RectCause::Destroyed { None } else { self.filter.counts(HWND(h as *mut _), None) };
                if matches!(cause, RectCause::DragSample | RectCause::DragEnd) {
                    if let (Some(prev), Some(now)) = (self.drag_prev, r) {
                        if prev != now {
                            let swept = self.grid.sweep(rpx(&prev), rpx(&now));
                            self.sec.cells_swept += swept.len() as u64;
                            self.sec.drag_samples += 1;
                            let jump = (now.left - prev.left).abs().max((now.top - prev.top).abs());
                            self.sec.max_jump = self.sec.max_jump.max(jump);
                        }
                    }
                    self.drag_prev = r;
                }
                if r.is_some() || self.tracked.contains_key(&h) {
                    self.set_rect(h, r, cause);
                }
            }
        }

        fn log_second(&mut self) {
            let raw = RAW.with(|c| c.replace(0));
            let filt = FILTERED.with(|c| c.replace(0));
            self.totals.0 += raw;
            self.totals.1 += filt;
            self.peak = (self.peak.0.max(raw), self.peak.1.max(filt));
            let s = std::mem::take(&mut self.sec);
            log!(
                "1s: raw {raw} ev, filtered {filt} ev, tracked {}, applied {}, cells changed {}, drag samples {}, cells swept {}, max sample jump {} px, foreground changes {}",
                self.tracked.len(),
                s.applied,
                s.cells_changed,
                s.drag_samples,
                s.cells_swept,
                s.max_jump,
                s.foreground
            );
        }

        /// Full rescan into a fresh grid; compare bare/covered per cell. Doesn't correct the live grid, so drift accumulates
        /// and shows up.
        fn compare(&mut self, label: &str) {
            if self.drag.is_some() {
                log!("rescan ({label}): skipped, drag in progress");
                return;
            }
            self.drain();
            let mut ex = Excluded::default();
            let snap = self.snapshot(Some(&mut ex));
            let scale = art_scale_for(self.mons.iter().find(|m| m.primary).map(|m| m.dpi).unwrap_or(96));
            let mut fresh = OcclusionGrid::new(&infos(&self.mons), CELL_ART_PX, scale);
            for (h, r) in &snap {
                fresh.apply(id(*h), Some(rpx(r)), RectCause::Jumped);
            }
            let (mut false_covered, mut false_bare) = (0usize, 0usize);
            for m in &self.mons {
                let mid = MonitorId(m.index as u64);
                let Some((cols, rows)) = self.grid.dims(mid) else { continue };
                for cy in 0..rows {
                    for cx in 0..cols {
                        let c = CellId { monitor: mid, cx, cy };
                        match (self.grid.is_bare(c), fresh.is_bare(c)) {
                            (false, true) => false_covered += 1,
                            (true, false) => false_bare += 1,
                            _ => {}
                        }
                    }
                }
            }
            let live: HashSet<isize> = self.tracked.keys().copied().collect();
            let scan: HashSet<isize> = snap.keys().copied().collect();
            let only_live: Vec<_> = live.difference(&scan).copied().collect();
            let only_scan: Vec<_> = scan.difference(&live).copied().collect();
            let moved: Vec<_> = live.intersection(&scan).filter(|h| self.tracked[h] != snap[h]).copied().collect();
            self.mismatch_checks += 1;
            self.max_false_covered = self.max_false_covered.max(false_covered);
            self.max_false_bare = self.max_false_bare.max(false_bare);
            let bad = false_covered + false_bare > 0;
            if bad {
                self.mismatch_dirty_checks += 1;
            }
            log!(
                "rescan ({label}): {} | false-covered {false_covered} cells, false-bare {false_bare} cells | windows live {} / rescan {} (only live {}, only rescan {}, rect differs {}) | excluded overlays: {} click-through, {} transparent layered",
                if bad { "MISMATCH" } else { "match" },
                live.len(),
                scan.len(),
                only_live.len(),
                only_scan.len(),
                moved.len(),
                ex.click_through,
                ex.transparent_layered
            );
            for h in only_live.iter().take(3) {
                log!("  only live: window {} rect {}", common::hwnd_id(HWND(*h as *mut _)), common::fmt_rect(&self.tracked[h]));
            }
            for h in only_scan.iter().take(3) {
                log!("  only rescan: window {} rect {}", common::hwnd_id(HWND(*h as *mut _)), common::fmt_rect(&snap[h]));
            }
            for h in moved.iter().take(3) {
                log!("  rect differs: window {} live {} rescan {}", common::hwnd_id(HWND(*h as *mut _)), common::fmt_rect(&self.tracked[h]), common::fmt_rect(&snap[h]));
            }
            // Big covering windows, so overlay false positives can be spotted (rects and style flags only).
            for (h, r) in &snap {
                for m in &self.mons {
                    let iw = (r.right.min(m.rect.right) - r.left.max(m.rect.left)).max(0) as i64;
                    let ih = (r.bottom.min(m.rect.bottom) - r.top.max(m.rect.top)).max(0) as i64;
                    if iw * ih * 10 >= (m.w() as i64 * m.h() as i64) * 9 {
                        let ex = unsafe { GetWindowLongPtrW(HWND(*h as *mut _), GWL_EXSTYLE) } as u32;
                        let fg = unsafe { GetForegroundWindow() }.0 as isize == *h;
                        log!(
                            "  covers ≥90% of monitor {}: window {} rect {} flags[{}{}{}{}]{}",
                            m.index,
                            common::hwnd_id(HWND(*h as *mut _)),
                            common::fmt_rect(r),
                            if ex & WS_EX_TOPMOST.0 != 0 { " topmost" } else { "" },
                            if ex & WS_EX_LAYERED.0 != 0 { " layered" } else { "" },
                            if ex & WS_EX_TOOLWINDOW.0 != 0 { " toolwindow" } else { "" },
                            if ex & WS_EX_NOACTIVATE.0 != 0 { " noactivate" } else { "" },
                            if fg { " (foreground)" } else { "" }
                        );
                    }
                }
            }
        }

        fn summary(&self, elapsed: Duration) {
            let secs = elapsed.as_secs_f64().max(1.0);
            log!(
                "summary @{:.0}s: raw {} ev ({:.1}/s avg, peak {}/s), filtered {} ev ({:.1}/s avg, peak {}/s), filter keeps {:.2}%; rescans {} with mismatch {} (worst: {} false-covered, {} false-bare cells)",
                secs,
                self.totals.0,
                self.totals.0 as f64 / secs,
                self.peak.0,
                self.totals.1,
                self.totals.1 as f64 / secs,
                self.peak.1,
                if self.totals.0 > 0 { 100.0 * self.totals.1 as f64 / self.totals.0 as f64 } else { 0.0 },
                self.mismatch_checks,
                self.mismatch_dirty_checks,
                self.max_false_covered,
                self.max_false_bare
            );
        }
    }

    fn set_virt(mons: &[Mon]) -> (i32, i32) {
        let vx = mons.iter().map(|m| m.rect.left).min().unwrap_or(0);
        let vy = mons.iter().map(|m| m.rect.top).min().unwrap_or(0);
        let vw = (mons.iter().map(|m| m.rect.right).max().unwrap_or(1) - vx).max(1);
        let vh = (mons.iter().map(|m| m.rect.bottom).max().unwrap_or(1) - vy).max(1);
        VIRT.with(|v| v.set((vx, vy, vw, vh)));
        (vw, vh)
    }

    pub fn main() {
        common::log_init(LOG_PATH);
        log!("BEZEL spike S10 (occlusion fidelity). Logs rects, ids and counts only.");
        common::log_os_version();
        common::set_per_monitor_v2();
        common::install_ctrl_handler();

        let mons = common::monitors();
        for m in &mons {
            log!("monitor {}: rect {} dpi {}{}", m.index, common::fmt_rect(&m.rect), m.dpi, if m.primary { " primary" } else { "" });
        }
        let (vw, vh) = set_virt(&mons);

        let hinst: windows::Win32::Foundation::HINSTANCE = unsafe { GetModuleHandleW(PCWSTR::null()) }.unwrap_or_default().into();
        let debug = unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(debug_proc),
                hInstance: hinst,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: w!("BezelS10Grid"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let ch = (DEBUG_W as i64 * vh as i64 / vw as i64) as i32;
            let mut r = RECT { left: 0, top: 0, right: DEBUG_W, bottom: ch };
            let _ = AdjustWindowRectEx(&mut r, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0));
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BezelS10Grid"),
                w!("BEZEL S10 grid (green = bare, grey = covered)"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                r.right - r.left,
                r.bottom - r.top,
                None,
                None,
                Some(hinst),
                None,
            )
        };
        let debug = match debug {
            Ok(h) => {
                unsafe {
                    let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
                }
                Some(h)
            }
            Err(e) => {
                log!("debug window failed ({e}); continuing without it");
                None
            }
        };

        let mut t = Tracker::new(mons);
        t.boot();

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
            .map(|(a, b)| unsafe { SetWinEventHook(*a, *b, None, Some(win_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS) })
            .collect();
        log!("winevent hooks: {}/{} installed (out-of-context, skip own process)", hooks.iter().filter(|h| !h.is_invalid()).count(), hooks.len());
        log!("keys: q quit (or close the grid window). Run ~10 minutes of normal use.");

        let keys = common::console_keys();
        let start = Instant::now();
        let mut last_frame = Instant::now();
        let mut last_sec = Instant::now();
        let mut last_cmp = Instant::now();
        let mut last_sum = Instant::now();
        let mut last_paint = Instant::now();
        'main: loop {
            let wait = (last_frame + FRAME).saturating_duration_since(Instant::now()).as_millis() as u32;
            unsafe {
                MsgWaitForMultipleObjectsEx(None, wait, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
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
            while let Ok(k) = keys.try_recv() {
                if k == 'q' {
                    break 'main;
                }
            }
            if last_frame.elapsed() >= FRAME {
                last_frame = Instant::now();
                t.drain();
            }
            if last_sec.elapsed() >= Duration::from_secs(1) {
                last_sec = Instant::now();
                t.log_second();
            }
            if last_cmp.elapsed() >= Duration::from_secs(5) {
                last_cmp = Instant::now();
                let mons = common::monitors();
                if !common::same_layout(&mons, &t.mons) {
                    log!("monitor layout changed: rebuilding grid");
                    set_virt(&mons);
                    t = Tracker::new(mons);
                    t.boot();
                } else {
                    t.compare("5s");
                }
            }
            if last_sum.elapsed() >= Duration::from_secs(60) {
                last_sum = Instant::now();
                t.summary(start.elapsed());
            }
            if t.grid_dirty && last_paint.elapsed() >= Duration::from_millis(100) {
                last_paint = Instant::now();
                t.grid_dirty = false;
                render_images(&t.grid, &t.mons);
                if let Some(h) = debug {
                    unsafe {
                        let _ = InvalidateRect(Some(h), None, false);
                    }
                }
            }
        }

        for h in hooks {
            if !h.is_invalid() {
                unsafe {
                    let _ = UnhookWinEvent(h);
                }
            }
        }
        t.compare("final");
        t.summary(start.elapsed());
        if let Some(h) = debug {
            unsafe {
                let _ = DestroyWindow(h);
            }
        }
        CLEANED.store(true, Ordering::SeqCst);
        log!("exit: hooks removed.");
    }
}
