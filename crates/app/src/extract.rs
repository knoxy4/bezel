//! Sim → draw lists (ADR §6 `extract()`). Phase 3 is deliberately ugly: filled rects, ropes as glowing polylines, no
//! sprites until the atlas lands (phase 4).

use std::collections::HashMap;

use bezel_core::{MonitorInfo, RectPx, SurfaceId};
use bezel_render_api::{DrawCmd, DrawList, Rgba8};
use bezel_sim::{Sim, UnitState};

const RIG: Rgba8 = Rgba8([92, 66, 48, 255]);
const RIG_ROOF: Rgba8 = Rgba8([255, 176, 64, 255]);
const ROPE: Rgba8 = Rgba8([95, 216, 232, 255]); // cyan cables (docs/art/inspiration.md)
const SPARK: Rgba8 = Rgba8([255, 230, 140, 255]);
const CORE: Rgba8 = Rgba8([120, 240, 255, 255]);
const HATCH: Rgba8 = Rgba8([255, 200, 90, 255]);
const MINER: Rgba8 = Rgba8([255, 140, 40, 255]);
const LAMP: Rgba8 = Rgba8([255, 250, 190, 255]);
const CHUTE: Rgba8 = Rgba8([90, 210, 200, 255]);
const SALVAGE: Rgba8 = Rgba8([255, 210, 90, 255]);

fn miner(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, s: f32, chute: bool) {
    let (w, h) = (8.0 * s, 12.0 * s);
    cmds.push(DrawCmd::Fill { rect: [x - w / 2.0, y - h, x + w / 2.0, y], color: MINER });
    cmds.push(DrawCmd::Fill { rect: [x - 2.0 * s, y - h + 2.0 * s, x + 2.0 * s, y - h + 5.0 * s], color: LAMP });
    if chute {
        let top = y - h - 22.0 * s;
        cmds.push(DrawCmd::Polyline {
            pts: vec![[x - 14.0 * s, top + 8.0 * s], [x - 8.0 * s, top + 1.0 * s], [x, top], [x + 8.0 * s, top + 1.0 * s], [x + 14.0 * s, top + 8.0 * s]],
            width: 3.0 * s,
            color: CHUTE,
            glow: false,
        });
        cmds.push(DrawCmd::Polyline { pts: vec![[x - 14.0 * s, top + 8.0 * s], [x, y - h]], width: 1.0, color: CHUTE, glow: false });
        cmds.push(DrawCmd::Polyline { pts: vec![[x + 14.0 * s, top + 8.0 * s], [x, y - h]], width: 1.0, color: CHUTE, glow: false });
    }
}

