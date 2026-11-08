//! BEZEL composition root (ADR-0001 §6). Phase 3: the first full loop on the real desktop.
//!
//! Main thread: bars (AppBars), underlays, message pump, occlusion grid, sim, render. Worker threads: window tracking
//! (shell-watch) and input pulses, both over channels. Every exit path undocks the bars and detaches the underlays.
//! `--seconds N` exits after N seconds (default: run until Ctrl+C or console close).

#[cfg(not(windows))]
fn main() {
    say!("bezel-app needs Windows 11");
}

#[cfg(windows)]
mod extract;

/// stdout + an appended `bezel.log` in the working directory (rects, ids and counts only).
#[cfg(windows)]
mod log {
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};
    static FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
    pub fn line(s: &str) {
        println!("{s}");
        let f = FILE.get_or_init(|| Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open("bezel.log").expect("open bezel.log")));
        if let Ok(mut f) = f.lock() {
            let _ = writeln!(f, "{s}");
            let _ = f.flush();
        }
    }
}

#[cfg(windows)]
macro_rules! say {
    ($($t:tt)*) => { crate::log::line(&format!($($t)*)) };
}

/// Percent of each monitor's cells that are bare.
#[cfg(windows)]
fn bare_report(grid: &bezel_layout::OcclusionGrid, mons: &[bezel_core::MonitorInfo]) -> String {
    mons.iter()
        .map(|m| {
            let (cols, rows) = grid.dims(m.id).unwrap_or((0, 0));
            let total = cols as usize * rows as usize;
            let mut bare = 0usize;
            for cy in 0..rows {
                for cx in 0..cols {
                    if grid.is_bare(bezel_core::CellId { monitor: m.id, cx, cy }) {
                        bare += 1;
                    }
                }
            }
            format!("m{} {}% bare", m.id.0, if total > 0 { bare * 100 / total } else { 0 })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Read-only: no bars, no underlays, no GPU. Tests every counted window as a would-be rig target.
#[cfg(windows)]
fn probe() {
    use bezel_core::{PulseBatch, TargetWindow};
    use bezel_layout::{art_scale_for, OcclusionGrid};
    use bezel_os_api::WindowTracker;
    use bezel_os_win32::{session, Win32WindowTracker};
    use bezel_sim::{FrameInputs, Sim, SimConfig};
    session::set_per_monitor_v2();
    let mons = session::monitors();
    let scale = art_scale_for(mons.iter().find(|m| m.primary).map(|m| m.dpi).unwrap_or(96));
    let tracker = Win32WindowTracker::new();
    let mut grid = OcclusionGrid::new(&mons, 16, scale);
    let mut all = Vec::new();
    for m in &mons {
        let snap = tracker.snapshot(m.id);
        grid.rebuild(m.id, &snap);
        for w in snap {
            if !all.iter().any(|(id, _)| *id == w.0) {
                all.push(w);
            }
        }
    }
    say!("probe: {} windows count; {}", all.len(), bare_report(&grid, &mons));
    let bedrock: Vec<_> = mons.iter().map(|m| (m.bounds, m.work.b)).collect();
    for (id, r) in &all {
        let (cx, cy) = ((r.l + r.r) / 2, (r.t + r.b) / 2);
        let Some(m) = mons.iter().find(|m| m.bounds.contains(cx, cy)) else { continue };
        if r.height() <= 64 && r.width() >= m.bounds.width() - 2 {
            say!("  window {:x} on m{}: taskbar-shaped, never a target", id.0, m.id.0);
            continue;
        }
        let maximized = r.width() >= m.work.width() - 16 && r.height() >= m.work.height() - 16;
        let t = TargetWindow { id: *id, frame: *r, monitor: m.id, maximized };
        let mut sim = Sim::new(SimConfig { art_scale: scale as f32, ..SimConfig::default() }, 1);
        sim.step(&FrameInputs { pulses: PulseBatch::default(), cells: &[], swept: &[], moved: &[], target: Some(t), grid: &grid, bedrock: &bedrock });
        say!(
            "  window {:x} on m{} ({},{})-({},{}){}: {}",
            id.0,
            m.id.0,
            r.l,
            r.t,
            r.r,
            r.b,
            if maximized { " maximized" } else { "" },
            match sim.build_block() {
                None => "RIG WOULD BUILD".to_string(),
                Some(b) => format!("no rig ({b:?})"),
            }
        );
    }
}

#[cfg(windows)]
fn main() {
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use bezel_core::{CellId, Edge, ExtWindowId, MonitorInfo, OsEvent, Px, PulseBatch, RectCause, RectPx, SurfaceId, TargetWindow};
    use bezel_layout::{art_scale_for, CellDelta, OcclusionGrid};
    use bezel_os_api::{DesktopLayer, DockMode, InputPulseSource, LayerHealth, ShellDock, UnderlayMode, WallpaperFit, WindowTracker};
    use bezel_os_win32::{session, Win32DesktopLayer, Win32Pulses, Win32ShellDock, Win32WindowTracker};
    use bezel_render_api::{Renderer, Rgba8, SurfaceKind, WallpaperFitR};
    use bezel_render_d2d::D2dRenderer;
    use bezel_sim::{AnimDemand, FrameInputs, Sim, SimConfig, SimEvent, DT};

    const TRENCH_PX: i32 = 112; // D17
    const SHAFT_PX: i32 = 144;
    const CELL_ART_PX: u16 = 16;
    const SEED: u64 = 0xBE2E_1000;

    if std::env::args().any(|a| a == "--probe") {
        return probe();
    }
    let secs = std::env::args().skip_while(|a| a != "--seconds").nth(1).and_then(|s| s.parse::<u64>().ok());
    session::set_per_monitor_v2();
    session::install_console_exit();
    std::panic::set_hook(Box::new(|info| {
        say!("PANIC: {info}");
        let b = bezel_os_win32::emergency_undock();
        let u = bezel_os_win32::emergency_detach();
        say!("emergency: {b} bar(s) undocked, {u} underlay(s) detached");
    }));

    let mut layer = match Win32DesktopLayer::new() {
        Ok(l) => l,
        Err(e) => return say!("desktop layer: {e}"),
    };
    let mut dock = match Win32ShellDock::new() {
        Ok(d) => d,
        Err(e) => return say!("dock: {e}"),
    };
    let mut gfx = match D2dRenderer::new() {
        Ok(r) => r,
        Err(e) => return say!("renderer: {e}"),
    };

    let mut mons = session::monitors();
    let Some(home) = mons.iter().find(|m| m.primary).cloned() else { return say!("no primary monitor") };
    let scale = art_scale_for(home.dpi);

    // ── underlays (S9 path) ──
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

    // ── bars (S3 + S0 paths), TRENCH first ──
    let mut bars: HashMap<SurfaceId, RectPx> = HashMap::new();
    for (s, edge, px) in [(SurfaceId::Trench, Edge::Bottom, TRENCH_PX), (SurfaceId::Shaft, Edge::Left, SHAFT_PX)] {
        match dock.dock(s, home.id, edge, px * scale as i32, DockMode::Reserve) {
            Ok(r) => {
                bars.insert(s, r);
                if let Some(h) = dock.surface_handle(s) {
                    if let Err(e) = gfx.attach(s, SurfaceKind::BarComposited, h, r, scale) {
                        say!("{s:?}: renderer attach failed: {e}");
                    }
                }
            }
            Err(e) => say!("{s:?}: dock failed: {e}"),
        }
    }
    mons = session::monitors(); // work areas changed when the bars docked
    let bedrock_of = |mons: &[MonitorInfo], bars: &HashMap<SurfaceId, RectPx>| -> Vec<(RectPx, Px)> {
        mons.iter()
            .map(|m| {
                let floor = if m.id == home.id { bars.get(&SurfaceId::Trench).map(|t| t.t).unwrap_or(m.work.b) } else { m.work.b };
                (m.bounds, floor)
            })
            .collect()
    };
    let mut bedrock = bedrock_of(&mons, &bars);

    // ── window tracking + pulses ──
    let (os_tx, os_rx) = mpsc::channel::<OsEvent>();
    let (pulse_tx, pulse_rx) = mpsc::channel::<PulseBatch>();
    let mut tracker = Win32WindowTracker::new();
    if let Err(e) = tracker.start(os_tx) {
        say!("window tracker: {e}");
    }
    let mut pulses = Win32Pulses::new();
    match pulses.start(pulse_tx) {
        Ok(()) => say!("pulse counting on: activity counts only, never which key (D5; first-run card comes with the demo)"),
        Err(e) => say!("pulses: {e}"),
    }

    let mut grid = OcclusionGrid::new(&mons, CELL_ART_PX, scale);
    let mut last_rect: HashMap<ExtWindowId, RectPx> = HashMap::new();
    for m in &mons {
        let snap = tracker.snapshot(m.id);
        for (id, r) in &snap {
            last_rect.insert(*id, *r);
        }
        grid.rebuild(m.id, &snap);
    }
    let mut target: Option<TargetWindow> = tracker.foreground();
    let mut sim = Sim::new(SimConfig { art_scale: scale as f32, ..SimConfig::default() }, SEED);
    say!(
        "BEZEL phase 3 loop: home monitor {} ({}x{}), {} windows tracked, TRENCH {:?}, SHAFT {:?}",
        home.id.0,
        home.bounds.width(),
        home.bounds.height(),
        grid.tracked(),
        bars.get(&SurfaceId::Trench),
        bars.get(&SurfaceId::Shaft)
    );

    let start = Instant::now();
    let mut last = Instant::now();
    let mut acc = 0.0f32;
    let mut last_health = Instant::now();
    let mut next_frame = Instant::now();
    let mut stood_down = false;
    let mut cells: Vec<CellDelta> = Vec::new();
    let mut swept: Vec<CellId> = Vec::new();
    let mut moved: Vec<(ExtWindowId, RectCause)> = Vec::new();
    let mut pulse_acc = PulseBatch::default();
    let (mut frames, mut crushes, mut rigs_built) = (0u64, 0u32, 0u32);
    let mut last_report = Instant::now() - Duration::from_secs(60);
    let mut last_why: (Option<(ExtWindowId, RectPx, bool)>, Option<bezel_sim::BuildBlock>) = (None, None);

    loop {
        if secs.is_some_and(|s| start.elapsed() >= Duration::from_secs(s)) || session::exit_requested() {
            break;
        }
        let wait = next_frame.saturating_duration_since(Instant::now()).as_millis().min(50) as u32;
        if !session::pump(wait) {
            break;
        }

        // Window hints → latest per window this frame (a drag start/end isn't hidden by a later location event).
        let mut latest: Vec<(ExtWindowId, Option<RectPx>, RectCause)> = Vec::new();
        while let Ok(ev) = os_rx.try_recv() {
            match ev {
                OsEvent::WindowRect { id, rect, cause } => match latest.iter_mut().find(|(i, _, _)| *i == id) {
                    Some(e) if !matches!(e.2, RectCause::DragStart | RectCause::DragEnd) => *e = (id, rect, cause),
                    Some(e) => e.1 = rect,
                    None => latest.push((id, rect, cause)),
                },
                OsEvent::Foreground(t) => target = t,
                _ => {}
            }
        }
        for (id, rect, cause) in latest {
            let prev = last_rect.get(&id).copied();
            if prev == rect {
                continue;
            }
            if matches!(cause, RectCause::DragSample | RectCause::DragEnd) {
                if let (Some(a), Some(b)) = (prev, rect) {
                    // Every cell the window crossed, minus the ones it was already covering (S10 finding).
                    let already = grid.cells_in(a);
                    swept.extend(grid.sweep(a, b).into_iter().filter(|c| !already.contains(c)));
                }
            }
            cells.push(grid.apply(id, rect, cause));
            if prev.is_some() {
                moved.push((id, cause));
            }
            match rect {
                Some(r) => {
                    last_rect.insert(id, r);
                }
                None => {
                    last_rect.remove(&id);
                }
            }
            if let Some(t) = target.as_mut() {
                if t.id == id {
                    match rect {
                        Some(r) => t.frame = r,
                        None => target = None,
                    }
                }
            }
        }
        while let Ok(p) = pulse_rx.try_recv() {
            pulse_acc.key_events = pulse_acc.key_events.saturating_add(p.key_events);
            pulse_acc.mouse_clicks = pulse_acc.mouse_clicks.saturating_add(p.mouse_clicks);
            pulse_acc.wheel_ticks = pulse_acc.wheel_ticks.saturating_add(p.wheel_ticks);
            pulse_acc.mouse_travel_px = pulse_acc.mouse_travel_px.saturating_add(p.mouse_travel_px);
        }

        for ev in dock.service() {
            match ev {
                OsEvent::Docked { surface, rect } => {
                    gfx.resize(surface, (rect.width() as u32, rect.height() as u32));
                    bars.insert(surface, rect);
                    mons = session::monitors();
                    bedrock = bedrock_of(&mons, &bars);
                }
                OsEvent::StandDown { on, .. } => stood_down = on, // invariant 3; input counting pauses too (D7)
                _ => {}
            }
        }
        pulses.set_paused(stood_down);

        if last_health.elapsed() >= Duration::from_secs(1) {
            last_health = Instant::now();
            if layer.health() == LayerHealth::ResetNeeded {
                say!("desktop layer reset: reattaching underlays");
                for m in &mons {
                    gfx.detach(SurfaceId::Underlay(m.id));
                }
                layer.detach_all();
                attach_underlays(&mut layer, &mut gfx, &session::monitors());
            }
        }

        if last_report.elapsed() >= Duration::from_secs(15) {
            last_report = Instant::now();
            say!("coverage: {}", bare_report(&grid, &mons));
        }

        // Fixed 20 Hz sim; this frame's inputs go to the first tick (D13).
        let now = Instant::now();
        acc += (now - last).as_secs_f32().min(0.25);
        last = now;
        while acc >= DT {
            acc -= DT;
            let events = sim.step(&FrameInputs { pulses: pulse_acc, cells: &cells, swept: &swept, moved: &moved, target, grid: &grid, bedrock: &bedrock });
            pulse_acc = PulseBatch::default();
            cells.clear();
            swept.clear();
            moved.clear();
            let why = (target.map(|t| (t.id, t.frame, t.maximized)), sim.build_block());
            if why != last_why {
                match why.0 {
                    Some((id, f, max)) => say!(
                        "target {:x} ({},{})-({},{}){}: {}",
                        id.0, f.l, f.t, f.r, f.b,
                        if max { " maximized" } else { "" },
                        match why.1 { None => "rigged / building".to_string(), Some(b) => format!("no rig: {b:?}") }
                    ),
                    None => say!("no target window"),
                }
                last_why = why;
            }
            for e in events {
                match e {
                    SimEvent::RigDeployed { id } => {
                        rigs_built += 1;
                        let ropes = sim.rigs().iter().find(|r| r.id == id).map(|r| r.ropes.iter().flatten().count()).unwrap_or(0);
                        say!("rig {id} deployed with {ropes} rope(s){}", if ropes == 0 { " (no bare desktop beside the window: miners jump)" } else { "" });
                    }
                    SimEvent::StructureCrushed { id, salvage } => {
                        crushes += 1;
                        say!("rig {id} crushed: {salvage} salvage falling home");
                    }
                    SimEvent::StructureBuried { id } => say!("rig {id} buried"),
                    SimEvent::StructureUnburied { id } => say!("rig {id} dug out"),
                    _ => {}
                }
            }
        }

        // Pacer (D10): ambient 24, active 30, burst 60; nothing while a fullscreen app owns the screen.
        if Instant::now() >= next_frame {
            let hz = match sim.anim_demand() {
                AnimDemand::Ambient => 24.0,
                AnimDemand::Active => 30.0,
                AnimDemand::Burst => 60.0,
            };
            next_frame = Instant::now() + Duration::from_secs_f32(1.0 / hz);
            if !stood_down {
                let frame = extract::frame(&sim, &mons, &bars, scale as f32);
                if let Err(e) = gfx.submit(&frame) {
                    say!("submit: {e}");
                }
                frames += 1;
            }
        }
    }

    pulses.stop();
    tracker.stop();
    gfx.detach(SurfaceId::Trench);
    gfx.detach(SurfaceId::Shaft);
    dock.undock_all();
    for m in &mons {
        gfx.detach(SurfaceId::Underlay(m.id));
    }
    layer.detach_all();
    session::mark_cleaned();
    let l = sim.ledger();
    say!(
        "exit after {:.0} s: {frames} frames, {rigs_built} rig(s) built, {crushes} crush(es); scrap {} fuel {} salvage {} delivered {}. Bars undocked, underlays detached, wallpaper never written.",
        start.elapsed().as_secs_f32(),
        l.scrap,
        l.fuel,
        l.salvage_total,
        l.delivered_total
    );
}
