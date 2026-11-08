//! Spike S9: desktop underlay (ADR-0001 §5.3, §9).
//!
//! Attaches one window per monitor in the desktop layer, beneath the icons, and draws a tinted checkerboard,
//! a moving square and a label on it. Opaque mode repaints the real wallpaper first (read-only); transparent
//! mode clears to alpha 0 so a live wallpaper underneath shows through.
//!
//! Console keys: t = toggle transparent on the current monitor, n = next monitor, r = force reattach,
//! p = switch presenter (DirectComposition ↔ flip swap chain) on the current monitor, q = quit.
//! Logs to spike_s9.log in the working directory.
//!
//! Invariant 6: this program never writes the wallpaper. No SPI_SETDESKWALLPAPER, no IDesktopWallpaper::Set*.

#[cfg(windows)]
#[macro_use]
mod common;

#[cfg(not(windows))]
fn main() {
    eprintln!("spike_s9_underlay is Windows-only");
}

#[cfg(windows)]
fn main() {
    imp::main();
}

#[cfg(windows)]
mod imp {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::mem::ManuallyDrop;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use windows::core::{w, Interface, BOOL, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{GENERIC_READ, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Direct2D::Common::{
        D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
    };
    use windows::Win32::Graphics::Direct2D::{
        D2D1CreateFactory, ID2D1Bitmap1, ID2D1BitmapBrush1, ID2D1DeviceContext, ID2D1Factory1, ID2D1Image,
        ID2D1SolidColorBrush, D2D1_BITMAP_BRUSH_PROPERTIES1, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE,
        D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE,
        D2D1_EXTEND_MODE_WRAP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_INTERPOLATION_MODE_LINEAR,
        D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
    };
    use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1};
    use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION};
    use windows::Win32::Graphics::DirectComposition::{DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual};
    use windows::Win32::Graphics::DirectWrite::{
        DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
        DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_ALPHA_MODE, DXGI_ALPHA_MODE_IGNORE, DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Graphics::Dxgi::{
        IDXGIAdapter, IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH,
        DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
    };
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, EndPaint, MapWindowPoints, RedrawWindow, PAINTSTRUCT, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE,
    };
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, IWICPalette, WICBitmapDitherTypeNone,
        WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Shell::{
        DesktopWallpaper, IDesktopWallpaper, DESKTOP_WALLPAPER_POSITION, DWPOS_CENTER, DWPOS_FILL, DWPOS_FIT, DWPOS_SPAN,
        DWPOS_STRETCH, DWPOS_TILE,
    };
    use windows::Win32::UI::WindowsAndMessaging::*;

    use crate::common::{self, fmt_rect, hwnd_id, Mon, CLEANED, EXIT};

    const LOG_PATH: &str = "spike_s9.log";
    const FRAME: Duration = Duration::from_millis(33); // ~30 fps; not a perf spike
    const CELL_ART_PX: f32 = 16.0;
    /// WM_SETTINGCHANGE wParam meaning "the wallpaper changed" (SPI 0x0014). Compared only; never sent.
    const SPI_NOTIFY_DESKWALLPAPER: u32 = 0x0014;

    // ── globals the window procs, panic hook and ctrl handler can reach ──

    /// Underlay HWNDs (as isize) so every exit path can find them.
    static UNDERLAYS: Mutex<Vec<isize>> = Mutex::new(Vec::new());
    /// True while we destroy our own windows, so WM_DESTROY isn't read as Explorer tearing them down.
    static DETACHING: AtomicBool = AtomicBool::new(false);
    static TORN_DOWN: AtomicBool = AtomicBool::new(false);
    static RESET: AtomicBool = AtomicBool::new(false);
    static WALLPAPER_HINT: AtomicBool = AtomicBool::new(false);
    static DISPLAY_HINT: AtomicBool = AtomicBool::new(false);
    static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

    fn emergency_detach(why: &str) {
        let hwnds: Vec<isize> = std::mem::take(&mut *UNDERLAYS.lock().unwrap_or_else(|p| p.into_inner()));
        DETACHING.store(true, Ordering::SeqCst);
        let mut ok = 0;
        for h in &hwnds {
            // SAFETY: DestroyWindow only succeeds on the owning (main) thread; elsewhere it fails harmlessly and the
            // OS destroys the window when the process exits.
            if unsafe { DestroyWindow(HWND(*h as *mut _)) }.is_ok() {
                ok += 1;
            }
        }
        DETACHING.store(false, Ordering::SeqCst);
        common::log_line(&format!("emergency detach ({why}): destroyed {ok}/{} underlay windows", hwnds.len()));
    }

    // ── desktop discovery ────────────────────────────────────────

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Topo {
        Legacy,
        Raised,
    }

    #[derive(Clone, Copy, Debug)]
    struct Desktop {
        topo: Topo,
        progman: HWND,
        /// Where the underlays are parented: Progman (raised) or the WorkerW behind the icons (legacy).
        host: HWND,
        defview: HWND,
        /// Raised only: the shell's wallpaper WorkerW under Progman.
        shell_worker: Option<HWND>,
    }

    fn class_is(h: HWND, name: &str) -> bool {
        let mut buf = [0u16; 64];
        // SAFETY: plain read of a window class name into a local buffer. Used for matching only, never logged.
        let n = unsafe { GetClassNameW(h, &mut buf) };
        n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == name
    }

    unsafe extern "system" fn find_defview_host(h: HWND, lp: LPARAM) -> BOOL {
        if let Ok(dv) = unsafe { FindWindowExW(Some(h), None, w!("SHELLDLL_DefView"), PCWSTR::null()) } {
            unsafe { *(lp.0 as *mut (isize, isize)) = (h.0 as isize, dv.0 as isize) };
            return BOOL(0);
        }
        BOOL(1)
    }

