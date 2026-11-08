//! Phase 2 demo: the promoted underlay (os-win32 DesktopLayer + render-d2d) with a physics rope swinging on every
//! monitor. Opaque over a static wallpaper, transparent over a live one (D38). Reattaches when the layer reports a
//! reset. Runs `--seconds N` (default 15) or until Ctrl+C.
//! No `windows` dependency here: everything Win32 goes through os-win32 / render-d2d (ADR §7).

#[cfg(not(windows))]
fn main() {
    eprintln!("underlay_demo is Windows-only");
}

#[cfg(windows)]
fn main() {
    use std::time::{Duration, Instant};

    use bezel_core::{MonitorInfo, RectPx, SurfaceId};
    use bezel_layout::art_scale_for;
    use bezel_os_api::{DesktopLayer, LayerHealth, UnderlayMode, WallpaperFit};
    use bezel_os_win32::{session, Win32DesktopLayer};
    use bezel_physics::RopeWorld;
    use bezel_render_api::{DrawCmd, DrawList, Renderer, Rgba8, SurfaceKind, WallpaperFitR};
    use bezel_render_d2d::D2dRenderer;

    let secs = std::env::args()
        .skip_while(|a| a != "--seconds")
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(15);
    session::set_per_monitor_v2();
    session::install_console_exit();
    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC: {info}");
        let n = bezel_os_win32::emergency_detach();
        eprintln!("emergency detach: {n} underlay(s)");
    }));

    let mut layer = match Win32DesktopLayer::new() {
        Ok(l) => l,
        Err(e) => return eprintln!("desktop layer: {e}"),
    };
    let mut gfx = match D2dRenderer::new() {
        Ok(r) => r,
        Err(e) => return eprintln!("renderer: {e}"),
    };

    fn fit_r(f: WallpaperFit, virt: RectPx) -> WallpaperFitR {
        match f {
            WallpaperFit::Fill => WallpaperFitR::Fill,
            WallpaperFit::Fit => WallpaperFitR::Fit,
            WallpaperFit::Stretch => WallpaperFitR::Stretch,
            WallpaperFit::Tile => WallpaperFitR::Tile,
            WallpaperFit::Center => WallpaperFitR::Center,
            WallpaperFit::Span => WallpaperFitR::Span { virtual_screen: virt },
        }
    }

    let attach_all = |layer: &mut Win32DesktopLayer, gfx: &mut D2dRenderer, mons: &[MonitorInfo]| -> usize {
        let virt = mons.iter().skip(1).fold(mons[0].bounds, |a, m| a.union(&m.bounds));
        let mut n = 0;
        for m in mons {
            let h = match layer.attach(m.id, m.bounds) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("monitor {}: attach failed: {e}", m.id.0);
                    continue;
                }
            };
            if let Err(e) = gfx.attach(SurfaceId::Underlay(m.id), SurfaceKind::Underlay, h, m.bounds, art_scale_for(m.dpi)) {
                eprintln!("monitor {}: renderer attach failed: {e}", m.id.0);
                continue;
            }
            let foreign = layer.foreign_occupant(m.id);
            if let Some(wp) = layer.wallpaper(m.id) {
                let bg = Rgba8(wp.background);
                let _ = gfx.set_wallpaper(m.id, wp.path.as_deref(), fit_r(wp.fit, virt), bg);
            }
            if foreign {
                match gfx.set_underlay_transparent(m.id, true) {
                    Ok(()) => {
                        let _ = layer.set_mode(m.id, UnderlayMode::TransparentOverForeign);
                    }
                    Err(e) => eprintln!("monitor {}: {e}", m.id.0),
                }
            }
            println!(
                "monitor {}: {:?} {}x{} | presenter {} | {}",
                m.id.0,
                layer.topology(),
                m.bounds.width(),
                m.bounds.height(),
                gfx.presenter_name(m.id).unwrap_or("-"),
                if gfx.is_transparent(m.id) { "transparent over live wallpaper" } else { "opaque" }
            );
            n += 1;
        }
        n
    };

    let mut mons = session::monitors();
    if mons.is_empty() {
        return eprintln!("no monitors");
    }
    let mut attached = attach_all(&mut layer, &mut gfx, &mons) > 0;
    let mut ropes = RopeWorld::new();
    let mut rope_ids: Vec<_> = mons
        .iter()
        .map(|m| {
            let ax = m.bounds.width() as f32 * 0.5;
            ropes.spawn([ax, 160.0], [ax + 420.0, 160.0], 24)
        })
        .collect();

    let start = Instant::now();
    let mut last = Instant::now();
    let mut last_health = Instant::now();
    let mut resets = 0u32;
    while start.elapsed() < Duration::from_secs(secs) && !session::exit_requested() {
        if !session::pump(16) {
            break;
        }
        let dt = last.elapsed().as_secs_f32().min(0.1);
        last = Instant::now();
        if last_health.elapsed() >= Duration::from_secs(1) {
            last_health = Instant::now();
            match layer.health() {
                LayerHealth::Ok => {}
                LayerHealth::ZOrderFixed => println!("z-order drift fixed"),
                LayerHealth::ResetNeeded => {
                    resets += 1;
                    println!("layer reset #{resets}: reattaching");
                    for m in &mons {
                        gfx.detach(SurfaceId::Underlay(m.id));
                    }
                    layer.detach_all();
                    mons = session::monitors();
                    attached = attach_all(&mut layer, &mut gfx, &mons) > 0;
                    rope_ids.truncate(mons.len());
                }
            }
        }
        if !attached {
            continue;
        }
        ropes.step(dt);
        let t = start.elapsed().as_secs_f32();
        let frame: Vec<DrawList> = mons
            .iter()
            .zip(&rope_ids)
            .map(|(m, r)| {
                let (w, h) = (m.bounds.width() as f32, m.bounds.height() as f32);
                let mut cmds = Vec::new();
                // A faint 64 px grid so the surface is visible over any wallpaper.
                let grid = Rgba8([90, 200, 190, 70]);
                let mut x = 0.0;
                while x < w {
                    cmds.push(DrawCmd::Polyline { pts: vec![[x, 0.0], [x, h]], width: 1.0, color: grid, glow: false });
                    x += 64.0;
                }
                let mut y = 0.0;
                while y < h {
                    cmds.push(DrawCmd::Polyline { pts: vec![[0.0, y], [w, y]], width: 1.0, color: grid, glow: false });
                    y += 64.0;
                }
                let nodes = ropes.nodes(*r).to_vec();
                cmds.push(DrawCmd::Polyline { pts: nodes.clone(), width: 3.0, color: Rgba8([255, 170, 60, 255]), glow: true });
                if let Some(tip) = nodes.last() {
                    let s = 18.0 + 4.0 * (t * 6.0).sin();
                    cmds.push(DrawCmd::Fill { rect: [tip[0] - s, tip[1] - s, tip[0] + s, tip[1] + s], color: Rgba8([255, 120, 30, 255]) });
                }
                cmds.push(DrawCmd::Text {
                    s: format!(
                        "BEZEL phase 2 · underlay demo · monitor {} · {:?} · {} · {}",
                        m.id.0,
                        layer.topology(),
                        gfx.presenter_name(m.id).unwrap_or("-"),
                        if gfx.is_transparent(m.id) { "transparent" } else { "opaque" }
                    ),
                    x: 48.0,
                    y: 48.0,
                    size: 22.0,
                    color: Rgba8([255, 255, 255, 230]),
                });
                DrawList { surface: SurfaceId::Underlay(m.id), dirty: Vec::new(), cmds }
            })
            .collect();
        if let Err(e) = gfx.submit(&frame) {
            eprintln!("submit: {e}");
        }
    }

    for m in &mons {
        gfx.detach(SurfaceId::Underlay(m.id));
    }
    layer.detach_all();
    session::mark_cleaned();
    println!("detached; {resets} reset(s). The wallpaper was never written.");
}
