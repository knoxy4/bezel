//! Phase 2 gate demo: TRENCH + SHAFT docked as AppBars on the home monitor (os-win32 ShellDock, from S3) and drawn by
//! the bar presenter (one Commit for both, from S0), with the underlay on every monitor (from S9). A cart drops out of
//! the shaft and runs along the trench, crossing the corner; a drill bores up the shaft.
//! Runs `--seconds N` (default 20) or until Ctrl+C, then undocks and detaches on every exit path.
//! No `windows` dependency: everything Win32 goes through os-win32 / render-d2d (ADR §7).

#[cfg(not(windows))]
fn main() {
    eprintln!("bedrock_demo is Windows-only");
}

#[cfg(windows)]
fn main() {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use bezel_core::{Edge, MonitorInfo, OsEvent, RectPx, SurfaceId};
    use bezel_layout::art_scale_for;
    use bezel_os_api::{DesktopLayer, DockMode, LayerHealth, ShellDock, UnderlayMode, WallpaperFit};
    use bezel_os_win32::{session, Win32DesktopLayer, Win32ShellDock};
    use bezel_render_api::{DrawCmd, DrawList, Renderer, Rgba8, SurfaceKind, WallpaperFitR};
    use bezel_render_d2d::D2dRenderer;

    const TRENCH_PX: i32 = 112; // D17, at 100% scaling
    const SHAFT_PX: i32 = 144;

    let secs = std::env::args().skip_while(|a| a != "--seconds").nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(20);
    session::set_per_monitor_v2();
    session::install_console_exit();
    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC: {info}");
        let b = bezel_os_win32::emergency_undock();
        let u = bezel_os_win32::emergency_detach();
        eprintln!("emergency: {b} bar(s) undocked, {u} underlay(s) detached");
    }));

    let mut layer = match Win32DesktopLayer::new() {
        Ok(l) => l,
        Err(e) => return eprintln!("desktop layer: {e}"),
    };
    let mut dock = match Win32ShellDock::new() {
        Ok(d) => d,
        Err(e) => return eprintln!("dock: {e}"),
    };
    let mut gfx = match D2dRenderer::new() {
        Ok(r) => r,
        Err(e) => return eprintln!("renderer: {e}"),
    };

    let mons = session::monitors();
    let Some(home) = mons.iter().find(|m| m.primary).cloned() else { return eprintln!("no primary monitor") };
    let scale = art_scale_for(home.dpi) as i32;
    println!("home monitor {} {}x{} @ {} dpi, taskbar on {:?}", home.id.0, home.bounds.width(), home.bounds.height(), home.dpi, dock.taskbar_edge(home.id));

    // ── underlays ──
    let fit_r = |f: WallpaperFit, virt: RectPx| match f {
        WallpaperFit::Fill => WallpaperFitR::Fill,
        WallpaperFit::Fit => WallpaperFitR::Fit,
        WallpaperFit::Stretch => WallpaperFitR::Stretch,
        WallpaperFit::Tile => WallpaperFitR::Tile,
        WallpaperFit::Center => WallpaperFitR::Center,
        WallpaperFit::Span => WallpaperFitR::Span { virtual_screen: virt },
    };
    let attach_underlays = |layer: &mut Win32DesktopLayer, gfx: &mut D2dRenderer, mons: &[MonitorInfo]| {
        let virt = mons.iter().skip(1).fold(mons[0].bounds, |a, m| a.union(&m.bounds));
        for m in mons {
            let Ok(h) = layer.attach(m.id, m.bounds) else { continue };
            if gfx.attach(SurfaceId::Underlay(m.id), SurfaceKind::Underlay, h, m.bounds, art_scale_for(m.dpi)).is_err() {
                continue;
            }
            if let Some(wp) = layer.wallpaper(m.id) {
                let _ = gfx.set_wallpaper(m.id, wp.path.as_deref(), fit_r(wp.fit, virt), Rgba8(wp.background));
            }
            if layer.foreign_occupant(m.id) && gfx.set_underlay_transparent(m.id, true).is_ok() {
                let _ = layer.set_mode(m.id, UnderlayMode::TransparentOverForeign);
            }
        }
    };
    attach_underlays(&mut layer, &mut gfx, &mons);

    // ── bars: TRENCH first so the corner is its (§2.5) ──
    let mut rects: HashMap<SurfaceId, RectPx> = HashMap::new();
    for (s, edge, px) in [(SurfaceId::Trench, Edge::Bottom, TRENCH_PX), (SurfaceId::Shaft, Edge::Left, SHAFT_PX)] {
        match dock.dock(s, home.id, edge, px * scale, DockMode::Reserve) {
            Ok(r) => {
                println!("{s:?} docked at ({},{})-({},{})", r.l, r.t, r.r, r.b);
                rects.insert(s, r);
                if let Some(h) = dock.surface_handle(s) {
                    if let Err(e) = gfx.attach(s, SurfaceKind::BarComposited, h, r, scale as u8) {
                        eprintln!("{s:?}: renderer attach failed: {e}");
                    }
                }
            }
            Err(e) => eprintln!("{s:?}: dock failed: {e}"),
        }
    }
    let work_after = session::monitors().into_iter().find(|m| m.id == home.id).map(|m| m.work);
    if let Some(w) = work_after {
        println!("home work area now ({},{})-({},{})", w.l, w.t, w.r, w.b);
    }

    let start = Instant::now();
    let mut last_health = Instant::now();
    let mut stood_down = false;
    let (mut frames, mut resets, mut redocks) = (0u64, 0u32, 0u32);
    while start.elapsed() < Duration::from_secs(secs) && !session::exit_requested() {
        if !session::pump(33) {
            break; // ~30 fps: the Active tier (D10); the real pacer comes with the app loop
        }
        for ev in dock.service() {
            match ev {
                OsEvent::Docked { surface, rect } => {
                    redocks += 1;
                    println!("re-docked {surface:?} at ({},{})-({},{})", rect.l, rect.t, rect.r, rect.b);
                    gfx.resize(surface, (rect.width() as u32, rect.height() as u32));
                    rects.insert(surface, rect);
                }
                OsEvent::StandDown { on, .. } => {
                    stood_down = on;
                    println!("fullscreen stand-down: {on}");
                }
                _ => {}
            }
        }
        if last_health.elapsed() >= Duration::from_secs(1) {
            last_health = Instant::now();
            if layer.health() == LayerHealth::ResetNeeded {
                resets += 1;
                println!("underlay reset #{resets}: reattaching");
                for m in &mons {
                    gfx.detach(SurfaceId::Underlay(m.id));
                }
                layer.detach_all();
                attach_underlays(&mut layer, &mut gfx, &session::monitors());
            }
        }
        if stood_down {
            continue;
        }
        let t = start.elapsed().as_secs_f32();
        let mut frame: Vec<DrawList> = Vec::new();
        let (Some(tr), Some(sh)) = (rects.get(&SurfaceId::Trench).copied(), rects.get(&SurfaceId::Shaft).copied()) else { continue };
        let s = scale as f32;

        // The cart's path in screen px: down the shaft, through the corner, along the trench.
        let cart = 28.0 * s;
        let down = (tr.t - sh.t) as f32 + (tr.height() as f32 - cart) / 2.0;
        let along = (tr.r - tr.l) as f32 - cart - sh.width() as f32;
        let d = (t * 520.0 * s) % (down + along);
        let (cx, cy) = if d < down {
            (sh.l as f32 + (sh.width() as f32 - cart) / 2.0, sh.t as f32 + d)
        } else {
            (sh.l as f32 + (sh.width() as f32 - cart) / 2.0 + (d - down), tr.t as f32 + (tr.height() as f32 - cart) / 2.0)
        };
        let cart_in = |r: RectPx, cmds: &mut Vec<DrawCmd>| {
            let (x, y) = (cx - r.l as f32, cy - r.t as f32);
            cmds.push(DrawCmd::Fill { rect: [x, y, x + cart, y + cart], color: Rgba8([255, 150, 40, 255]) });
            cmds.push(DrawCmd::Fill { rect: [x + cart * 0.42, y, x + cart * 0.58, y + cart], color: Rgba8([255, 255, 255, 255]) });
        };

        // TRENCH: brick, a foundry glow that breathes, the label.
        let (tw, th) = (tr.width() as f32, tr.height() as f32);
        let mut tc = vec![DrawCmd::Fill { rect: [0.0, 0.0, tw, th], color: Rgba8([42, 30, 26, 255]) }];
        let glow = (110.0 + 60.0 * (t * 2.2).sin()) as u8;
        tc.push(DrawCmd::Fill { rect: [0.0, 0.0, tw, 6.0 * s], color: Rgba8([255, 120, 30, glow]) });
        let mut x = 0.0;
        while x < tw {
            tc.push(DrawCmd::Fill { rect: [x, th - 18.0 * s, x + 30.0 * s, th - 16.0 * s], color: Rgba8([70, 48, 40, 255]) });
            x += 36.0 * s;
        }
        tc.push(DrawCmd::Text { s: "TRENCH · foundry · BEZEL phase 2 bedrock demo".into(), x: sh.width() as f32 + 24.0 * s, y: 40.0 * s, size: 20.0 * s, color: Rgba8([255, 220, 180, 255]) });
        cart_in(tr, &mut tc);
        frame.push(DrawList { surface: SurfaceId::Trench, dirty: Vec::new(), cmds: tc });

        // SHAFT: strata bands scrolling down (the drill bores up), the drill, the label.
        let (sw, shh) = (sh.width() as f32, sh.height() as f32);
        let mut sc = vec![DrawCmd::Fill { rect: [0.0, 0.0, sw, shh], color: Rgba8([24, 22, 34, 255]) }];
        let band = 90.0 * s;
        let scroll = (t * 30.0 * s) % band;
        let mut y = -band + scroll;
        let mut k = 0;
        while y < shh {
            let c = if k % 2 == 0 { Rgba8([34, 30, 48, 255]) } else { Rgba8([28, 26, 40, 255]) };
            sc.push(DrawCmd::Fill { rect: [0.0, y, sw, y + band], color: c });
            y += band;
            k += 1;
        }
        let dy = 60.0 * s;
        sc.push(DrawCmd::Polyline { pts: vec![[sw * 0.3, dy + 40.0 * s], [sw * 0.5, dy], [sw * 0.7, dy + 40.0 * s]], width: 4.0 * s, color: Rgba8([180, 200, 255, 255]), glow: true });
        sc.push(DrawCmd::Text { s: "SHAFT".into(), x: 16.0 * s, y: shh - 60.0 * s, size: 22.0 * s, color: Rgba8([200, 200, 255, 255]) });
        cart_in(sh, &mut sc);
        frame.push(DrawList { surface: SurfaceId::Shaft, dirty: Vec::new(), cmds: sc });

        // Underlays: a faint label so the frontier layer is visibly alive.
        for m in &mons {
            frame.push(DrawList {
                surface: SurfaceId::Underlay(m.id),
                dirty: Vec::new(),
                cmds: vec![DrawCmd::Text { s: format!("frontier · monitor {}", m.id.0), x: 48.0, y: 48.0, size: 20.0, color: Rgba8([160, 230, 220, 200]) }],
            });
        }
        if let Err(e) = gfx.submit(&frame) {
            eprintln!("submit: {e}");
        }
        frames += 1;
    }

    gfx.detach(SurfaceId::Trench);
    gfx.detach(SurfaceId::Shaft);
    dock.undock_all();
    for m in &mons {
        gfx.detach(SurfaceId::Underlay(m.id));
    }
    layer.detach_all();
    session::mark_cleaned();
    let work_end = session::monitors().into_iter().find(|m| m.id == home.id).map(|m| m.work);
    println!(
        "done: {frames} frames, {redocks} re-dock(s), {resets} underlay reset(s). Home work area back to {:?} (was {:?}). Wallpaper never written.",
        work_end.map(|w| (w.l, w.t, w.r, w.b)),
        (home.work.l, home.work.t, home.work.r, home.work.b)
    );
}