    fn discover() -> Result<Desktop, String> {
        unsafe {
            let progman = FindWindowW(w!("Progman"), PCWSTR::null()).map_err(|e| format!("no Progman: {e}"))?;
            // Undocumented: asks Progman to spawn the WorkerW behind the icons (legacy). Harmless on raised.
            let mut res = 0usize;
            let r = SendMessageTimeoutW(progman, 0x052C, WPARAM(0), LPARAM(0), SMTO_NORMAL, 1000, Some(&mut res));
            if r.0 == 0 {
                log!("0x052C to Progman timed out or failed");
            }
            let ex = GetWindowLongPtrW(progman, GWL_EXSTYLE) as u32;
            let noredir = ex & WS_EX_NOREDIRECTIONBITMAP.0 != 0;
            let dv_in_progman = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null()).ok();
            log!("probe: Progman {} noredirectionbitmap={} icons-in-Progman={}", hwnd_id(progman), noredir, dv_in_progman.is_some());

            if let (true, Some(dv)) = (noredir, dv_in_progman) {
                let shell_worker = FindWindowExW(Some(progman), None, w!("WorkerW"), PCWSTR::null()).ok();
                return Ok(Desktop { topo: Topo::Raised, progman, host: progman, defview: dv, shell_worker });
            }

            let mut found: (isize, isize) = (0, 0);
            let _ = EnumWindows(Some(find_defview_host), LPARAM(&mut found as *mut (isize, isize) as isize));
            if found.0 == 0 {
                if let Some(dv) = dv_in_progman {
                    found = (progman.0 as isize, dv.0 as isize);
                } else {
                    return Err("no SHELLDLL_DefView anywhere".into());
                }
            }
            let icons_host = HWND(found.0 as *mut _);
            let worker = FindWindowExW(None, Some(icons_host), w!("WorkerW"), PCWSTR::null())
                .map_err(|_| "legacy: no WorkerW behind the icons (0x052C didn't spawn one)".to_string())?;
            Ok(Desktop { topo: Topo::Legacy, progman, host: worker, defview: HWND(found.1 as *mut _), shell_worker: None })
        }
    }

    unsafe extern "system" fn collect_child(h: HWND, lp: LPARAM) -> BOOL {
        unsafe { (*(lp.0 as *mut Vec<isize>)).push(h.0 as isize) };
        BOOL(1)
    }

    /// Screen rects of visible windows in the desktop layer that aren't the shell's and aren't ours:
    /// a live wallpaper (Wallpaper Engine, Lively) or a desktop-pinned widget.
    fn foreign_rects(d: &Desktop) -> Vec<RECT> {
        let ours: Vec<isize> = UNDERLAYS.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut all: Vec<isize> = Vec::new();
        let mut roots = vec![d.host];
        if d.progman != d.host {
            roots.push(d.progman);
        }
        for r in roots {
            // SAFETY: the callback only appends to `all`.
            unsafe {
                let _ = EnumChildWindows(Some(r), Some(collect_child), LPARAM(&mut all as *mut Vec<isize> as isize));
            }
        }
        all.sort_unstable();
        all.dedup();
        let mut out = Vec::new();
        for raw in all {
            let h = HWND(raw as *mut _);
            if ours.contains(&raw) || h == d.defview {
                continue;
            }
            unsafe {
                if !IsWindowVisible(h).as_bool() || IsChild(d.defview, h).as_bool() {
                    continue;
                }
            }
            if class_is(h, "WorkerW") || class_is(h, "SHELLDLL_DefView") {
                continue;
            }
            let mut rc = RECT::default();
            if unsafe { GetWindowRect(h, &mut rc) }.is_ok() && rc.right > rc.left && rc.bottom > rc.top {
                out.push(rc);
            }
        }
        out
    }

    fn occupied(m: &Mon, foreign: &[RECT]) -> usize {
        let area = (m.w() as i64) * (m.h() as i64);
        foreign
            .iter()
            .filter(|r| {
                let w = (r.right.min(m.rect.right) - r.left.max(m.rect.left)).max(0) as i64;
                let h = (r.bottom.min(m.rect.bottom) - r.top.max(m.rect.top)).max(0) as i64;
                area > 0 && w * h * 2 >= area
            })
            .count()
    }

    // ── window procs ─────────────────────────────────────────────

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

    unsafe extern "system" fn control_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let tbc = TASKBAR_CREATED.load(Ordering::SeqCst);
        if tbc != 0 && msg == tbc {
            common::log_line("TaskbarCreated: Explorer restarted");
            RESET.store(true, Ordering::SeqCst);
            return LRESULT(0);
        }
        match msg {
            WM_SETTINGCHANGE => {
                if wp.0 as u32 == SPI_NOTIFY_DESKWALLPAPER {
                    WALLPAPER_HINT.store(true, Ordering::SeqCst);
                }
                LRESULT(0)
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                DISPLAY_HINT.store(true, Ordering::SeqCst);
                LRESULT(0)
            }
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                if wp.0 != 0 {
                    EXIT.store(true, Ordering::SeqCst);
                    emergency_detach("WM_ENDSESSION");
                    CLEANED.store(true, Ordering::SeqCst);
                }
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
        }
    }

    // ── GPU ──────────────────────────────────────────────────────

    struct Gpu {
        d3d: ID3D11Device,
        factory: IDXGIFactory2,
        dc: ID2D1DeviceContext,
        dcomp: Option<IDCompositionDevice>,
        dwrite: IDWriteFactory,
        wic: IWICImagingFactory,
    }

    impl Gpu {
        fn new() -> windows::core::Result<Self> {
            unsafe {
                let mut d3d: Option<ID3D11Device> = None;
                D3D11CreateDevice(
                    None::<&IDXGIAdapter>,
                    D3D_DRIVER_TYPE_HARDWARE,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0]),
                    D3D11_SDK_VERSION,
                    Some(&mut d3d),
                    None,
                    None,
                )?;
                let d3d = d3d.expect("D3D11CreateDevice returned no device");
                let dxgi: IDXGIDevice = d3d.cast()?;
                let factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
                let d2f: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
                let dc = d2f.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
                dc.SetDpi(96.0, 96.0); // DIPs = physical px
                let dcomp = match DCompositionCreateDevice::<_, IDCompositionDevice>(&dxgi) {
                    Ok(d) => Some(d),
                    Err(e) => {
                        log!("DCompositionCreateDevice failed: {e}");
                        None
                    }
                };
                let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
                let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
                Ok(Self { d3d, factory, dc, dcomp, dwrite, wic })
            }
        }
    }

    // ── wallpaper (read-only) ────────────────────────────────────

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    enum Fit {
        Center,
        Tile,
        Stretch,
        Fit,
        Fill,
        Span,
    }

    fn fit_of(p: DESKTOP_WALLPAPER_POSITION) -> Fit {
        match p {
            DWPOS_CENTER => Fit::Center,
            DWPOS_TILE => Fit::Tile,
            DWPOS_STRETCH => Fit::Stretch,
            DWPOS_FIT => Fit::Fit,
            DWPOS_SPAN => Fit::Span,
            DWPOS_FILL => Fit::Fill,
            _ => Fit::Fill,
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct WallDesc {
        path: Option<String>,
        fit: Fit,
        bg: u32,
        /// Hash of (path, mtime, size): changes when the image changes, without logging the path.
        sig: u64,
    }

    fn read_wallpaper(dw: &IDesktopWallpaper, m: &Mon) -> Option<WallDesc> {
        unsafe {
            let count = dw.GetMonitorDevicePathCount().ok()?;
            for i in 0..count {
                let Ok(id) = dw.GetMonitorDevicePathAt(i) else { continue };
                let rc = dw.GetMonitorRECT(PCWSTR(id.0));
                let matches = rc.map(|r| r == m.rect).unwrap_or(false);
                let desc = if matches {
                    let path = dw.GetWallpaper(PCWSTR(id.0)).ok().map(|p| {
                        let s = p.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(p.0 as *const _));
                        s
                    });
                    let path = path.filter(|s| !s.is_empty());
                    let fit = fit_of(dw.GetPosition().unwrap_or(DWPOS_FILL));
                    let bg = dw.GetBackgroundColor().map(|c| c.0).unwrap_or(0);
                    let mut h = DefaultHasher::new();
                    path.hash(&mut h);
                    if let Some(meta) = path.as_ref().and_then(|p| std::fs::metadata(p).ok()) {
                        meta.len().hash(&mut h);
                        meta.modified().ok().hash(&mut h);
                    }
                    fit.hash(&mut h);
                    bg.hash(&mut h);
                    Some(WallDesc { path, fit, bg, sig: h.finish() })
                } else {
                    None
                };
                CoTaskMemFree(Some(id.0 as *const _));
                if desc.is_some() {
                    return desc;
                }
            }
            None
        }
    }

    fn ext_of(p: &str) -> String {
        std::path::Path::new(p).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "?".into())
    }

    struct Wall {
        desc: WallDesc,
        bmp: Option<(ID2D1Bitmap1, u32, u32)>,
    }

    fn load_image(gpu: &Gpu, path: &str) -> windows::core::Result<(ID2D1Bitmap1, u32, u32)> {
        unsafe {
            let dec = gpu.wic.CreateDecoderFromFilename(&HSTRING::from(path), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
            let frame = dec.GetFrame(0)?;
            let conv = gpu.wic.CreateFormatConverter()?;
            conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None::<&IWICPalette>, 0.0, WICBitmapPaletteTypeMedianCut)?;
            let (mut w, mut h) = (0u32, 0u32);
            conv.GetSize(&mut w, &mut h)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                colorContext: ManuallyDrop::new(None),
            };
            let bmp = gpu.dc.CreateBitmapFromWicBitmap(&conv, Some(&props))?;
            Ok((bmp, w, h))
        }
    }

    fn colorref(c: u32) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: (c & 0xff) as f32 / 255.0, g: ((c >> 8) & 0xff) as f32 / 255.0, b: ((c >> 16) & 0xff) as f32 / 255.0, a: 1.0 }
    }

    fn rectf(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F {
        D2D_RECT_F { left: l, top: t, right: r, bottom: b }
    }

    // ── presenters ───────────────────────────────────────────────

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Mode {
        Opaque,
        Transparent,
    }

    enum Presenter {
        DComp { _target: IDCompositionTarget, visual: IDCompositionVisual, sc: IDXGISwapChain1 },
        Flip { sc: IDXGISwapChain1 },
    }

    impl Presenter {
        fn name(&self) -> &'static str {
            match self {
                Presenter::DComp { .. } => "DComp",
                Presenter::Flip { .. } => "flip",
            }
        }
        fn sc(&self) -> &IDXGISwapChain1 {
            match self {
                Presenter::DComp { sc, .. } | Presenter::Flip { sc } => sc,
            }
        }
    }

    fn sc_desc(w: u32, h: u32, alpha: DXGI_ALPHA_MODE, seq: bool) -> DXGI_SWAP_CHAIN_DESC1 {
        DXGI_SWAP_CHAIN_DESC1 {
            Width: w,
            Height: h,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: if seq { DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL } else { DXGI_SWAP_EFFECT_FLIP_DISCARD },
            AlphaMode: alpha,
            Flags: 0,
        }
    }

    fn comp_swapchain(gpu: &Gpu, w: u32, h: u32, mode: Mode) -> windows::core::Result<IDXGISwapChain1> {
        let alpha = if mode == Mode::Transparent { DXGI_ALPHA_MODE_PREMULTIPLIED } else { DXGI_ALPHA_MODE_IGNORE };
        unsafe { gpu.factory.CreateSwapChainForComposition(&gpu.d3d, &sc_desc(w, h, alpha, true), None) }
    }

    fn make_presenter(gpu: &Gpu, hwnd: HWND, w: u32, h: u32, mode: Mode, force_flip: bool, mi: usize) -> Option<Presenter> {
        if !force_flip {
            if let Some(dc) = &gpu.dcomp {
                let r = (|| unsafe {
                    let target = dc.CreateTargetForHwnd(hwnd, true)?;
                    let sc = comp_swapchain(gpu, w, h, mode)?;
                    let visual = dc.CreateVisual()?;
                    visual.SetContent(&sc)?;
                    target.SetRoot(&visual)?;
                    dc.Commit()?;
                    Ok::<_, windows::core::Error>(Presenter::DComp { _target: target, visual, sc })
                })();
                match r {
                    Ok(p) => return Some(p),
                    Err(e) => log!("monitor {mi}: DirectComposition target FAILED ({e}); falling back to flip swap chain"),
                }
            }
        }
        match unsafe { gpu.factory.CreateSwapChainForHwnd(&gpu.d3d, hwnd, &sc_desc(w, h, DXGI_ALPHA_MODE_IGNORE, false), None, None) } {
            Ok(sc) => Some(Presenter::Flip { sc }),
            Err(e) => {
                log!("monitor {mi}: flip swap chain FAILED too ({e}); this monitor has no presenter");
                None
            }
        }
    }

    // ── one underlay per monitor ─────────────────────────────────

    const TINTS: [(f32, f32, f32); 6] = [(0.10, 0.85, 0.80), (0.95, 0.30, 0.75), (1.00, 0.70, 0.10), (0.45, 0.95, 0.25), (0.35, 0.55, 1.00), (1.00, 0.35, 0.25)];

    struct Underlay {
        mi: usize,
        hwnd: HWND,
        w: u32,
        h: u32,
        scale: f32,
        mode: Mode,
        pres: Option<Presenter>,
        force_flip: bool,
        wall: Option<Wall>,
        checker: Option<ID2D1BitmapBrush1>,
        text: Option<IDWriteTextFormat>,
        sq: (f32, f32, f32, f32),
        frames: u32,
        fps: f32,
        fps_t: Instant,
        present_errors: u32,
    }

    fn make_checker(gpu: &Gpu, mi: usize, scale: f32) -> windows::core::Result<ID2D1BitmapBrush1> {
        let cell = (CELL_ART_PX * scale).round().max(1.0) as u32;
        let n = cell * 2;
        let (r, g, b) = TINTS[mi % TINTS.len()];
        let px = |a: f32| -> [u8; 4] { [(b * a * 255.0) as u8, (g * a * 255.0) as u8, (r * a * 255.0) as u8, (a * 255.0) as u8] };
        let (on, off) = (px(0.34), px(0.10));
        let mut data = Vec::with_capacity((n * n * 4) as usize);
        for y in 0..n {
            for x in 0..n {
                data.extend_from_slice(if ((x / cell) + (y / cell)) % 2 == 0 { &on } else { &off });
            }
        }
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: ManuallyDrop::new(None),
        };
        unsafe {
            let bmp = gpu.dc.CreateBitmap(D2D_SIZE_U { width: n, height: n }, Some(data.as_ptr() as *const _), n * 4, &props)?;
            let bp = D2D1_BITMAP_BRUSH_PROPERTIES1 {
                extendModeX: D2D1_EXTEND_MODE_WRAP,
                extendModeY: D2D1_EXTEND_MODE_WRAP,
                interpolationMode: D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
            };
            gpu.dc.CreateBitmapBrush(&bmp, Some(&bp), None)
        }
    }

    impl Underlay {
        fn draw(&mut self, gpu: &Gpu, label: &str, selected: bool, dt: f32) {
            let Some(pres) = self.pres.take() else { return };
            let transparent = self.mode == Mode::Transparent;
            let r = (|| unsafe {
                let surf: IDXGISurface = pres.sc().GetBuffer(0)?;
                let props = D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: if transparent { D2D1_ALPHA_MODE_PREMULTIPLIED } else { D2D1_ALPHA_MODE_IGNORE },
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                    colorContext: ManuallyDrop::new(None),
                };
                let target = gpu.dc.CreateBitmapFromDxgiSurface(&surf, Some(&props))?;
                let dc = &gpu.dc;
                dc.SetTarget(&target);
                dc.BeginDraw();
                let (w, h) = (self.w as f32, self.h as f32);
                if transparent {
                    dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
                } else {
                    let bg = self.wall.as_ref().map(|wl| wl.desc.bg).unwrap_or(0);
                    dc.Clear(Some(&colorref(bg)));
                    if let Some(wl) = &self.wall {
                        if let Some((bmp, iw, ih)) = &wl.bmp {
                            self.draw_wallpaper(dc, bmp, *iw as f32, *ih as f32, wl.desc.fit, w, h);
                        }
                    }
                }
                let full = rectf(0.0, 0.0, w, h);
                if let Some(ch) = &self.checker {
                    dc.FillRectangle(&full, ch);
                }
                // Moving square.
                let s = 64.0 * self.scale;
                let (x, y, vx, vy) = &mut self.sq;
                *x += *vx * dt;
                *y += *vy * dt;
                if *x < 0.0 || *x + s > w {
                    *vx = -*vx;
                    *x = x.clamp(0.0, (w - s).max(0.0));
                }
                if *y < 0.0 || *y + s > h {
                    *vy = -*vy;
                    *y = y.clamp(0.0, (h - s).max(0.0));
                }
                let (tr, tg, tb) = TINTS[self.mi % TINTS.len()];
                let tint: ID2D1SolidColorBrush = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: tr, g: tg, b: tb, a: 1.0 }, None)?;
                let white: ID2D1SolidColorBrush = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
                let black: ID2D1SolidColorBrush = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.85 }, None)?;
                dc.FillRectangle(&rectf(*x, *y, *x + s, *y + s), &tint);
                dc.FillRectangle(&rectf(*x + s * 0.25, *y + s * 0.25, *x + s * 0.75, *y + s * 0.75), &white);
                if selected {
                    let b = 6.0 * self.scale;
                    for r in [rectf(0.0, 0.0, w, b), rectf(0.0, h - b, w, h), rectf(0.0, 0.0, b, h), rectf(w - b, 0.0, w, h)] {
                        dc.FillRectangle(&r, &tint);
                    }
                }
                if let Some(tf) = &self.text {
                    let text: Vec<u16> = label.encode_utf16().collect();
                    let (lx, ly) = (48.0 * self.scale, 48.0 * self.scale);
                    let o = 2.0 * self.scale;
                    dc.DrawText(&text, tf, &rectf(lx + o, ly + o, w - lx, h), &black, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    dc.DrawText(&text, tf, &rectf(lx, ly, w - lx, h), &white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                }
                dc.EndDraw(None, None)?;
                dc.SetTarget(None::<&ID2D1Image>);
                drop(target);
                drop(surf);
                pres.sc().Present(0, DXGI_PRESENT(0)).ok()
            })();
            if let Err(e) = r {
                self.present_errors += 1;
                if self.present_errors <= 5 || self.present_errors % 300 == 0 {
                    log!("monitor {}: draw/present error #{} ({e})", self.mi, self.present_errors);
                }
                unsafe { gpu.dc.SetTarget(None::<&ID2D1Image>) };
            }
            self.pres = Some(pres);
            self.frames += 1;
            let el = self.fps_t.elapsed().as_secs_f32();
            if el >= 1.0 {
                self.fps = self.frames as f32 / el;
                self.frames = 0;
                self.fps_t = Instant::now();
            }
        }

        /// Approximates Windows' own fit modes. Span is relative to the virtual screen (set via `span_origin`).
        #[allow(clippy::too_many_arguments)]
        unsafe fn draw_wallpaper(&self, dc: &ID2D1DeviceContext, bmp: &ID2D1Bitmap1, iw: f32, ih: f32, fit: Fit, w: f32, h: f32) {
            let draw = |d: D2D_RECT_F| unsafe { dc.DrawBitmap(bmp, Some(&d), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
            match fit {
                Fit::Stretch => draw(rectf(0.0, 0.0, w, h)),
                Fit::Center => {
                    let (x, y) = ((w - iw) / 2.0, (h - ih) / 2.0);
                    draw(rectf(x, y, x + iw, y + ih));
                }
                Fit::Tile => {
                    let mut y = 0.0;
                    while y < h {
                        let mut x = 0.0;
                        while x < w {
                            draw(rectf(x, y, x + iw, y + ih));
                            x += iw.max(1.0);
                        }
                        y += ih.max(1.0);
                    }
                }
                Fit::Fit | Fit::Fill => {
                    let s = if fit == Fit::Fit { (w / iw).min(h / ih) } else { (w / iw).max(h / ih) };
                    let (dw, dh) = (iw * s, ih * s);
                    let (x, y) = ((w - dw) / 2.0, (h - dh) / 2.0);
                    draw(rectf(x, y, x + dw, y + dh));
                }
                Fit::Span => {
                    let (vx, vy, vw, vh) = SPAN.with(|s| *s.borrow());
                    let s = (vw / iw).max(vh / ih);
                    let (dw, dh) = (iw * s, ih * s);
                    let (x, y) = ((vw - dw) / 2.0 - vx, (vh - dh) / 2.0 - vy);
                    draw(rectf(x, y, x + dw, y + dh));
                }
            }
        }
    }

    thread_local! {
        /// For span fit: (this monitor's offset from the virtual-screen origin x, y; virtual width, height). Set per draw.
        static SPAN: std::cell::RefCell<(f32, f32, f32, f32)> = const { std::cell::RefCell::new((0.0, 0.0, 0.0, 0.0)) };
    }

    // ── app ──────────────────────────────────────────────────────

    struct App {
        gpu: Gpu,
        wp: Option<IDesktopWallpaper>,
        hinst: windows::Win32::Foundation::HINSTANCE,
        desk: Option<Desktop>,
        mons: Vec<Mon>,
        unders: Vec<Underlay>,
        cur: usize,
        /// Per-monitor preferences that survive a reattach (index-keyed; reset when the layout changes).
        want_transparent: Vec<Option<bool>>,
        want_flip: Vec<bool>,
        occupants: Vec<usize>,
        z_fixes: u32,
        attaches: u32,
    }

    impl App {
        fn attach_all(&mut self, why: &str) -> bool {
            let t0 = Instant::now();
            let desk = match discover() {
                Ok(d) => d,
                Err(e) => {
                    log!("attach ({why}): desktop not ready: {e}");
                    return false;
                }
            };
            let mons = common::monitors();
            if !common::same_layout(&mons, &self.mons) {
                self.want_transparent = vec![None; mons.len()];
                self.want_flip = vec![false; mons.len()];
                self.cur = 0;
            }
            self.mons = mons;
            self.desk = Some(desk);
            log!(
                "desktop structure: {:?} (host {}, icons {}, shell wallpaper worker {})",
                desk.topo,
                hwnd_id(desk.host),
                hwnd_id(desk.defview),
                desk.shell_worker.map(hwnd_id).unwrap_or_else(|| "-".into())
            );
            let foreign = foreign_rects(&desk);
            self.occupants = self.mons.iter().map(|m| occupied(m, &foreign)).collect();
            for m in &self.mons {
                log!(
                    "monitor {}: {} rect {} work {} dpi {} ({}%){} | live wallpaper/widget: {}",
                    m.index,
                    m.device,
                    fmt_rect(&m.rect),
                    fmt_rect(&m.work),
                    m.dpi,
                    m.dpi * 100 / 96,
                    if m.primary { " primary" } else { "" },
                    if self.occupants[m.index] > 0 { format!("YES ({} window(s))", self.occupants[m.index]) } else { "no".into() }
                );
            }
            for i in 0..self.mons.len() {
                if let Some(u) = self.make_underlay(&desk, i) {
                    UNDERLAYS.lock().unwrap_or_else(|p| p.into_inner()).push(u.hwnd.0 as isize);
                    self.unders.push(u);
                }
            }
            self.fix_z(&desk, false);
            if let Some(dc) = &self.gpu.dcomp {
                let _ = unsafe { dc.Commit() };
            }
            self.attaches += 1;
            log!("attach #{} ({why}): {} underlay(s) in {} ms", self.attaches, self.unders.len(), t0.elapsed().as_millis());
            true
        }

        fn make_underlay(&mut self, desk: &Desktop, i: usize) -> Option<Underlay> {
            let m = self.mons[i].clone();
            let mut pts = [POINT { x: m.rect.left, y: m.rect.top }, POINT { x: m.rect.right, y: m.rect.bottom }];
            unsafe { MapWindowPoints(None, Some(desk.host), &mut pts) };
            let (x, y, w, h) = (pts[0].x, pts[0].y, (pts[1].x - pts[0].x).max(1), (pts[1].y - pts[0].y).max(1));
            let style = WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_DISABLED;
            let mk = |ex: WINDOW_EX_STYLE| unsafe {
                CreateWindowExW(ex, w!("BezelS9Underlay"), w!("BEZEL S9 underlay"), style, x, y, w, h, Some(desk.host), None, Some(self.hinst), None)
            };
            let hwnd = match mk(WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP) {
                Ok(h) => h,
                Err(e) => {
                    log!("monitor {i}: CreateWindowEx with NOREDIRECTIONBITMAP failed ({e}); retrying without");
                    match mk(WS_EX_NOACTIVATE) {
                        Ok(h) => h,
                        Err(e) => {
                            log!("monitor {i}: CreateWindowEx FAILED ({e})");
                            return None;
                        }
                    }
                }
            };
            let wdpi = unsafe { GetDpiForWindow(hwnd) };
            let scale = (m.dpi as f32 / 96.0).round().max(1.0);
            let want_t = self.want_transparent[i].unwrap_or(self.occupants.get(i).copied().unwrap_or(0) > 0);
            let mut mode = if want_t { Mode::Transparent } else { Mode::Opaque };
            let pres = make_presenter(&self.gpu, hwnd, w as u32, h as u32, mode, self.want_flip[i], i);
            if matches!(pres, Some(Presenter::Flip { .. })) && mode == Mode::Transparent {
                log!("monitor {i}: transparent mode needs the DComp presenter; flip swap chain is opaque only");
                mode = Mode::Opaque;
            }
            let wall = self.wp.as_ref().and_then(|dw| read_wallpaper(dw, &m)).map(|desc| self.load_wall(i, desc));
            let checker = make_checker(&self.gpu, i, scale).map_err(|e| log!("monitor {i}: checker brush failed ({e})")).ok();
            let text = unsafe {
                self.gpu.dwrite.CreateTextFormat(w!("Segoe UI"), None, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 22.0 * scale, w!("en-us"))
            }
            .ok();
            log!(
                "monitor {i}: underlay {} at host-client ({x},{y}) {w}x{h}, window dpi {wdpi}, presenter {}, mode {:?}",
                hwnd_id(hwnd),
                pres.as_ref().map(|p| p.name()).unwrap_or("NONE"),
                mode
            );
            Some(Underlay {
                mi: i,
                hwnd,
                w: w as u32,
                h: h as u32,
                scale,
                mode,
                pres,
                force_flip: self.want_flip[i],
                wall,
                checker,
                text,
                sq: (100.0 * scale, 100.0 * scale, 220.0 * scale, 160.0 * scale),
                frames: 0,
                fps: 0.0,
                fps_t: Instant::now(),
                present_errors: 0,
            })
        }

        fn load_wall(&self, i: usize, desc: WallDesc) -> Wall {
            let bmp = desc.path.as_ref().and_then(|p| match load_image(&self.gpu, p) {
                Ok(b) => Some(b),
                Err(e) => {
                    log!("monitor {i}: wallpaper image (.{}) failed to load ({e}); drawing the background color", ext_of(p));
                    None
                }
            });
            log!(
                "monitor {i}: wallpaper read: image {} fit {:?} background #{:06x}",
                match (&desc.path, &bmp) {
                    (Some(p), Some((_, w, h))) => format!("yes (.{}, {w}x{h})", ext_of(p)),
                    (Some(p), None) => format!("unreadable (.{})", ext_of(p)),
                    (None, _) => "none (solid color)".into(),
                },
                desc.fit,
                desc.bg
            );
            Wall { desc, bmp }
        }

        fn detach_all(&mut self, why: &str) {
            DETACHING.store(true, Ordering::SeqCst);
            let n = self.unders.len();
            for u in self.unders.drain(..) {
                let hwnd = u.hwnd;
                drop(u); // releases the swap chain and DComp target first
                unsafe {
                    if IsWindow(Some(hwnd)).as_bool() {
                        let _ = DestroyWindow(hwnd);
                    }
                }
            }
            UNDERLAYS.lock().unwrap_or_else(|p| p.into_inner()).clear();
            if let Some(dc) = &self.gpu.dcomp {
                let _ = unsafe { dc.Commit() };
            }
            // Ask the shell to repaint what we uncovered. A repaint only; the wallpaper setting is never touched.
            if let Some(d) = self.desk {
                unsafe {
                    if IsWindow(Some(d.host)).as_bool() {
                        let _ = RedrawWindow(Some(d.host), None, None, RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN);
                    }
                }
            }
            DETACHING.store(false, Ordering::SeqCst);
            TORN_DOWN.store(false, Ordering::SeqCst);
            log!("detached {n} underlay(s) ({why})");
        }

        /// Keeps our windows where they belong: directly below the icons (raised) or on top inside the WorkerW (legacy).
        /// Returns whether anything had drifted.
        fn fix_z(&mut self, d: &Desktop, check_only_first: bool) -> bool {
            let ours: Vec<HWND> = self.unders.iter().map(|u| u.hwnd).collect();
            if ours.is_empty() {
                return false;
            }
            let drifted = unsafe {
                match d.topo {
                    Topo::Raised => {
                        // Expect: icons, then every underlay, then anything else.
                        let mut h = GetWindow(d.defview, GW_HWNDNEXT).ok();
                        let mut seen = 0;
                        while let Some(x) = h {
                            if !ours.contains(&x) {
                                break;
                            }
                            seen += 1;
                            h = GetWindow(x, GW_HWNDNEXT).ok();
                        }
                        seen != ours.len()
                    }
                    Topo::Legacy => {
                        let mut h = GetWindow(ours[0], GW_HWNDFIRST).ok();
                        let mut seen = 0;
                        while let Some(x) = h {
                            if !ours.contains(&x) {
                                break;
                            }
                            seen += 1;
                            h = GetWindow(x, GW_HWNDNEXT).ok();
                        }
                        seen != ours.len()
                    }
                }
            };
            if drifted || !check_only_first {
                let mut after = match d.topo {
                    Topo::Raised => d.defview,
                    Topo::Legacy => HWND_TOP,
                };
                for h in &ours {
                    unsafe {
                        let _ = SetWindowPos(*h, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
                    }
                    after = *h;
                }
            }
            if drifted && check_only_first {
                self.z_fixes += 1;
                log!("z-order drift: something moved between the icons and our underlays; restored (fix #{})", self.z_fixes);
            }
            drifted
        }

        fn health(&mut self) {
            let Some(d) = self.desk else { return };
            let progman_now = unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.ok();
            let host_ok = unsafe { IsWindow(Some(d.host)).as_bool() } && progman_now == Some(d.progman);
            let mut lost = Vec::new();
            for u in &self.unders {
                let ok = unsafe { IsWindow(Some(u.hwnd)).as_bool() && GetParent(u.hwnd).ok() == Some(d.host) };
                if !ok {
                    lost.push(u.mi);
                }
            }
            if !host_ok || !lost.is_empty() || TORN_DOWN.load(Ordering::SeqCst) {
                log!("health: desktop host {} / lost underlays {:?}: reattaching", if host_ok { "ok" } else { "GONE" }, lost);
                RESET.store(true, Ordering::SeqCst);
                return;
            }
            self.fix_z(&d, true);
            let mons = common::monitors();
            if !common::same_layout(&mons, &self.mons) {
                log!("health: monitor layout changed: reattaching");
                RESET.store(true, Ordering::SeqCst);
                return;
            }
            // Wallpaper: poll (virtual-desktop switches and per-monitor changes don't always broadcast).
            let hinted = WALLPAPER_HINT.swap(false, Ordering::SeqCst);
            for k in 0..self.unders.len() {
                let mi = self.unders[k].mi;
                let now = self.wp.as_ref().and_then(|dw| read_wallpaper(dw, &self.mons[mi]));
                let before = self.unders[k].wall.as_ref().map(|w| w.desc.sig);
                if now.as_ref().map(|w| w.sig) != before {
                    log!("monitor {mi}: wallpaper changed{}", if hinted { " (WM_SETTINGCHANGE seen)" } else { " (poll)" });
                    let wall = now.map(|desc| self.load_wall(mi, desc));
                    self.unders[k].wall = wall;
                }
            }
            let foreign = foreign_rects(&d);
            let occ: Vec<usize> = self.mons.iter().map(|m| occupied(m, &foreign)).collect();
            if occ != self.occupants {
                log!("live wallpaper/widget occupancy changed: {:?} -> {:?}", self.occupants, occ);
                self.occupants = occ;
            }
        }

        fn label(&self, u: &Underlay) -> String {
            let topo = self.desk.map(|d| format!("{:?}", d.topo)).unwrap_or_default();
            format!(
                "BEZEL S9 · monitor {} · {} · {} · {} · {:.0} fps\nconsole: t transparent · n next monitor · r reattach · p presenter · q quit",
                u.mi,
                topo,
                u.pres.as_ref().map(|p| p.name()).unwrap_or("NO PRESENTER"),
                if u.mode == Mode::Transparent { "transparent" } else { "opaque" },
                u.fps
            )
        }

        fn toggle_transparent(&mut self) {
            let Some(k) = self.unders.iter().position(|u| u.mi == self.cur) else { return };
            let gpu = &self.gpu;
            let u = &mut self.unders[k];
            let next = if u.mode == Mode::Opaque { Mode::Transparent } else { Mode::Opaque };
            match &u.pres {
                Some(Presenter::DComp { visual, .. }) => {
                    let visual = visual.clone();
                    match comp_swapchain(gpu, u.w, u.h, next) {
                        Ok(sc) => unsafe {
                            let r = visual.SetContent(&sc).and_then(|_| gpu.dcomp.as_ref().unwrap().Commit());
                            match r {
                                Ok(()) => {
                                    if let Some(Presenter::DComp { sc: old, .. }) = &mut u.pres {
                                        *old = sc;
                                    }
                                    u.mode = next;
                                    self.want_transparent[u.mi] = Some(next == Mode::Transparent);
                                    log!("monitor {}: mode -> {:?}", u.mi, next);
                                }
                                Err(e) => log!("monitor {}: mode switch failed ({e})", u.mi),
                            }
                        },
                        Err(e) => log!("monitor {}: swap chain for {:?} failed ({e})", u.mi, next),
                    }
                }
                Some(Presenter::Flip { .. }) => log!("monitor {}: transparent mode needs the DComp presenter (press p to switch)", u.mi),
                None => log!("monitor {}: no presenter", u.mi),
            }
        }

        fn switch_presenter(&mut self) {
            let Some(k) = self.unders.iter().position(|u| u.mi == self.cur) else { return };
            let Some(d) = self.desk else { return };
            let mi = self.cur;
            self.want_flip[mi] = !self.unders[k].force_flip;
            log!("monitor {mi}: switching presenter to {} (recreating its window)", if self.want_flip[mi] { "flip" } else { "DComp" });
            DETACHING.store(true, Ordering::SeqCst);
            let old = self.unders.remove(k);
            let hwnd = old.hwnd;
            drop(old);
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            UNDERLAYS.lock().unwrap_or_else(|p| p.into_inner()).retain(|h| *h != hwnd.0 as isize);
            DETACHING.store(false, Ordering::SeqCst);
            if let Some(u) = self.make_underlay(&d, mi) {
                UNDERLAYS.lock().unwrap_or_else(|p| p.into_inner()).push(u.hwnd.0 as isize);
                self.unders.insert(k, u);
            }
            self.fix_z(&d, false);
        }

        fn frame(&mut self, dt: f32) {
            let vx = self.mons.iter().map(|m| m.rect.left).min().unwrap_or(0);
            let vy = self.mons.iter().map(|m| m.rect.top).min().unwrap_or(0);
            let vr = self.mons.iter().map(|m| m.rect.right).max().unwrap_or(0);
            let vb = self.mons.iter().map(|m| m.rect.bottom).max().unwrap_or(0);
            for k in 0..self.unders.len() {
                let label = self.label(&self.unders[k]);
                let mi = self.unders[k].mi;
                let m = &self.mons[mi];
                SPAN.with(|s| *s.borrow_mut() = ((m.rect.left - vx) as f32, (m.rect.top - vy) as f32, (vr - vx) as f32, (vb - vy) as f32));
                let selected = mi == self.cur;
                self.unders[k].draw(&self.gpu, &label, selected, dt);
            }
        }
    }

    fn register_classes(hinst: windows::Win32::Foundation::HINSTANCE) {
        unsafe {
            let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
            for (name, proc_) in [
                (w!("BezelS9Underlay"), underlay_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT),
                (w!("BezelS9Control"), control_proc),
            ] {
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(proc_),
                    hInstance: hinst,
                    hCursor: cursor,
                    lpszClassName: name,
                    ..Default::default()
                };
                RegisterClassExW(&wc);
            }
        }
    }

    pub fn main() {
        common::log_init(LOG_PATH);
        log!("BEZEL spike S9 (desktop underlay). Never writes the wallpaper.");
        common::log_os_version();
        common::set_per_monitor_v2();
        common::install_ctrl_handler();
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            common::log_line(&format!("PANIC: {info}"));
            emergency_detach("panic");
            prev(info);
        }));

        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let hinst: windows::Win32::Foundation::HINSTANCE = unsafe { GetModuleHandleW(PCWSTR::null()) }.unwrap_or_default().into();
        register_classes(hinst);
        TASKBAR_CREATED.store(unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }, Ordering::SeqCst);

        // Hidden top-level window: receives TaskbarCreated, WM_SETTINGCHANGE, WM_DISPLAYCHANGE, WM_ENDSESSION.
        let control = unsafe {
            CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("BezelS9Control"), w!("BEZEL S9 control"), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst), None)
        };
        log!("control window: {}", control.as_ref().map(|h| hwnd_id(*h)).unwrap_or_else(|e| format!("FAILED ({e})")));

        let gpu = match Gpu::new() {
            Ok(g) => g,
            Err(e) => {
                log!("GPU setup FAILED: {e}");
                return;
            }
        };
        log!("DirectComposition device: {}", if gpu.dcomp.is_some() { "ok" } else { "unavailable" });
        let wp: Option<IDesktopWallpaper> = unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }
            .map_err(|e| log!("IDesktopWallpaper unavailable ({e}); opaque mode will paint solid black"))
            .ok();

        let mut app = App {
            gpu,
            wp,
            hinst,
            desk: None,
            mons: Vec::new(),
            unders: Vec::new(),
            cur: 0,
            want_transparent: Vec::new(),
            want_flip: Vec::new(),
            occupants: Vec::new(),
            z_fixes: 0,
            attaches: 0,
        };
        let mut attached = app.attach_all("startup");
        let mut reset_since: Option<Instant> = if attached { None } else { Some(Instant::now()) };
        let keys = common::console_keys();
        log!("keys: t transparent · n next monitor · r reattach · p presenter · q quit");

        let mut last = Instant::now();
        let mut last_health = Instant::now();
        let mut last_retry = Instant::now();
        'main: loop {
            let next = last + FRAME;
            let wait = next.saturating_duration_since(Instant::now()).as_millis() as u32;
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
                match k {
                    'q' => break 'main,
                    'n' if !app.mons.is_empty() => {
                        app.cur = (app.cur + 1) % app.mons.len();
                        log!("current monitor -> {}", app.cur);
                    }
                    't' => app.toggle_transparent(),
                    'p' => app.switch_presenter(),
                    'r' => {
                        log!("forced reattach");
                        let t = Instant::now();
                        app.detach_all("forced reattach");
                        attached = app.attach_all("forced reattach");
                        log!("forced reattach took {} ms", t.elapsed().as_millis());
                    }
                    _ => {}
                }
            }
            if DISPLAY_HINT.swap(false, Ordering::SeqCst) {
                log!("display change broadcast");
                RESET.store(true, Ordering::SeqCst);
            }
            if TORN_DOWN.load(Ordering::SeqCst) && !RESET.load(Ordering::SeqCst) {
                log!("an underlay window was destroyed from outside");
                RESET.store(true, Ordering::SeqCst);
            }
            if RESET.swap(false, Ordering::SeqCst) {
                app.detach_all("reset");
                attached = false;
                reset_since = Some(Instant::now());
                last_retry = Instant::now() - Duration::from_secs(1);
            }
            if !attached && last_retry.elapsed() >= Duration::from_millis(500) {
                last_retry = Instant::now();
                attached = app.attach_all("recovery");
                if attached {
                    if let Some(t) = reset_since.take() {
                        log!("recovered {} ms after the reset", t.elapsed().as_millis());
                    }
                }
            }
            if attached && last_health.elapsed() >= Duration::from_secs(1) {
                last_health = Instant::now();
                app.health();
            }
            let now = Instant::now();
            let dt = (now - last).as_secs_f32().min(0.1);
            last = now;
            if attached {
                app.frame(dt);
            }
        }

        app.detach_all("quit");
        CLEANED.store(true, Ordering::SeqCst);
        if let Ok(c) = control {
            unsafe {
                let _ = DestroyWindow(c);
            }
        }
        log!("exit: {} attach(es), {} z-order fix(es). The wallpaper was never written.", app.attaches, app.z_fixes);
    }
}
