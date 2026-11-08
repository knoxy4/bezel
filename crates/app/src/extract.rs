//! Sim → draw lists (ADR §6 `extract()`). Phase 4: hero sprites from the Gemini atlas (D46), with plain shapes as
//! the fallback when a sprite (or the whole atlas) is missing. Ropes and glows stay procedural.

use std::collections::HashMap;

use bezel_core::{MonitorInfo, RectPx, SurfaceId};
use bezel_render_api::{DrawCmd, DrawList, Rgba8};
use bezel_sim::{Sim, UnitState};

use crate::atlas::Sprites;

const ROPE: Rgba8 = Rgba8([95, 216, 232, 255]); // cyan cables (docs/art/inspiration.md)
const MINER: Rgba8 = Rgba8([255, 140, 40, 255]);
const SALVAGE: Rgba8 = Rgba8([255, 210, 90, 255]);
const SPARK: Rgba8 = Rgba8([255, 230, 140, 255]);
const CORE: Rgba8 = Rgba8([120, 240, 255, 255]);
const HATCH: Rgba8 = Rgba8([255, 200, 90, 255]);
const TEXT: Rgba8 = Rgba8([255, 230, 200, 255]);

struct Paint<'a> {
    sp: &'a Sprites,
    s: f32,
}

impl Paint<'_> {
    /// Draw sprite `name` with its pivot at (x, y). Returns false if the atlas has no such sprite.
    fn spr(&self, cmds: &mut Vec<DrawCmd>, name: &str, x: f32, y: f32, flip: bool) -> bool {
        self.spr_a(cmds, name, x, y, flip, 255)
    }
    fn spr_a(&self, cmds: &mut Vec<DrawCmd>, name: &str, x: f32, y: f32, flip: bool, alpha: u8) -> bool {
        match self.sp.get(name) {
            Some(id) => {
                cmds.push(DrawCmd::Sprite { id, frame: 0, x, y, flip_x: flip, alpha, scale: self.s });
                true
            }
            None => false,
        }
    }
    /// Repeat `name` along x from x0 to x1 (the last tile is pulled back to end exactly at x1), pivot y at `y`.
    fn tile_x(&self, cmds: &mut Vec<DrawCmd>, name: &str, w: f32, x0: f32, x1: f32, y: f32) {
        let tw = w * self.s;
        if tw <= 0.0 || x1 - x0 < tw {
            return;
        }
        let mut x = x0;
        while x + tw <= x1 {
            self.spr(cmds, name, x + tw / 2.0, y, false);
            x += tw;
        }
        if x < x1 {
            self.spr(cmds, name, x1 - tw / 2.0, y, false);
        }
    }
    fn miner_box(&self, cmds: &mut Vec<DrawCmd>, x: f32, y: f32) {
        let s = self.s;
        cmds.push(DrawCmd::Fill { rect: [x - 4.0 * s, y - 12.0 * s, x + 4.0 * s, y], color: MINER });
    }
}

fn miner_sprite(u: &bezel_sim::Unit, tick: u64) -> (String, bool) {
    let phase = ((tick / 5) as u32 + u.id) % 2;
    match &u.state {
        UnitState::AtRig { .. } | UnitState::WallMine { .. } => ("miner_mine".into(), u.id % 2 == 0),
        UnitState::Climbing { .. } | UnitState::WallClimb { .. } => ("miner_climb".into(), false),
        UnitState::WallSlide { .. } => ("miner_climb".into(), true),
        UnitState::Falling(b) if b.chute => (format!("chute_{}", ((tick / 8) as u32 + u.id) % 3), false),
        UnitState::Falling(_) => ("miner_fall".into(), u.id % 2 == 0),
        UnitState::Walking { dir, .. } => {
            let name = if u.carrying > 0 { "miner_carry".to_string() } else { format!("miner_walk_{phase}") };
            (name, *dir < 0.0)
        }
        UnitState::Home { .. } => ("miner_idle".into(), false),
    }
}

/// Sprite sizes (art px) the layout needs; they match tools/build_sprites.py output.
const GIRDER_W: f32 = 45.0;
const EARTH_W: f32 = 116.0;
const RAIL_W: f32 = 116.0;
const RAIL_H: f32 = 29.0;
const SHAFT_TILE_W: f32 = 65.0;
const SHAFT_TILE_H: f32 = 126.0;
const PIPE_H: f32 = 127.0;

