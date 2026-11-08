//! Sim → draw lists (ADR §6 `extract()`). Phase 3 is deliberately ugly: filled rects, ropes as glowing polylines, no
//! sprites until the atlas lands (phase 4).

use std::collections::HashMap;

use bezel_core::{MonitorInfo, RectPx, SurfaceId};
use bezel_render_api::{DrawCmd, DrawList, Rgba8};
use bezel_sim::{Sim, UnitState};

const RIG: Rgba8 = Rgba8([92, 66, 48, 255]);
const RIG_ROOF: Rgba8 = Rgba8([255, 176, 64, 255]);
const ROPE: Rgba8 = Rgba8([235, 190, 120, 255]);
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
            if matches!(u.state, UnitState::Home { .. }) || !on(u.pos[0], u.pos[1]) {
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
            s: format!("scrap {}   fuel {}   salvage {}   hauled {}", l.scrap, l.fuel, l.salvage_total, l.delivered_total),
            x: fx + fw + 32.0 * s,
            y: 22.0 * s,
            size: 20.0 * s,
            color: Rgba8([255, 230, 200, 255]),
        });
        c.push(DrawCmd::Text { s: "BEZEL · phase 3 loop (ugly on purpose)".into(), x: fx + fw + 32.0 * s, y: 56.0 * s, size: 14.0 * s, color: Rgba8([180, 150, 130, 255]) });
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
        let mut c = vec![DrawCmd::Fill { rect: [0.0, 0.0, w, h], color: Rgba8([24, 22, 34, 255]) }];
        // Fuel tank: fills upward; the drill (phase 4) will burn it.
        let frac = (l.fuel as f32 / 100.0).min(1.0);
        let (tx, tw) = (w * 0.3, w * 0.4);
        let (top, bot) = (h * 0.15, h - 24.0 * s);
        c.push(DrawCmd::Fill { rect: [tx, top, tx + tw, bot], color: Rgba8([14, 13, 22, 255]) });
        c.push(DrawCmd::Fill { rect: [tx, bot - (bot - top) * frac, tx + tw, bot], color: Rgba8([120, 200, 255, 255]) });
        c.push(DrawCmd::Text { s: format!("FUEL {}", l.fuel), x: 12.0 * s, y: top - 30.0 * s, size: 16.0 * s, color: Rgba8([190, 210, 255, 255]) });
        out.push(DrawList { surface: SurfaceId::Shaft, dirty: Vec::new(), cmds: c });
    }
    out
}