/// One frame for every surface. Underlay coords are monitor-local; bar coords are bar-local.
pub fn frame(sim: &Sim, mons: &[MonitorInfo], bars: &HashMap<SurfaceId, RectPx>, s: f32) -> Vec<DrawList> {
    let mut out = Vec::new();

    for m in mons {
        let b = m.bounds;
        let (ox, oy) = (b.l as f32, b.t as f32);
        let on = |x: f32, y: f32| b.contains(x as i32, y as i32);
        let mut cmds = Vec::new();
        for rig in sim.rigs().iter().filter(|r| !r.buried) {
            // Davits: post up from the window's top corner, jib out over the side, a hook block at the tip.
            for (base, tip) in rig.cranes.iter().flatten() {
                if on(base[0], base[1]) || on(tip[0], tip[1]) {
                    let (bx, by, tx, ty) = (base[0] - ox, base[1] - oy, tip[0] - ox, tip[1] - oy);
                    cmds.push(DrawCmd::Polyline { pts: vec![[bx, by], [bx, ty], [tx, ty]], width: 2.5 * s, color: RIG_ROOF, glow: false });
                    cmds.push(DrawCmd::Polyline { pts: vec![[bx, ty + 7.0 * s], [bx + (tx - bx) * 0.45, ty]], width: 1.5 * s, color: RIG_ROOF, glow: false });
                    cmds.push(DrawCmd::Fill { rect: [tx - 3.0 * s, ty - 1.0 * s, tx + 3.0 * s, ty + 4.0 * s], color: RIG });
                }
            }
            for r in rig.ropes.iter().flatten() {
                let pts: Vec<[f32; 2]> = sim.rope_nodes(*r).iter().map(|p| [p[0] - ox, p[1] - oy]).collect();
                if pts.iter().any(|p| on(p[0] + ox, p[1] + oy)) {
                    cmds.push(DrawCmd::Polyline { pts, width: 2.0 * s, color: ROPE, glow: true });
                }
            }
            let r = rig.rect;
            if on(r.l as f32, r.t as f32) || on(r.r as f32 - 1.0, r.b as f32 - 1.0) {
                let (l, t, rr, bb) = (r.l as f32 - ox, r.t as f32 - oy, r.r as f32 - ox, r.b as f32 - oy);
                cmds.push(DrawCmd::Fill { rect: [l, t + (bb - t) * 0.35, rr, bb], color: RIG });
                cmds.push(DrawCmd::Fill { rect: [l - 4.0 * s, t + (bb - t) * 0.25, rr + 4.0 * s, t + (bb - t) * 0.38], color: RIG_ROOF });
                // A crane jib and a drill mast so it reads as a rig.
                cmds.push(DrawCmd::Polyline { pts: vec![[l + 8.0 * s, t + (bb - t) * 0.3], [l + 8.0 * s, t], [rr + 18.0 * s, t + 4.0 * s]], width: 2.0 * s, color: RIG_ROOF, glow: false });
                cmds.push(DrawCmd::Polyline { pts: vec![[rr - 10.0 * s, bb], [rr - 10.0 * s, t - 6.0 * s]], width: 3.0 * s, color: RIG_ROOF, glow: true });
            }
        }
        for u in sim.units() {
            let in_shaft = matches!(u.state, UnitState::WallClimb { .. } | UnitState::WallMine { .. } | UnitState::WallSlide { .. });
            if in_shaft || matches!(u.state, UnitState::Home { .. }) || !on(u.pos[0], u.pos[1]) {
                continue;
            }
            if let UnitState::AtRig { rig, .. } = u.state {
                if sim.rigs().iter().any(|r| r.id == rig && r.buried) {
                    continue;
                }
            }
            let chute = matches!(&u.state, UnitState::Falling(b) if b.chute);
            miner(&mut cmds, u.pos[0] - ox, u.pos[1] - oy, s, chute);
        }
        for (r, ttl) in sim.loose_ropes() {
            let pts: Vec<[f32; 2]> = sim.rope_nodes(*r).iter().map(|p| [p[0] - ox, p[1] - oy]).collect();
            if pts.iter().any(|p| on(p[0] + ox, p[1] + oy)) {
                let a = (255.0 * (*ttl as f32 / 60.0).min(1.0)) as u8;
                cmds.push(DrawCmd::Polyline { pts, width: 2.0 * s, color: Rgba8([ROPE.0[0], ROPE.0[1], ROPE.0[2], a]), glow: true });
            }
        }
        let sparks: Vec<[f32; 3]> = sim.sparks().iter().filter(|p| on(p.pos[0], p.pos[1])).map(|p| [p.pos[0] - ox, p.pos[1] - oy, (2.0 + p.ttl as f32 * 0.15) * s]).collect();
        if !sparks.is_empty() {
            cmds.push(DrawCmd::Particles { pts: sparks, color: SPARK });
        }
        let pts: Vec<[f32; 3]> = sim.salvage().iter().filter(|p| on(p.body.pos[0], p.body.pos[1])).map(|p| [p.body.pos[0] - ox, p.body.pos[1] - oy, 7.0 * s]).collect();
        if !pts.is_empty() {
            cmds.push(DrawCmd::Particles { pts, color: SALVAGE });
        }
        out.push(DrawList { surface: SurfaceId::Underlay(m.id), dirty: Vec::new(), cmds });
    }

    let l = sim.ledger();
    if let Some(tr) = bars.get(&SurfaceId::Trench) {
        let shaft_w = bars.get(&SurfaceId::Shaft).map(|r| r.width() as f32).unwrap_or(0.0);
        let (w, h) = (tr.width() as f32, tr.height() as f32);
        let heat = sim.heat();
        let mut c = vec![DrawCmd::Fill { rect: [0.0, 0.0, w, h], color: Rgba8([40, 29, 25, 255]) }];
        // Foundry: a heat bar that glows hotter as it fills.
        let (fx, fy, fw, fh) = (shaft_w + 24.0 * s, 16.0 * s, 260.0 * s, 18.0 * s);
        c.push(DrawCmd::Fill { rect: [fx, fy, fx + fw, fy + fh], color: Rgba8([22, 16, 14, 255]) });
        let g = (90.0 + 165.0 * heat) as u8;
        c.push(DrawCmd::Fill { rect: [fx, fy, fx + fw * heat, fy + fh], color: Rgba8([255, g, 30, 255]) });
        c.push(DrawCmd::Text { s: format!("FOUNDRY {:>3.0}%", heat * 100.0), x: fx, y: fy + fh + 6.0 * s, size: 16.0 * s, color: Rgba8([255, 200, 150, 255]) });
        c.push(DrawCmd::Text {
            s: format!("scrap {}   fuel {}   salvage {}   hauled {}   cores {}", l.scrap, l.fuel, l.salvage_total, l.delivered_total, l.cores),
            x: fx + fw + 32.0 * s,
            y: 22.0 * s,
            size: 20.0 * s,
            color: Rgba8([255, 230, 200, 255]),
        });
        c.push(DrawCmd::Text { s: "BEZEL · phase 3 loop (ugly on purpose)".into(), x: fx + fw + 32.0 * s, y: 56.0 * s, size: 14.0 * s, color: Rgba8([180, 150, 130, 255]) });
        // Hatches: a glow in the trench roof where cargo just came home.
        for (x, ttl) in sim.hatches() {
            let lx = x - tr.l as f32;
            if lx >= 0.0 && lx < w {
                let a = (60.0 + 195.0 * (*ttl as f32 / 24.0).min(1.0)) as u8;
                c.push(DrawCmd::Fill { rect: [lx - 14.0 * s, 0.0, lx + 14.0 * s, 8.0 * s], color: Rgba8([HATCH.0[0], HATCH.0[1], HATCH.0[2], a]) });
                c.push(DrawCmd::Fill { rect: [lx - 2.0 * s, 8.0 * s, lx + 2.0 * s, 26.0 * s], color: Rgba8([HATCH.0[0], HATCH.0[1], HATCH.0[2], a / 2]) });
            }
        }
        // Miners resting at home line up along the trench floor.
        let mut k = 0.0;
        for _ in sim.units().iter().filter(|u| matches!(u.state, UnitState::Home { .. })) {
            miner(&mut c, fx + k * 16.0 * s + 6.0 * s, h - 10.0 * s, s, false);
            k += 1.0;
        }
        out.push(DrawList { surface: SurfaceId::Trench, dirty: Vec::new(), cmds: c });
    }
    if let Some(sh) = bars.get(&SurfaceId::Shaft) {
        let (w, h) = (sh.width() as f32, sh.height() as f32);
        let (ox, oy) = (sh.l as f32, sh.t as f32);
        let mut c = vec![DrawCmd::Fill { rect: [0.0, 0.0, w, h], color: Rgba8([24, 22, 34, 255]) }];
        // Strata scroll down as the drill bores up (one band per 8 m).
        let band = 90.0 * s;
        let scroll = (sim.depth() as f32 * band / 8.0) % band;
        let mut y = -band + scroll;
        let mut k = (sim.depth() / 8) as i64;
        while y < h {
            let col = if k % 2 == 0 { Rgba8([34, 30, 48, 255]) } else { Rgba8([28, 26, 40, 255]) };
            c.push(DrawCmd::Fill { rect: [0.0, y, w, y + band], color: col });
            y += band;
            k -= 1;
        }
        // Inner-wall pipe (the wall-run line runs beside it).
        c.push(DrawCmd::Fill { rect: [w - 6.0 * s, 0.0, w - 3.0 * s, h], color: Rgba8([70, 72, 88, 255]) });
        // The drill at the top of the shaft, spinning while it has fuel.
        let (dx, dy) = (w * 0.5, 54.0 * s);
        let spin = if sim.drilling() { (sim.tick().0 % 4) as f32 } else { 0.0 };
        c.push(DrawCmd::Fill { rect: [dx - 22.0 * s, dy - 30.0 * s, dx + 22.0 * s, dy - 8.0 * s], color: Rgba8([90, 96, 120, 255]) });
        let tip = dy + 26.0 * s;
        for i in 0..4 {
            let yy = dy - 8.0 * s + (i as f32 + spin * 0.25) * 8.0 * s;
            let half = 20.0 * s * (1.0 - (yy - (dy - 8.0 * s)) / (tip - (dy - 8.0 * s)));
            c.push(DrawCmd::Polyline { pts: vec![[dx - half, yy], [dx + half, yy + 4.0 * s]], width: 2.0 * s, color: Rgba8([200, 210, 230, 255]), glow: false });
        }
        c.push(DrawCmd::Polyline { pts: vec![[dx - 20.0 * s, dy - 8.0 * s], [dx, tip], [dx + 20.0 * s, dy - 8.0 * s]], width: 2.0 * s, color: Rgba8([160, 170, 200, 255]), glow: sim.drilling() });
        c.push(DrawCmd::Text { s: format!("{} m", sim.depth()), x: 10.0 * s, y: dy + 34.0 * s, size: 15.0 * s, color: Rgba8([190, 210, 255, 255]) });
        // Next core: a gauge down the outer wall.
        let gp = sim.core_progress();
        c.push(DrawCmd::Fill { rect: [6.0 * s, dy + 60.0 * s, 12.0 * s, h - 40.0 * s], color: Rgba8([14, 13, 22, 255]) });
        let gy = h - 40.0 * s - (h - 100.0 * s - dy) * gp;
        c.push(DrawCmd::Fill { rect: [6.0 * s, gy, 12.0 * s, h - 40.0 * s], color: CORE });
        // Falling cores.
        for cb in sim.cores_falling() {
            let (x, y) = (cb.pos[0] - ox, cb.pos[1] - oy);
            c.push(DrawCmd::Fill { rect: [x - 9.0 * s, y - 9.0 * s, x + 9.0 * s, y + 9.0 * s], color: CORE });
            c.push(DrawCmd::Fill { rect: [x - 4.0 * s, y - 4.0 * s, x + 4.0 * s, y + 4.0 * s], color: Rgba8([255, 255, 255, 255]) });
        }
        // Wall-run: the grapple bites the window's top-left corner; the crew's line runs down the inner wall.
        if let Some(wr) = sim.wallrun() {
            let (hx, hy, lx) = (wr.hook[0] - ox, wr.hook[1] - oy, wr.wall_x - ox);
            c.push(DrawCmd::Polyline { pts: vec![[lx, hy], [w - 1.0, hy]], width: 3.0 * s, color: ROPE, glow: true });
            c.push(DrawCmd::Fill { rect: [hx - 8.0 * s, hy - 5.0 * s, hx, hy + 5.0 * s], color: RIG_ROOF });
            c.push(DrawCmd::Polyline { pts: vec![[lx, hy], [lx, h]], width: 2.0 * s, color: ROPE, glow: true });
        }
        for u in sim.units() {
            let wall = matches!(u.state, UnitState::WallClimb { .. } | UnitState::WallMine { .. } | UnitState::WallSlide { .. });
            if wall && u.pos[1] < sh.b as f32 {
                miner(&mut c, u.pos[0] - ox - 6.0 * s, u.pos[1] - oy + 12.0 * s, s, false);
            }
        }
        c.push(DrawCmd::Text { s: format!("FUEL {}", l.fuel), x: 10.0 * s, y: h - 30.0 * s, size: 15.0 * s, color: Rgba8([190, 210, 255, 255]) });
        out.push(DrawList { surface: SurfaceId::Shaft, dirty: Vec::new(), cmds: c });
    }
    out
}