/// One frame for every surface. Underlay coords are monitor-local; bar coords are bar-local.
pub fn frame(sim: &Sim, mons: &[MonitorInfo], bars: &HashMap<SurfaceId, RectPx>, s: f32, sprites: &Sprites) -> Vec<DrawList> {
    let p = Paint { sp: sprites, s };
    let tick = sim.tick().0;
    let mut out = Vec::new();

    for m in mons {
        let b = m.bounds;
        let (ox, oy) = (b.l as f32, b.t as f32);
        let on = |x: f32, y: f32| b.contains(x as i32, y as i32);
        let mut cmds = Vec::new();
        for rig in sim.rigs().iter().filter(|r| !r.buried) {
            let f = rig.anchor;
            if !(on(f.l as f32, f.t as f32 - 1.0) || on(f.r as f32 - 1.0, f.t as f32 - 1.0)) {
                continue;
            }
            // A girder along the window's whole top edge, the outpost on it, davits holding the cables.
            p.tile_x(&mut cmds, "girder_seg", GIRDER_W, f.l as f32 - ox, f.r as f32 - ox, f.t as f32 - oy);
            let cx = ((rig.rect.l + rig.rect.r) / 2) as f32 - ox;
            if !p.spr(&mut cmds, "outpost_full", cx, f.t as f32 - oy, false) {
                let r = rig.rect;
                cmds.push(DrawCmd::Fill { rect: [r.l as f32 - ox, r.t as f32 - oy, r.r as f32 - ox, r.b as f32 - oy], color: Rgba8([92, 66, 48, 255]) });
            }
            for (k, (base, tip)) in rig.cranes.iter().enumerate().filter_map(|(k, c)| c.map(|c| (k, c))) {
                let name = if k == 0 { "davit_l" } else { "davit_r" };
                if !p.spr(&mut cmds, name, tip[0] - ox, tip[1] - oy, false) {
                    cmds.push(DrawCmd::Polyline { pts: vec![[base[0] - ox, base[1] - oy], [base[0] - ox, tip[1] - oy], [tip[0] - ox, tip[1] - oy]], width: 2.5 * s, color: SALVAGE, glow: false });
                }
            }
            for r in rig.ropes.iter().flatten() {
                let pts: Vec<[f32; 2]> = sim.rope_nodes(*r).iter().map(|q| [q[0] - ox, q[1] - oy]).collect();
                cmds.push(DrawCmd::Polyline { pts, width: 2.0 * s, color: ROPE, glow: true });
            }
        }
        for (r, ttl) in sim.loose_ropes() {
            let pts: Vec<[f32; 2]> = sim.rope_nodes(*r).iter().map(|q| [q[0] - ox, q[1] - oy]).collect();
            if pts.iter().any(|q| on(q[0] + ox, q[1] + oy)) {
                let a = (255.0 * (*ttl as f32 / 60.0).min(1.0)) as u8;
                cmds.push(DrawCmd::Polyline { pts, width: 2.0 * s, color: Rgba8([ROPE.0[0], ROPE.0[1], ROPE.0[2], a]), glow: true });
            }
        }
        for u in sim.units() {
            let shaft_or_home = matches!(u.state, UnitState::WallClimb { .. } | UnitState::WallMine { .. } | UnitState::WallSlide { .. } | UnitState::Home { .. });
            if shaft_or_home || !on(u.pos[0], u.pos[1]) {
                continue;
            }
            if let UnitState::AtRig { rig, .. } = u.state {
                if sim.rigs().iter().any(|r| r.id == rig && r.buried) {
                    continue;
                }
            }
            let (name, flip) = miner_sprite(u, tick);
            if !p.spr(&mut cmds, &name, u.pos[0] - ox, u.pos[1] - oy, flip) {
                p.miner_box(&mut cmds, u.pos[0] - ox, u.pos[1] - oy);
            }
        }
        for (i, sv) in sim.salvage().iter().enumerate() {
            if on(sv.body.pos[0], sv.body.pos[1]) {
                let name = ["crystal_m", "plank", "cog", "crystal_s", "debris_0", "girder_bent"][i % 6];
                if !p.spr(&mut cmds, name, sv.body.pos[0] - ox, sv.body.pos[1] - oy, i % 2 == 1) {
                    cmds.push(DrawCmd::Particles { pts: vec![[sv.body.pos[0] - ox, sv.body.pos[1] - oy, 7.0 * s]], color: SALVAGE });
                }
            }
        }
        for sk in sim.sparks().iter().filter(|k| on(k.pos[0], k.pos[1])) {
            let name = format!("spark_{}", 3 - (sk.ttl.min(31) / 8) as usize);
            if !p.spr(&mut cmds, &name, sk.pos[0] - ox, sk.pos[1] - oy, false) {
                cmds.push(DrawCmd::Particles { pts: vec![[sk.pos[0] - ox, sk.pos[1] - oy, 3.0 * s]], color: SPARK });
            }
        }
        out.push(DrawList { surface: SurfaceId::Underlay(m.id), dirty: Vec::new(), cmds });
    }

    let l = sim.ledger();
    if let Some(tr) = bars.get(&SurfaceId::Trench) {
        let shaft_w = bars.get(&SurfaceId::Shaft).map(|r| r.width() as f32).unwrap_or(0.0);
        let (w, h) = (tr.width() as f32, tr.height() as f32);
        let heat = sim.heat();
        let mut c = vec![DrawCmd::Fill { rect: [0.0, 0.0, w, h], color: Rgba8([26, 20, 22, 255]) }];
        // Cross-section: rail along the top, earth along the bottom.
        p.tile_x(&mut c, "tile_earth", EARTH_W, 0.0, w, h);
        p.tile_x(&mut c, "tile_rail", RAIL_W, 0.0, w, RAIL_H * s);
        // The foundry: the fire grows with heat and flickers.
        let fx = shaft_w + 60.0 * s;
        let fire = if heat < 0.15 { 0 } else if heat < 0.55 { 1 + (tick / 6 % 2) as usize } else { 2 - (tick / 4 % 2) as usize };
        if !p.spr(&mut c, &format!("furnace_{fire}"), fx, h - 4.0 * s, false) {
            c.push(DrawCmd::Fill { rect: [fx - 28.0 * s, h - 60.0 * s, fx + 28.0 * s, h - 4.0 * s], color: Rgba8([120, 60, 30, 255]) });
        }
        p.spr(&mut c, "anvil", fx + 48.0 * s, h - 4.0 * s, false);
        p.spr(&mut c, "bucket", fx + 76.0 * s, h - 4.0 * s, false);
        p.spr(&mut c, "crates", fx - 46.0 * s, h - 4.0 * s, false);
        // Heat gauge and the ledger.
        let (gx, gy, gw, gh) = (fx + 100.0 * s, RAIL_H * s + 8.0 * s, 200.0 * s, 12.0 * s);
        c.push(DrawCmd::Fill { rect: [gx, gy, gx + gw, gy + gh], color: Rgba8([22, 16, 14, 230]) });
        let g = (90.0 + 165.0 * heat) as u8;
        c.push(DrawCmd::Fill { rect: [gx, gy, gx + gw * heat, gy + gh], color: Rgba8([255, g, 30, 255]) });
        c.push(DrawCmd::Text { s: format!("FOUNDRY {:>3.0}%", heat * 100.0), x: gx, y: gy + gh + 2.0 * s, size: 14.0 * s, color: Rgba8([255, 200, 150, 255]) });
        c.push(DrawCmd::Text {
            s: format!("scrap {}   fuel {}   salvage {}   hauled {}   cores {}", l.scrap, l.fuel, l.salvage_total, l.delivered_total, l.cores),
            x: gx + gw + 24.0 * s,
            y: gy - 2.0 * s,
            size: 18.0 * s,
            color: TEXT,
        });
        // Miners resting at home stand by the furnace.
        let mut k = 0.0;
        for _ in sim.units().iter().filter(|u| matches!(u.state, UnitState::Home { .. })) {
            let x = fx + 110.0 * s + k * 18.0 * s;
            if !p.spr(&mut c, "miner_idle", x, h - 6.0 * s, (k as i32) % 2 == 1) {
                p.miner_box(&mut c, x, h - 6.0 * s);
            }
            k += 1.0;
        }
        // The terrarium: ambient plants at the far end (fest cut: set dressing only).
        let mut px = w - 30.0 * s;
        for name in ["plant_star", "plant_spiky", "plant_mushroom", "plant_eye", "plant_tentacle", "plant_fern"] {
            p.spr(&mut c, name, px, h - 4.0 * s, false);
            px -= 30.0 * s;
        }
        // Hatches: a glow in the trench roof where cargo just came home.
        for (x, ttl) in sim.hatches() {
            let lx = x - tr.l as f32;
            if lx >= 0.0 && lx < w {
                let a = (60.0 + 195.0 * (*ttl as f32 / 24.0).min(1.0)) as u8;
                c.push(DrawCmd::Fill { rect: [lx - 14.0 * s, 0.0, lx + 14.0 * s, 6.0 * s], color: Rgba8([HATCH.0[0], HATCH.0[1], HATCH.0[2], a]) });
                p.spr_a(&mut c, "ping_2", lx, 10.0 * s, false, a);
            }
        }
        out.push(DrawList { surface: SurfaceId::Trench, dirty: Vec::new(), cmds: c });
    }

    if let Some(sh) = bars.get(&SurfaceId::Shaft) {
        let (w, h) = (sh.width() as f32, sh.height() as f32);
        let (ox, oy) = (sh.l as f32, sh.t as f32);
        let mut c = vec![DrawCmd::Fill { rect: [0.0, 0.0, w, h], color: Rgba8([24, 22, 34, 255]) }];
        // Stone walls scroll down as the drill bores up (one tile per 16 m).
        let th = SHAFT_TILE_H * s;
        let scroll = (sim.depth() as f32 * th / 16.0) % th;
        let mut y = -th + scroll;
        while y < h {
            let mut x = 0.0;
            while x < w {
                p.spr(&mut c, "tile_shaft", x + SHAFT_TILE_W * s / 2.0, y + th, false);
                x += SHAFT_TILE_W * s;
            }
            y += th;
        }
        // Inner-wall pipe: the wall-run line runs beside it.
        let mut py = 0.0;
        while py < h {
            p.spr(&mut c, "tile_pipe", w - 9.0 * s, py + PIPE_H * s, false);
            py += PIPE_H * s;
        }
        // The drill, spinning while it has fuel; the elevator cage at the bottom.
        let spin = if sim.drilling() { (tick / 3 % 3) as usize } else { 0 };
        if !p.spr(&mut c, &format!("drill_{spin}"), w * 0.5, 4.0 * s, false) {
            c.push(DrawCmd::Fill { rect: [w * 0.5 - 16.0 * s, 4.0 * s, w * 0.5 + 16.0 * s, 70.0 * s], color: Rgba8([90, 96, 120, 255]) });
        }
        let crew_home = sim.units().iter().any(|u| matches!(u.state, UnitState::Home { .. }));
        p.spr(&mut c, if crew_home { "cage_full" } else { "cage_empty" }, w * 0.42, h - 4.0 * s, false);
        c.push(DrawCmd::Text { s: format!("{} m", sim.depth()), x: 8.0 * s, y: 96.0 * s, size: 15.0 * s, color: Rgba8([190, 210, 255, 255]) });
        // Next core: a gauge down the outer wall.
        let gp = sim.core_progress();
        let (g0, g1) = (120.0 * s, h - 70.0 * s);
        c.push(DrawCmd::Fill { rect: [4.0 * s, g0, 9.0 * s, g1], color: Rgba8([14, 13, 22, 255]) });
        c.push(DrawCmd::Fill { rect: [4.0 * s, g1 - (g1 - g0) * gp, 9.0 * s, g1], color: CORE });
        for cb in sim.cores_falling() {
            if !p.spr(&mut c, "core", cb.pos[0] - ox, cb.pos[1] - oy, false) {
                c.push(DrawCmd::Fill { rect: [cb.pos[0] - ox - 9.0 * s, cb.pos[1] - oy - 9.0 * s, cb.pos[0] - ox + 9.0 * s, cb.pos[1] - oy + 9.0 * s], color: CORE });
            }
        }
        // Wall-run: the grapple bites the window's top-left corner; the crew's line runs down the inner wall.
        if let Some(wr) = sim.wallrun() {
            let (hy, lx) = (wr.hook[1] - oy, wr.wall_x - ox);
            c.push(DrawCmd::Polyline { pts: vec![[lx, hy], [w - 1.0, hy]], width: 3.0 * s, color: ROPE, glow: true });
            c.push(DrawCmd::Polyline { pts: vec![[lx, hy], [lx, h]], width: 2.0 * s, color: ROPE, glow: true });
        }
        for u in sim.units() {
            let wall = matches!(u.state, UnitState::WallClimb { .. } | UnitState::WallMine { .. } | UnitState::WallSlide { .. });
            if wall && u.pos[1] < sh.b as f32 {
                let (name, flip) = miner_sprite(u, tick);
                if !p.spr(&mut c, &name, u.pos[0] - ox, u.pos[1] - oy + 14.0 * s, flip) {
                    p.miner_box(&mut c, u.pos[0] - ox, u.pos[1] - oy + 12.0 * s);
                }
            }
        }
        c.push(DrawCmd::Text { s: format!("FUEL {}", l.fuel), x: 8.0 * s, y: 116.0 * s, size: 14.0 * s, color: Rgba8([190, 210, 255, 255]) });
        out.push(DrawList { surface: SurfaceId::Shaft, dirty: Vec::new(), cmds: c });
    }
    out
}
