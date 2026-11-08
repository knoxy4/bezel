//! Spike S0: two bar windows, one DirectComposition device, one Commit() per frame (ADR-0001 §2.5, §9).
//!
//! A SHAFT-shaped and a TRENCH-shaped window meet at the bottom-left corner of the primary monitor's work area. Each
//! draws into an IDCompositionSurface, and both surfaces' updates land in one atomic Commit(), so a sprite crossing the
//! seam should never tear. A square runs down the shaft, across the seam into the trench, then along it.
//! The windows are plain topmost popups, not AppBars: the work area isn't touched.
//!
//! Args: --seconds N (default 20). Console key q quits early. Logs frame pacing to spike_s0.log.

#[cfg(windows)]
#[macro_use]
mod common;

#[cfg(not(windows))]
fn main() {
    eprintln!("spike_s0_corner is Windows-only");
}

#[cfg(windows)]
fn main() {
    imp::main();
}

#[cfg(windows)]
mod imp {
    use std::mem::ManuallyDrop;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use windows::core::{w, Interface, PCWSTR};
    use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F};
    use windows::Win32::Graphics::Direct2D::{
        D2D1CreateFactory, ID2D1DeviceContext, ID2D1Factory1, ID2D1Image, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
        D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    };
    use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0};
    use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION};
    use windows::Win32::Graphics::DirectComposition::{
        DCompositionCreateDevice, IDCompositionDevice, IDCompositionSurface, IDCompositionTarget, IDCompositionVisual,
    };
    use windows::Win32::Graphics::Dwm::DwmFlush;
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM};
    use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice, IDXGISurface};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    use crate::common::{self, fmt_rect, CLEANED, EXIT};

    const LOG_PATH: &str = "spike_s0.log";
    const TRENCH_H: i32 = 112;
    const SHAFT_W: i32 = 144;
    const SHAFT_H: i32 = 640;
    const TRENCH_W: i32 = 960;
    const SQ: f32 = 56.0;
    const SPEED: f32 = 700.0; // px/s: fast enough that a one-frame skew shows

    unsafe extern "system" fn bar_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_CLOSE => {
                EXIT.store(true, Ordering::SeqCst);
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
        }
    }

    struct Bar {
        name: &'static str,
        hwnd: HWND,
        rect: RECT,
        _target: IDCompositionTarget,
        _visual: IDCompositionVisual,
        surface: IDCompositionSurface,
    }

    fn rectf(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F {
        D2D_RECT_F { left: l, top: t, right: r, bottom: b }
    }

    /// Square position (top-left, screen px) at distance `d` along the L path: down the shaft and through the seam,
    /// then right along the trench, then wraps.
    fn sprite_at(d: f32, shaft: &RECT, trench: &RECT) -> (f32, f32) {
        let x0 = shaft.left as f32 + (SHAFT_W as f32 - SQ) / 2.0;
        let y_top = shaft.top as f32 + 20.0;
        let y_turn = trench.top as f32 + (TRENCH_H as f32 - SQ) / 2.0;
        let x_end = trench.right as f32 - SQ - 20.0;
        let leg1 = y_turn - y_top;
        let leg2 = x_end - x0;
        let d = d % (leg1 + leg2);
        if d < leg1 {
            (x0, y_top + d)
        } else {
            (x0 + (d - leg1), y_turn)
        }
    }

    unsafe fn draw_bar(dc: &ID2D1DeviceContext, bar: &Bar, sprite: (f32, f32), frame: u64) -> windows::core::Result<()> {
        let mut off = POINT::default();
        let surf: IDXGISurface = unsafe { bar.surface.BeginDraw(None, &mut off)? };
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: ManuallyDrop::new(None),
        };
        let r = (|| unsafe {
            let target = dc.CreateBitmapFromDxgiSurface(&surf, Some(&props))?;
            dc.SetTarget(&target);
            dc.BeginDraw();
            // The surface may be a region of a shared atlas: draw at its offset and never Clear the whole target.
            let (ox, oy) = (off.x as f32, off.y as f32);
            let rf = |l: f32, t: f32, r: f32, b: f32| rectf(l + ox, t + oy, r + ox, b + oy);
            let (w, h) = ((bar.rect.right - bar.rect.left) as f32, (bar.rect.bottom - bar.rect.top) as f32);
            let bg = if bar.name == "TRENCH" { D2D1_COLOR_F { r: 0.13, g: 0.12, b: 0.10, a: 1.0 } } else { D2D1_COLOR_F { r: 0.10, g: 0.10, b: 0.14, a: 1.0 } };
            let bgb = dc.CreateSolidColorBrush(&bg, None)?;
            dc.FillRectangle(&rf(0.0, 0.0, w, h), &bgb);
            // Stripes that slide with the frame count: a visible per-frame tick on each bar.
            let stripe = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.25, g: 0.22, b: 0.18, a: 1.0 }, None)?;
            let phase = (frame % 32) as f32;
            let mut x = -32.0 + phase;
            while x < w {
                dc.FillRectangle(&rf(x.max(0.0), 0.0, (x + 8.0).min(w), 6.0), &stripe);
                x += 32.0;
            }
            // The sprite, in this bar's local coordinates; the other bar draws its other half.
            let (sx, sy) = (sprite.0 - bar.rect.left as f32, sprite.1 - bar.rect.top as f32);
            if sx < w && sy < h && sx + SQ > 0.0 && sy + SQ > 0.0 {
                let orange = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 0.55, b: 0.1, a: 1.0 }, None)?;
                let white = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
                dc.FillRectangle(&rf(sx.max(0.0), sy.max(0.0), (sx + SQ).min(w), (sy + SQ).min(h)), &orange);
                // A vertical and a horizontal bar through the middle make a skew at the seam easy to see.
                dc.FillRectangle(&rf(sx + SQ / 2.0 - 3.0, sy.max(0.0), sx + SQ / 2.0 + 3.0, (sy + SQ).min(h)), &white);
                dc.FillRectangle(&rf(sx.max(0.0), sy + SQ / 2.0 - 3.0, (sx + SQ).min(w), sy + SQ / 2.0 + 3.0), &white);
            }
            dc.EndDraw(None, None)?;
            dc.SetTarget(None::<&ID2D1Image>);
            Ok(())
        })();
        unsafe { bar.surface.EndDraw()? };
        r
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        let secs = args.iter().position(|a| a == "--seconds").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<u64>().ok()).unwrap_or(20);
        common::log_init(LOG_PATH);
        log!("BEZEL spike S0 (corner commit), {secs} s.");
        common::log_os_version();
        common::set_per_monitor_v2();
        common::install_ctrl_handler();

        let Some(home) = common::monitors().into_iter().find(|m| m.primary) else {
            log!("no primary monitor");
            return;
        };
        let wa = home.work;
        let trench = RECT { left: wa.left, top: wa.bottom - TRENCH_H, right: wa.left + TRENCH_W, bottom: wa.bottom };
        let shaft = RECT { left: wa.left, top: trench.top - SHAFT_H, right: wa.left + SHAFT_W, bottom: trench.top };
        log!("primary work area {}; TRENCH {} SHAFT {} (seam at y={})", fmt_rect(&wa), fmt_rect(&trench), fmt_rect(&shaft), trench.top);

        let r = (|| unsafe {
            let mut d3d: Option<ID3D11Device> = None;
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0]),
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )?;
            let dxgi: IDXGIDevice = d3d.expect("no d3d device").cast()?;
            let d2f: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dc = d2f.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            dc.SetDpi(96.0, 96.0);
            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;

            let hinst: windows::Win32::Foundation::HINSTANCE = GetModuleHandleW(PCWSTR::null())?.into();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(bar_proc),
                hInstance: hinst,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: w!("BezelS0Bar"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let mut bars = Vec::new();
            for (name, rc) in [("TRENCH", trench), ("SHAFT", shaft)] {
                let hwnd = CreateWindowExW(
                    WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    w!("BezelS0Bar"),
                    w!("BEZEL S0"),
                    WS_POPUP | WS_VISIBLE,
                    rc.left,
                    rc.top,
                    rc.right - rc.left,
                    rc.bottom - rc.top,
                    None,
                    None,
                    Some(hinst),
                    None,
                )?;
                let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
                let visual = dcomp.CreateVisual()?;
                let surface = dcomp.CreateSurface((rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_PREMULTIPLIED)?;
                visual.SetContent(&surface)?;
                target.SetRoot(&visual)?;
                bars.push(Bar { name, hwnd, rect: rc, _target: target, _visual: visual, surface });
            }
            dcomp.Commit()?;
            Ok::<_, windows::core::Error>((dc, dcomp, bars))
        })();
        let (dc, dcomp, bars) = match r {
            Ok(x) => x,
            Err(e) => {
                log!("setup FAILED: {e}");
                return;
            }
        };
        log!("two DComp targets on one device: ok");

        let keys = common::console_keys();
        let start = Instant::now();
        let mut last = Instant::now();
        let mut sec = Instant::now();
        let (mut frames, mut sum_ms, mut max_ms, mut max_commit_ms, mut slow) = (0u64, 0f64, 0f64, 0f64, 0u64);
        let (mut total_frames, mut total_slow, mut total_draw_err) = (0u64, 0u64, 0u64);
        let mut intervals: Vec<f64> = Vec::new();
        let mut frame = 0u64;
        'main: loop {
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'main;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if EXIT.load(Ordering::SeqCst) || keys.try_iter().any(|k| k == 'q') || start.elapsed() >= Duration::from_secs(secs) {
                break;
            }
            let t = start.elapsed().as_secs_f32();
            let sprite = sprite_at(t * SPEED, &shaft, &trench);
            for b in &bars {
                if let Err(e) = unsafe { draw_bar(&dc, b, sprite, frame) } {
                    total_draw_err += 1;
                    if total_draw_err <= 5 {
                        log!("{} draw error: {e}", b.name);
                    }
                }
            }
            let c0 = Instant::now();
            if let Err(e) = unsafe { dcomp.Commit() } {
                log!("Commit failed: {e}");
            }
            max_commit_ms = max_commit_ms.max(c0.elapsed().as_secs_f64() * 1000.0);
            // Pace to the compositor: one frame per DWM present.
            let _ = unsafe { DwmFlush() };
            frame += 1;
            let now = Instant::now();
            let dt = (now - last).as_secs_f64() * 1000.0;
            last = now;
            frames += 1;
            sum_ms += dt;
            max_ms = max_ms.max(dt);
            intervals.push(dt);
            if sec.elapsed() >= Duration::from_secs(1) {
                let avg = sum_ms / frames as f64;
                // "Slow" = a frame interval above 1.5x this second's median, i.e. a missed compositor frame.
                intervals.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let med = intervals[intervals.len() / 2];
                slow = intervals.iter().filter(|&&x| x > med * 1.5).count() as u64;
                log!("1s: {frames} frames ({:.1} fps), interval avg {avg:.2} ms median {med:.2} max {max_ms:.2}, missed {slow}, max Commit {max_commit_ms:.2} ms", 1000.0 / avg);
                total_frames += frames;
                total_slow += slow;
                frames = 0;
                sum_ms = 0.0;
                max_ms = 0.0;
                max_commit_ms = 0.0;
                intervals.clear();
                sec = Instant::now();
            }
        }
        let _ = slow;
        for b in &bars {
            unsafe {
                let _ = DestroyWindow(b.hwnd);
            }
        }
        drop(bars);
        CLEANED.store(true, Ordering::SeqCst);
        log!("exit: {total_frames} frames, {total_slow} missed, {total_draw_err} draw errors");
    }
}
