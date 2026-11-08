//! Fixed-tick game simulation (ADR-0001 §6). Pure: no OS, no wall clock, seeded RNG, 20 Hz (D13).
//!
//! Phase 3 scope (the first full loop):
//! - **Foundry** (trench): input pulses heat it (D6 weights); heat smelts scrap into fuel.
//! - **Canopy, frontier mode** (§2.3): a rig on the target window's top edge when ≥ 88 px of bare cells sit above it,
//!   ropes down its sides through bare cells, miners that mine, climb down, parachute, land on window platforms (D24)
//!   or bedrock, and carry scrap home.
//! - **Crush / bury** (D21): a drag sweep crushes; anything else that covers buries; uncovering unburies. Anything
//!   anchored to a window that moves or vanishes snaps. A crush returns 60% of build cost as salvage (D25) that falls
//!   home.
//!
//! The §8 `Zone` split (canopy / frontier / trench / shaft behind `economy::Ledger`) comes when the zones grow; phase 3
//! keeps them in one struct.

use bezel_core::{CellId, ExtWindowId, Platform, Px, PulseBatch, RectCause, RectPx, TargetWindow, Tick, SIM_HZ};
use bezel_layout::{CellDelta, OcclusionGrid};
use bezel_physics::{fall_step, FallBody, Landing, RopeId, RopeWorld};

pub const DT: f32 = 1.0 / SIM_HZ as f32;

#[derive(Clone, Debug, PartialEq)]
pub struct SimConfig {
    /// Physical px per art px on the home monitor.
    pub art_scale: f32,
    pub rig_cost: u32,
    pub salvage_pct: u32,
    pub start_scrap: u32,
    pub max_rigs: usize,
    pub miners_per_rig: usize,
    /// Ticks a miner spends mining one load at the rig.
    pub mine_ticks: u32,
    pub scrap_per_load: u32,
    /// Ticks a delivered miner waits before riding the elevator back to a rig.
    pub home_ticks: u32,
    pub rebuild_cooldown_ticks: u32,
    /// D6: keyboard ×3, wheel ×3, clicks ×2, mouse travel ×2 (per 100 px).
    pub w_keys: f32,
    pub w_wheel: f32,
    pub w_clicks: f32,
    pub w_travel_per_100px: f32,
    pub heat_decay: f32,
    pub smelt_heat: f32,
    pub heat_max: f32,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            art_scale: 1.0,
            rig_cost: 30,
            salvage_pct: 60,
            start_scrap: 60,
            max_rigs: 3,
            miners_per_rig: 3,
            mine_ticks: 40,
            scrap_per_load: 2,
            home_ticks: 30,
            rebuild_cooldown_ticks: 60,
            w_keys: 3.0,
            w_wheel: 3.0,
            w_clicks: 2.0,
            w_travel_per_100px: 2.0,
            heat_decay: 0.985,
            smelt_heat: 20.0,
            heat_max: 400.0,
        }
    }
}

/// What the app hands the sim each tick. Everything is accumulated since the previous tick.
pub struct FrameInputs<'a> {
    pub pulses: PulseBatch,
    /// Cause-tagged state changes from the occlusion grid (D21).
    pub cells: &'a [CellDelta],
    /// Cells crossed by drag sweeps (`OcclusionGrid::sweep` between drag samples), tagged with the dragged window.
    /// They crush anything but the dragged window's own rig (D45).
    pub swept: &'a [(ExtWindowId, CellId)],
    /// Windows whose rect actually changed (or vanished: `None`), with why. Rigs ride their window (D45).
    pub moved: &'a [(ExtWindowId, Option<RectPx>, RectCause)],
    pub target: Option<TargetWindow>,
    pub grid: &'a OcclusionGrid,
    /// Per monitor: its x range (bounds) and the bedrock y there (trench top on the home monitor, work-area bottom
    /// elsewhere).
    pub bedrock: &'a [(RectPx, Px)],
    /// SHAFT's rect on the home monitor (screen px): wall-run and the drill live inside it.
    pub shaft: Option<RectPx>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cargo {
    Scrap,
    Salvage,
    Fuel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandingKind {
    Platform,
    Bedrock,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SimEvent {
    RigDeployed { id: u32 },
    WallRunStarted { window: ExtWindowId },
    /// The target changed: the crew slides home with what it has.
    WallRunReleased,
    /// The hooked window moved: the line snaps and the crew falls.
    WallRunSnapped,
    /// A core from the drill landed in the trench (shaft lite).
    BossCore { tier: u8 },
    RopeSnapped(RopeId),
    UnitFalling { unit: u32 },
    UnitLanded { unit: u32, on: LandingKind },
    StructureCrushed { id: u32, salvage: u32, by: CrushCause },
    StructureBuried { id: u32 },
    StructureUnburied { id: u32 },
    CargoDelivered { kind: Cargo, qty: u32 },
    Smelted { fuel: u32 },
}

/// What destroyed a rig (diagnostics: a phase-4 run crushed rigs on their own window's first drag pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrushCause {
    /// Another window's drag sweep crossed it.
    Swept { by: ExtWindowId },
    /// Another window's drag covered it.
    DragCover { by: ExtWindowId, cause: RectCause },
    /// Its anchor window closed.
    AnchorClosed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnimDemand {
    Ambient,
    Active,
    Burst,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    pub scrap: u32,
    pub fuel: u32,
    pub salvage_total: u32,
    pub delivered_total: u32,
    pub cores: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rig {
    pub id: u32,
    pub window: ExtWindowId,
    /// Physical px; sits on the window's top edge, above it.
    pub rect: RectPx,
    pub buried: bool,
    /// Rope on the window's left / right side.
    pub ropes: [Option<RopeId>; 2],
    /// The davit holding each rope: (post base on the window's top corner, jib tip the rope hangs from). Knox, run 4.
    pub cranes: [Option<([f32; 2], [f32; 2])>; 2],
    /// The anchor window's frame the rig was last placed on.
    pub anchor: RectPx,
    /// The anchor is minimized, hidden or cloaked: the rig is buried until it comes back (D45).
    pub lost: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnitState {
    AtRig { rig: u32, t: u32 },
    Climbing { rig: u32, side: usize, s: f32 },
    Falling(FallBody),
    Walking { y: Px, x0: Px, x1: Px, dir: f32 },
    Home { t: u32 },
    /// Wall-run (§2.3): climbing SHAFT's inner wall toward the hook (y in screen px).
    WallClimb { y: f32 },
    WallMine { t: u32 },
    /// Sliding down the inner wall to the trench with a load.
    WallSlide { y: f32 },
}

/// A maximized or flush-left target hooked from inside SHAFT. Nothing is drawn over the window itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallRun {
    pub window: ExtWindowId,
    /// The window's top-left corner, where the grapple bites (screen px).
    pub hook: [f32; 2],
    /// The crew's line runs down SHAFT's inner wall at this x.
    pub wall_x: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spark {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub ttl: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub id: u32,
    pub pos: [f32; 2],
    pub carrying: u32,
    pub state: UnitState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SalvagePiece {
    pub body: FallBody,
    pub value: u32,
}

/// xorshift64*: deterministic per seed and per build (D13).
#[derive(Clone, Debug, PartialEq)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in [-1, 1).
    fn signed(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sim {
    cfg: SimConfig,
    tick: Tick,
    rng: Rng,
    ledger: Ledger,
    heat: f32,
    smelt_acc: f32,
    rigs: Vec<Rig>,
    ropes: RopeWorld,
    units: Vec<Unit>,
    salvage: Vec<SalvagePiece>,
    next_id: u32,
    cooldown: u32,
    burst_ticks: u32,
    build_block: Option<BuildBlock>,
    wallrun: Option<WallRun>,
    depth: u32,
    drilling: bool,
    burn_acc: f32,
    core_acc: f32,
    cores_falling: Vec<FallBody>,
    loose: Vec<(RopeId, u32)>,
    sparks: Vec<Spark>,
    /// Trench hatches flashing where cargo just came home: (screen x, ticks left).
    hatches: Vec<(f32, u32)>,
    last_target: Option<(ExtWindowId, RectPx)>,
    settled_ticks: u32,
}

/// Why the target window got no rig this tick (diagnostics; phase-3 gate run showed nothing and needed a why).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildBlock {
    NoTarget,
    Maximized,
    AlreadyRigged,
    Cooldown,
    RigCap,
    NoScrap,
    /// The target moved within the last second (likely mid-drag).
    Settling,
    /// The window's top edge is within 88 px of the top of its monitor.
    EdgeTooHigh,
    /// Something covers the 88 px above the edge (often a maximized window behind everything: the grid ignores z-order).
    SkyCovered,
}

const RIG_W: f32 = 64.0;
const RIG_H: f32 = 40.0;
const BARE_ABOVE: f32 = 88.0; // §2.3: an outpost needs ≥ 88 px of bare cells above the edge
const ROPE_GAP: f32 = 10.0; // ropes hang this far outside the window frame
const JIB_H: f32 = 22.0; // davit height above the window's top edge
const POST_INSET: f32 = 6.0; // davit post sits this far inside the window's top corner
const CLIMB_PX_PER_TICK: f32 = 9.0;
const WALK_PX_PER_TICK: f32 = 4.0;
const WALL_CLIMB_PX_PER_TICK: f32 = 7.0;
const WALL_SLIDE_PX_PER_TICK: f32 = 16.0;
/// Seconds of drilling per unit of fuel burned (one metre bored).
const FUEL_BURN_SECS: f32 = 1.5;
/// Seconds of drilling per core dropped (shaft lite: a core on a timer).
const CORE_SECS: f32 = 40.0;
const CORE_SCRAP: u32 = 15;
const LOOSE_ROPE_TICKS: u32 = 60;
const HATCH_TICKS: u32 = 24;
/// One second of a still target before a rig goes up.
const SETTLE_TICKS: u32 = SIM_HZ;

fn rope_point(nodes: &[[f32; 2]], s: f32) -> [f32; 2] {
    if nodes.is_empty() {
        return [0.0, 0.0];
    }
    let f = s.clamp(0.0, 1.0) * (nodes.len() - 1) as f32;
    let i = (f.floor() as usize).min(nodes.len() - 1);
    let j = (i + 1).min(nodes.len() - 1);
    let t = f - i as f32;
    [nodes[i][0] + (nodes[j][0] - nodes[i][0]) * t, nodes[i][1] + (nodes[j][1] - nodes[i][1]) * t]
}

/// The part of a rig the grid judges. The rig sits on its window's top edge, so its bottom cell row is the row that
/// holds that edge, which the window itself covers (any overlap covers a cell). Judge the rig without that row.
fn sky(r: RectPx, cell: Px) -> RectPx {
    RectPx::new(r.l, r.t, r.r, (r.b - cell).max(r.t + 1))
}

fn floor_at(bedrock: &[(RectPx, Px)], x: f32) -> Px {
    bedrock.iter().find(|(r, _)| (x as Px) >= r.l && (x as Px) < r.r).map(|(_, y)| *y).unwrap_or_else(|| bedrock.iter().map(|(_, y)| *y).max().unwrap_or(0))
}

impl Sim {
    pub fn new(cfg: SimConfig, seed: u64) -> Self {
        let ledger = Ledger { scrap: cfg.start_scrap, ..Ledger::default() };
        Self {
            cfg,
            tick: Tick(0),
            rng: Rng::new(seed),
            ledger,
            heat: 0.0,
            smelt_acc: 0.0,
            rigs: Vec::new(),
            ropes: RopeWorld::new(),
            units: Vec::new(),
            salvage: Vec::new(),
            next_id: 1,
            cooldown: 0,
            burst_ticks: 0,
            build_block: None,
            last_target: None,
            settled_ticks: 0,
            wallrun: None,
            depth: 0,
            drilling: false,
            burn_acc: 0.0,
            core_acc: 0.0,
            cores_falling: Vec::new(),
            loose: Vec::new(),
            sparks: Vec::new(),
            hatches: Vec::new(),
        }
    }

    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    // ── read-only views for render extraction ──
    pub fn tick(&self) -> Tick {
        self.tick
    }
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }
    /// 0..1 of the foundry's heat gauge.
    pub fn heat(&self) -> f32 {
        (self.heat / self.cfg.heat_max).clamp(0.0, 1.0)
    }
    pub fn rigs(&self) -> &[Rig] {
        &self.rigs
    }
    pub fn rope_nodes(&self, r: RopeId) -> &[[f32; 2]] {
        self.ropes.nodes(r)
    }
    pub fn units(&self) -> &[Unit] {
        &self.units
    }
    pub fn salvage(&self) -> &[SalvagePiece] {
        &self.salvage
    }
    /// Why the current target has no new rig (None after a build, or when nothing blocked).
    pub fn build_block(&self) -> Option<BuildBlock> {
        self.build_block
    }

    pub fn wallrun(&self) -> Option<&WallRun> {
        self.wallrun.as_ref()
    }
    /// Metres the drill has bored.
    pub fn depth(&self) -> u32 {
        self.depth
    }
    pub fn drilling(&self) -> bool {
        self.drilling
    }
    /// 0..1 progress to the next core.
    pub fn core_progress(&self) -> f32 {
        (self.core_acc / CORE_SECS).clamp(0.0, 1.0)
    }
    pub fn cores_falling(&self) -> &[FallBody] {
        &self.cores_falling
    }
    /// Cut ropes still whipping and falling, with ticks left.
    pub fn loose_ropes(&self) -> &[(RopeId, u32)] {
        &self.loose
    }
    pub fn sparks(&self) -> &[Spark] {
        &self.sparks
    }
    pub fn hatches(&self) -> &[(f32, u32)] {
        &self.hatches
    }

    pub fn anim_demand(&self) -> AnimDemand {
        if self.burst_ticks > 0 || !self.salvage.is_empty() || !self.sparks.is_empty() || !self.cores_falling.is_empty() {
            AnimDemand::Burst
        } else if self.drilling || self.units.iter().any(|u| !matches!(u.state, UnitState::Home { .. } | UnitState::AtRig { .. })) {
            AnimDemand::Active
        } else {
            AnimDemand::Ambient
        }
    }

    pub fn step(&mut self, inp: &FrameInputs) -> Vec<SimEvent> {
        let mut ev = Vec::new();
        self.tick.0 += 1;
        self.burst_ticks = self.burst_ticks.saturating_sub(1);
        self.cooldown = self.cooldown.saturating_sub(1);
        self.foundry(inp.pulses, &mut ev);
        self.catastrophes(inp, &mut ev);
        self.bury(inp.grid, &mut ev);
        self.maybe_build(inp, &mut ev);
        self.wallrun_step(inp, &mut ev);
        self.drill_step(inp, &mut ev);
        self.ropes.step(DT);
        self.units_step(inp, &mut ev);
        self.salvage_step(inp, &mut ev);
        self.fx_step();
        ev
    }

    fn foundry(&mut self, p: PulseBatch, ev: &mut Vec<SimEvent>) {
        let c = &self.cfg;
        let add = p.key_events as f32 * c.w_keys
            + p.wheel_ticks as f32 * c.w_wheel
            + p.mouse_clicks as f32 * c.w_clicks
            + p.mouse_travel_px as f32 / 100.0 * c.w_travel_per_100px;
        self.heat = (self.heat * c.heat_decay + add).min(c.heat_max);
        // Keep one rig's worth of scrap: smelting must never eat the loop's way back to income.
        if self.heat >= c.smelt_heat && self.ledger.scrap > c.rig_cost {
            // Hotter smelts faster: one unit of fuel per second at the threshold, ~5 per second at max heat.
            self.smelt_acc += DT * (1.0 + 4.0 * self.heat / c.heat_max);
            if self.smelt_acc >= 1.0 {
                self.smelt_acc -= 1.0;
                self.ledger.scrap -= 1;
                self.ledger.fuel += 1;
                ev.push(SimEvent::Smelted { fuel: 1 });
            }
        } else {
            self.smelt_acc = 0.0;
        }
    }

    /// D21 + D45: another window's drag sweep (or drag-caused coverage) crushes a rig; its own window carries it.
    fn catastrophes(&mut self, inp: &FrameInputs, ev: &mut Vec<SimEvent>) {
        for (w, rect, cause) in inp.moved {
            let ids: Vec<u32> = self.rigs.iter().filter(|r| r.window == *w).map(|r| r.id).collect();
            for id in ids {
                match (rect, cause) {
                    (None, RectCause::Destroyed) => self.crush(id, CrushCause::AnchorClosed, ev),
                    (None, _) => {
                        if let Some(r) = self.rigs.iter_mut().find(|r| r.id == id) {
                            r.lost = true;
                        }
                    }
                    (Some(f), _) => self.carry(id, *f),
                }
            }
        }
        let mut doomed: Vec<(u32, CrushCause)> = Vec::new();
        for rig in self.rigs.iter().filter(|r| !r.lost) {
            let cells = inp.grid.cells_in(sky(rig.rect, inp.grid.cell_px()));
            let swept = inp.swept.iter().find(|(w, c)| *w != rig.window && cells.contains(c)).map(|(w, _)| CrushCause::Swept { by: *w });
            let drag_cover = inp
                .cells
                .iter()
                .filter(|d| d.window != rig.window && matches!(d.cause, RectCause::DragStart | RectCause::DragSample | RectCause::DragEnd))
                .find(|d| d.covered.iter().any(|c| cells.contains(c)))
                .map(|d| CrushCause::DragCover { by: d.window, cause: d.cause });
            if let Some(why) = swept.or(drag_cover) {
                doomed.push((rig.id, why));
            }
        }
        for (id, why) in doomed {
            self.crush(id, why, ev);
        }
    }

    /// D45: the anchor window moved to `f`; the rig, its davits and rope tops go with it, and so do the miners on it.
    fn carry(&mut self, id: u32, f: RectPx) {
        let s = self.cfg.art_scale;
        let Some(i) = self.rigs.iter().position(|r| r.id == id) else { return };
        let (hw, h) = ((RIG_W * s / 2.0) as Px, (RIG_H * s) as Px);
        let cx = (f.l + f.r) / 2;
        let new = RectPx::new(cx - hw, f.t - h, cx + hw, f.t);
        let rig = &mut self.rigs[i];
        let (dx, dy) = ((new.l - rig.rect.l) as f32, (new.t - rig.rect.t) as f32);
        let old = rig.anchor;
        let post_in = POST_INSET * s;
        for k in 0..2 {
            if let Some((_, tip)) = rig.cranes[k] {
                // Keep each davit's reach and height; re-seat it on the window's new corner.
                let (side_old, side_new, post) = if k == 0 { (old.l, f.l, f.l as f32 + post_in) } else { (old.r, f.r, f.r as f32 - post_in) };
                let reach = tip[0] - side_old as f32;
                let lift = old.t as f32 - tip[1];
                let new_tip = [side_new as f32 + reach, f.t as f32 - lift];
                rig.cranes[k] = Some(([post, f.t as f32], new_tip));
                if let Some(r) = rig.ropes[k] {
                    self.ropes.pin_start(r, new_tip);
                    self.ropes.set_collider(r, Some([f.l as f32, f.t as f32, f.r as f32, f.b as f32]));
                }
            }
        }
        rig.rect = new;
        rig.anchor = f;
        rig.lost = false;
        for u in &mut self.units {
            if matches!(u.state, UnitState::AtRig { rig, .. } if rig == id) {
                u.pos[0] += dx;
                u.pos[1] += dy;
            }
        }
    }

    fn crush(&mut self, id: u32, by: CrushCause, ev: &mut Vec<SimEvent>) {
        let Some(i) = self.rigs.iter().position(|r| r.id == id) else { return };
        let rig = self.rigs.remove(i);
        let salvage = self.cfg.rig_cost * self.cfg.salvage_pct / 100;
        ev.push(SimEvent::StructureCrushed { id, salvage, by });
        for r in rig.ropes.into_iter().flatten() {
            self.ropes.cut(r);
            self.loose.push((r, LOOSE_ROPE_TICKS)); // it whips and falls for a moment instead of vanishing
            ev.push(SimEvent::RopeSnapped(r));
        }
        // Salvage scatters from the wreck and falls home.
        let (cx, cy) = (((rig.rect.l + rig.rect.r) / 2) as f32, ((rig.rect.t + rig.rect.b) / 2) as f32);
        for _ in 0..18 {
            let vel = [self.rng.signed() * 420.0, -120.0 - 320.0 * self.rng.signed().abs()];
            let ttl = 16 + (self.rng.next() % 16) as u32;
            self.sparks.push(Spark { pos: [cx, cy], vel, ttl });
        }
        let pieces = 6u32;
        for k in 0..pieces {
            let v = salvage / pieces + u32::from(k < salvage % pieces);
            let vel = [self.rng.signed() * 260.0, -180.0 - 160.0 * self.rng.signed().abs()];
            self.salvage.push(SalvagePiece { body: FallBody { pos: [cx, cy], vel, chute: false }, value: v });
        }
        // Everyone on the rig or its ropes bails out under a chute; their load is lost.
        for u in &mut self.units {
            let on_it = matches!(u.state, UnitState::AtRig { rig, .. } | UnitState::Climbing { rig, .. } if rig == id);
            if on_it {
                u.carrying = 0;
                u.state = UnitState::Falling(FallBody { pos: u.pos, vel: [0.0, -60.0], chute: true });
                ev.push(SimEvent::UnitFalling { unit: u.id });
            }
        }
        self.cooldown = self.cfg.rebuild_cooldown_ticks;
        self.burst_ticks = 40;
    }

    /// Covered by anything that isn't a drag: buried, paused, hidden. Bare again: dug out.
    fn bury(&mut self, grid: &OcclusionGrid, ev: &mut Vec<SimEvent>) {
        for rig in &mut self.rigs {
            let covered = rig.lost || !grid.all_bare(sky(rig.rect, grid.cell_px()));
            if covered != rig.buried {
                rig.buried = covered;
                ev.push(if covered { SimEvent::StructureBuried { id: rig.id } } else { SimEvent::StructureUnburied { id: rig.id } });
            }
        }
    }

    fn maybe_build(&mut self, inp: &FrameInputs, ev: &mut Vec<SimEvent>) {
        let now = inp.target.map(|t| (t.id, t.frame));
        if now.is_some() && now == self.last_target {
            self.settled_ticks = self.settled_ticks.saturating_add(1);
        } else {
            self.settled_ticks = 0;
            self.last_target = now;
        }
        self.build_block = self.try_build(inp, ev).err();
    }

    fn try_build(&mut self, inp: &FrameInputs, ev: &mut Vec<SimEvent>) -> Result<(), BuildBlock> {
        let t = inp.target.ok_or(BuildBlock::NoTarget)?;
        if t.maximized {
            return Err(BuildBlock::Maximized);
        }
        if self.rigs.iter().any(|r| r.window == t.id) {
            return Err(BuildBlock::AlreadyRigged);
        }
        if self.cooldown > 0 {
            return Err(BuildBlock::Cooldown);
        }
        if self.rigs.len() >= self.cfg.max_rigs {
            return Err(BuildBlock::RigCap);
        }
        // With no rig standing there's no income, so the next rig takes whatever scrap is left (never a dead end).
        let cost = if self.rigs.is_empty() { self.cfg.rig_cost.min(self.ledger.scrap) } else { self.cfg.rig_cost };
        if self.ledger.scrap < cost {
            return Err(BuildBlock::NoScrap);
        }
        let s = self.cfg.art_scale;
        let f = t.frame;
        let cx = (f.l + f.r) / 2;
        let (hw, h, above) = ((RIG_W * s / 2.0) as Px, (RIG_H * s) as Px, (BARE_ABOVE * s) as Px);
        let rect = RectPx::new(cx - hw, f.t - h, cx + hw, f.t);
        // The whole 88 px of sky must be on a monitor (all_bare alone would clip it to the screen edge) and bare.
        // ...and on the window's own monitor: sky that spills onto the monitor above isn't open desktop above this window.
        let own = inp.bedrock.iter().map(|(r, _)| *r).find(|r| r.contains(cx, f.t));
        let sky_on_screen = own.is_some_and(|r| f.t - above >= r.t)
            && inp.grid.cell_at(cx - hw, f.t - above).is_some()
            && inp.grid.cell_at(cx + hw - 1, f.t - above).is_some();
        if !sky_on_screen {
            return Err(BuildBlock::EdgeTooHigh);
        }
        if !inp.grid.all_bare(sky(RectPx::new(cx - hw, f.t - above, cx + hw, f.t), inp.grid.cell_px())) {
            return Err(BuildBlock::SkyCovered); // wall-run/band territory, not frontier
        }
        // The window has to sit still for a second: a rig built mid-drag snaps on the next nudge.
        if self.settled_ticks < SETTLE_TICKS {
            return Err(BuildBlock::Settling);
        }
        // At least a cell clear of the frame, or the rope's own column would be the window's.
        let gap = ((ROPE_GAP * s) as Px).max(inp.grid.cell_px() + 2);
        let jib = (JIB_H * s) as Px;
        let post_in = (POST_INSET * s) as Px;
        let mut ropes = [None, None];
        let mut cranes = [None, None];
        for (k, (x, post)) in [(f.l - gap, f.l + post_in), (f.r + gap, f.r - post_in)].into_iter().enumerate() {
            // A davit: a post on the window's top corner, a jib reaching out over the side, the rope hanging from its tip.
            let side = RectPx::new(x - 2, f.t - jib, x + 2, f.b);
            let arm = sky(RectPx::new(x.min(post) - 2, f.t - jib, x.max(post) + 2, f.t), inp.grid.cell_px());
            if inp.grid.all_bare(side) && inp.grid.all_bare(arm) {
                let top = (f.t - jib) as f32;
                let segs = (((f.b as f32 - top) / 40.0).ceil() as u8).clamp(4, 40);
                // Spawned slightly slack and off-vertical so it settles with a visible swing.
                let out = if k == 0 { -30.0 } else { 30.0 }; // start swung outward, away from the frame
                let rope = self.ropes.spawn([x as f32, top], [x as f32 + out, f.b as f32], segs);
                // The window is solid to its own ropes: they bounce off the frame, never swing behind it.
                self.ropes.set_collider(rope, Some([f.l as f32, f.t as f32, f.r as f32, f.b as f32]));
                ropes[k] = Some(rope);
                cranes[k] = Some(([post as f32, f.t as f32], [x as f32, top]));
            }
        }
        self.ledger.scrap -= cost;
        let id = self.id();
        self.rigs.push(Rig { id, window: t.id, rect, buried: false, ropes, cranes, anchor: f, lost: false });
        ev.push(SimEvent::RigDeployed { id });
        let want = self.cfg.miners_per_rig;
        for k in 0..want {
            let uid = self.id();
            let x = rect.l as f32 + (k as f32 + 0.5) * (rect.width() as f32 / want as f32);
            self.units.push(Unit { id: uid, pos: [x, rect.b as f32], carrying: 0, state: UnitState::AtRig { rig: id, t: (k as u32) * 9 } });
        }
        Ok(())
    }

    fn units_step(&mut self, inp: &FrameInputs, ev: &mut Vec<SimEvent>) {
        let platforms: Vec<Platform> = self.platforms(inp.grid);
        let cfg = self.cfg.clone();
        let rigs = self.rigs.clone();
        let wr = self.wallrun;
        let floor = inp.shaft.map(|sh| sh.b as f32);
        let mut delivered = 0u32;
        let mut hatches: Vec<f32> = Vec::new();
        for u in &mut self.units {
            match u.state.clone() {
                UnitState::AtRig { rig, t } => {
                    let Some(r) = rigs.iter().find(|r| r.id == rig) else {
                        u.state = UnitState::Falling(FallBody { pos: u.pos, vel: [0.0, 0.0], chute: true });
                        continue;
                    };
                    if r.buried {
                        continue;
                    }
                    if t + 1 < cfg.mine_ticks {
                        u.state = UnitState::AtRig { rig, t: t + 1 };
                        continue;
                    }
                    u.carrying = cfg.scrap_per_load;
                    let side = (u.id as usize) % 2;
                    let side = if r.ropes[side].is_some() { Some(side) } else if r.ropes[1 - side].is_some() { Some(1 - side) } else { None };
                    match side {
                        Some(side) => u.state = UnitState::Climbing { rig, side, s: 0.0 },
                        None => {
                            u.state = UnitState::Falling(FallBody { pos: [r.rect.l as f32 - 6.0, r.rect.b as f32], vel: [-40.0, -120.0], chute: true });
                            ev.push(SimEvent::UnitFalling { unit: u.id });
                        }
                    }
                }
                UnitState::Climbing { rig, side, s } => {
                    let Some(rope) = rigs.iter().find(|r| r.id == rig).and_then(|r| r.ropes[side]) else {
                        u.state = UnitState::Falling(FallBody { pos: u.pos, vel: [0.0, 0.0], chute: true });
                        continue;
                    };
                    let nodes = self.ropes.nodes(rope);
                    let len = self.ropes.length(rope).max(1.0);
                    let s2 = s + CLIMB_PX_PER_TICK * cfg.art_scale / len;
                    u.pos = rope_point(nodes, s2);
                    if s2 >= 1.0 {
                        u.state = UnitState::Falling(FallBody { pos: u.pos, vel: [0.0, 0.0], chute: true });
                        ev.push(SimEvent::UnitFalling { unit: u.id });
                    } else {
                        u.state = UnitState::Climbing { rig, side, s: s2 };
                    }
                }
                UnitState::Falling(mut b) => {
                    let floor = floor_at(inp.bedrock, b.pos[0]);
                    match fall_step(&mut b, DT, &platforms, floor) {
                        Landing::StillFalling => u.state = UnitState::Falling(b),
                        Landing::Platform(w) => {
                            ev.push(SimEvent::UnitLanded { unit: u.id, on: LandingKind::Platform });
                            let p = platforms.iter().filter(|p| p.window == w && p.y as f32 == b.pos[1]).find(|p| b.pos[0] >= p.x0 as f32 && b.pos[0] < p.x1 as f32);
                            match p {
                                Some(p) => {
                                    let dir = if b.pos[0] - p.x0 as f32 <= p.x1 as f32 - b.pos[0] { -1.0 } else { 1.0 };
                                    u.state = UnitState::Walking { y: p.y, x0: p.x0, x1: p.x1, dir };
                                }
                                None => u.state = UnitState::Falling(FallBody { pos: [b.pos[0], b.pos[1] + 1.0], vel: [0.0, 0.0], chute: true }),
                            }
                            u.pos = b.pos;
                            continue;
                        }
                        Landing::Bedrock => {
                            ev.push(SimEvent::UnitLanded { unit: u.id, on: LandingKind::Bedrock });
                            delivered += u.carrying;
                            if u.carrying > 0 {
                                hatches.push(b.pos[0]);
                            }
                            u.carrying = 0;
                            u.state = UnitState::Home { t: 0 };
                        }
                    }
                    u.pos = b.pos;
                }
                UnitState::Walking { y, x0, x1, dir } => {
                    // Off the end of the platform, or the platform went away under them: step off and fall.
                    let still = platforms.iter().any(|p| p.y == y && u.pos[0] >= p.x0 as f32 && u.pos[0] < p.x1 as f32);
                    u.pos[0] += dir * WALK_PX_PER_TICK * cfg.art_scale;
                    if !still || u.pos[0] < x0 as f32 || u.pos[0] >= x1 as f32 {
                        u.pos[0] += dir * 6.0;
                        u.state = UnitState::Falling(FallBody { pos: [u.pos[0], y as f32 + 1.0], vel: [dir * 40.0, 0.0], chute: true });
                        ev.push(SimEvent::UnitFalling { unit: u.id });
                    } else {
                        u.state = UnitState::Walking { y, x0, x1, dir };
                    }
                }
                UnitState::WallClimb { y } => match wr {
                    Some(w) => {
                        let y2 = y - WALL_CLIMB_PX_PER_TICK * cfg.art_scale;
                        u.pos = [w.wall_x, y2.max(w.hook[1])];
                        u.state = if y2 <= w.hook[1] + 6.0 { UnitState::WallMine { t: 0 } } else { UnitState::WallClimb { y: y2 } };
                    }
                    None => u.state = UnitState::WallSlide { y: u.pos[1] },
                },
                UnitState::WallMine { t } => match wr {
                    Some(_) if t + 1 < cfg.mine_ticks => u.state = UnitState::WallMine { t: t + 1 },
                    Some(_) => {
                        u.carrying = cfg.scrap_per_load;
                        u.state = UnitState::WallSlide { y: u.pos[1] };
                    }
                    None => u.state = UnitState::WallSlide { y: u.pos[1] },
                },
                UnitState::WallSlide { y } => {
                    let bottom = floor.unwrap_or(y);
                    let y2 = y + WALL_SLIDE_PX_PER_TICK * cfg.art_scale;
                    u.pos[1] = y2.min(bottom);
                    if y2 >= bottom {
                        if u.carrying > 0 {
                            hatches.push(u.pos[0]);
                        }
                        delivered += u.carrying;
                        u.carrying = 0;
                        u.state = match wr {
                            Some(_) => UnitState::WallClimb { y: bottom },
                            None => UnitState::Home { t: 0 },
                        };
                    } else {
                        u.state = UnitState::WallSlide { y: y2 };
                    }
                }
                UnitState::Home { t } => {
                    if t + 1 < cfg.home_ticks {
                        u.state = UnitState::Home { t: t + 1 };
                        continue;
                    }
                    // The elevator: back up to an unburied rig, if there is one.
                    if let Some(r) = rigs.iter().filter(|r| !r.buried).min_by_key(|r| r.id) {
                        u.pos = [((r.rect.l + r.rect.r) / 2) as f32, r.rect.b as f32];
                        u.state = UnitState::AtRig { rig: r.id, t: 0 };
                    } else {
                        u.state = UnitState::Home { t };
                    }
                }
            }
        }
        self.hatches.extend(hatches.into_iter().map(|x| (x, HATCH_TICKS)));
        if delivered > 0 {
            self.ledger.scrap += delivered;
            self.ledger.delivered_total += delivered;
            ev.push(SimEvent::CargoDelivered { kind: Cargo::Scrap, qty: delivered });
        }
        // Homeless units with no rig left to return to retire once there are too many of them.
        let cap = self.cfg.miners_per_rig * self.rigs.len().max(1);
        let mut home = 0usize;
        self.units.retain(|u| {
            if matches!(u.state, UnitState::Home { .. }) {
                home += 1;
                home <= cap
            } else {
                true
            }
        });
    }

    /// §2.3 wall-run: when the target is maximized or flush against SHAFT and frontier mode can't place a rig, the crew
    /// climbs SHAFT's inner wall, hooks the window's top-left corner, mines it and slides home.
    fn wallrun_step(&mut self, inp: &FrameInputs, ev: &mut Vec<SimEvent>) {
        if let Some(w) = self.wallrun {
            let snapped = inp.moved.iter().any(|(id, _, _)| *id == w.window);
            let released = !inp.target.is_some_and(|t| t.id == w.window);
            if snapped || released {
                self.wallrun = None;
                if snapped {
                    for u in &mut self.units {
                        if matches!(u.state, UnitState::WallClimb { .. } | UnitState::WallMine { .. } | UnitState::WallSlide { .. }) {
                            u.carrying = 0;
                            u.state = UnitState::Falling(FallBody { pos: u.pos, vel: [0.0, -40.0], chute: true });
                            ev.push(SimEvent::UnitFalling { unit: u.id });
                        }
                    }
                    self.burst_ticks = 30;
                    self.cooldown = self.cfg.rebuild_cooldown_ticks;
                    ev.push(SimEvent::WallRunSnapped);
                } else {
                    ev.push(SimEvent::WallRunReleased);
                }
            }
        }
        if self.wallrun.is_some() {
            return;
        }
        let (Some(t), Some(sh)) = (inp.target, inp.shaft) else { return };
        let frontier_no = matches!(self.build_block, Some(BuildBlock::Maximized | BuildBlock::EdgeTooHigh | BuildBlock::SkyCovered));
        let flush = (t.frame.l - sh.r).abs() <= 8 && t.frame.t < sh.b - 40 && t.frame.b > sh.t;
        // Same pacing as a rig: no re-hook straight after a snap, and only on a window that has sat still a second.
        if !frontier_no || !flush || self.cooldown > 0 || self.settled_ticks < SETTLE_TICKS || self.rigs.iter().any(|r| r.window == t.id) {
            return;
        }
        let s = self.cfg.art_scale;
        let w = WallRun { window: t.id, hook: [sh.r as f32, t.frame.t.max(sh.t) as f32 + 6.0 * s], wall_x: sh.r as f32 - 14.0 * s };
        self.wallrun = Some(w);
        ev.push(SimEvent::WallRunStarted { window: t.id });
        let crew = self.units.iter().filter(|u| matches!(u.state, UnitState::WallClimb { .. } | UnitState::WallMine { .. } | UnitState::WallSlide { .. })).count();
        for k in crew..self.cfg.miners_per_rig {
            let uid = self.id();
            // Staggered: each starts a little further down so they don't climb as one block.
            let y = sh.b as f32 + k as f32 * 70.0 * s;
            self.units.push(Unit { id: uid, pos: [w.wall_x, sh.b as f32], carrying: 0, state: UnitState::WallClimb { y } });
        }
    }

    /// Shaft lite: the drill burns fuel and bores up; every `CORE_SECS` of drilling a core drops down the shaft.
    fn drill_step(&mut self, inp: &FrameInputs, ev: &mut Vec<SimEvent>) {
        let Some(sh) = inp.shaft else { return };
        self.drilling = self.ledger.fuel > 0;
        if self.drilling {
            self.burn_acc += DT;
            if self.burn_acc >= FUEL_BURN_SECS {
                self.burn_acc -= FUEL_BURN_SECS;
                self.ledger.fuel -= 1;
                self.depth += 1;
            }
            self.core_acc += DT;
            if self.core_acc >= CORE_SECS {
                self.core_acc -= CORE_SECS;
                let cx = (sh.l + sh.r) as f32 / 2.0;
                self.cores_falling.push(FallBody { pos: [cx, sh.t as f32 + 70.0 * self.cfg.art_scale], vel: [0.0, 0.0], chute: false });
            }
        }
        let mut landed = 0u32;
        self.cores_falling.retain_mut(|c| match fall_step(c, DT, &[], sh.b) {
            Landing::StillFalling => true,
            _ => {
                landed += 1;
                false
            }
        });
        for _ in 0..landed {
            self.ledger.cores += 1;
            self.ledger.scrap += CORE_SCRAP;
            self.hatches.push(((sh.l + sh.r) as f32 / 2.0, HATCH_TICKS * 2));
            self.burst_ticks = 30;
            ev.push(SimEvent::BossCore { tier: (self.depth / 50).min(255) as u8 });
        }
    }

    fn fx_step(&mut self) {
        for sp in &mut self.sparks {
            sp.vel[1] += bezel_physics::GRAVITY * DT;
            sp.pos[0] += sp.vel[0] * DT;
            sp.pos[1] += sp.vel[1] * DT;
            sp.ttl = sp.ttl.saturating_sub(1);
        }
        self.sparks.retain(|sp| sp.ttl > 0);
        let mut done = Vec::new();
        for (r, ttl) in &mut self.loose {
            *ttl = ttl.saturating_sub(1);
            if *ttl == 0 {
                done.push(*r);
            }
        }
        for r in done {
            self.ropes.retire(r);
        }
        self.loose.retain(|(_, ttl)| *ttl > 0);
        for h in &mut self.hatches {
            h.1 = h.1.saturating_sub(1);
        }
        self.hatches.retain(|h| h.1 > 0);
    }

    fn salvage_step(&mut self, inp: &FrameInputs, ev: &mut Vec<SimEvent>) {
        let mut got = 0u32;
        self.salvage.retain_mut(|p| {
            let floor = floor_at(inp.bedrock, p.body.pos[0]);
            // Salvage falls straight home: platforms don't catch it.
            match fall_step(&mut p.body, DT, &[], floor) {
                Landing::StillFalling => true,
                _ => {
                    got += p.value;
                    false
                }
            }
        });
        if got > 0 {
            self.ledger.scrap += got;
            self.ledger.salvage_total += got;
            ev.push(SimEvent::CargoDelivered { kind: Cargo::Salvage, qty: got });
        }
    }

    fn platforms(&self, grid: &OcclusionGrid) -> Vec<Platform> {
        let mut out = Vec::new();
        let mut m = 0u64;
        while grid.dims(bezel_core::MonitorId(m)).is_some() {
            out.extend(grid.platforms(bezel_core::MonitorId(m)));
            m += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bezel_core::{MonitorId, MonitorInfo};

    const M: MonitorId = MonitorId(0);

    fn world() -> (OcclusionGrid, Vec<(RectPx, Px)>) {
        let b = RectPx::new(0, 0, 1920, 1080);
        let mons = [MonitorInfo { id: M, bounds: b, work: b, dpi: 96, primary: true }];
        (OcclusionGrid::new(&mons, 16, 1), vec![(b, 1000)])
    }

    fn target(w: u64, r: RectPx) -> TargetWindow {
        TargetWindow { id: ExtWindowId(w), frame: r, monitor: M, maximized: false }
    }

    struct Run<'a> {
        sim: Sim,
        grid: &'a OcclusionGrid,
        bed: &'a [(RectPx, Px)],
    }

    impl Run<'_> {
        fn step(&mut self, t: Option<TargetWindow>) -> Vec<SimEvent> {
            self.step_with(t, PulseBatch::default(), &[], &[], &[])
        }
        /// Hold the target still through the settle second, so a rig can go up.
        fn build(&mut self, t: Option<TargetWindow>) -> Vec<SimEvent> {
            (0..=SETTLE_TICKS).flat_map(|_| self.step(t)).collect()
        }
        fn step_with(&mut self, t: Option<TargetWindow>, p: PulseBatch, cells: &[CellDelta], swept: &[(ExtWindowId, CellId)], moved: &[(ExtWindowId, Option<RectPx>, RectCause)]) -> Vec<SimEvent> {
            self.sim.step(&FrameInputs { pulses: p, cells, swept, moved, target: t, grid: self.grid, bedrock: self.bed, shaft: None })
        }
    }

    #[test]
    fn typing_heats_the_foundry_and_smelts_fuel() {
        let (grid, bed) = world();
        let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
        let typing = PulseBatch { key_events: 4, ..Default::default() };
        for _ in 0..100 {
            r.step_with(None, typing, &[], &[], &[]);
        }
        assert!(r.sim.heat() > 0.1, "heat {}", r.sim.heat());
        assert!(r.sim.ledger().fuel > 0 && r.sim.ledger().scrap < 60);
        let fuel = r.sim.ledger().fuel;
        for _ in 0..400 {
            r.step(None); // idle: heat decays below the threshold, smelting stops
        }
        let f2 = r.sim.ledger().fuel;
        for _ in 0..100 {
            r.step(None);
        }
        assert_eq!(r.sim.ledger().fuel, f2, "a cold foundry doesn't smelt");
        assert!(f2 >= fuel);
    }

    #[test]
    fn rig_goes_up_only_with_open_sky_above() {
        let (mut grid, bed) = world();
        let w = RectPx::new(400, 300, 1200, 800);
        grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
        let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
        let ev = r.build(Some(target(7, w)));
        assert!(ev.iter().any(|e| matches!(e, SimEvent::RigDeployed { .. })));
        assert_eq!(r.sim.rigs().len(), 1);
        assert_eq!(r.sim.rigs()[0].ropes.iter().flatten().count(), 2, "both sides are open");
        assert_eq!(r.sim.rigs()[0].cranes.iter().flatten().count(), 2, "every rope hangs from a davit");
        let (base, tip) = r.sim.rigs()[0].cranes[0].unwrap();
        assert!(tip[1] < base[1] && tip[0] < base[0], "left davit reaches up and out over the left side");
        assert_eq!(r.sim.ledger().scrap, 30);

        let (mut g2, bed2) = world();
        let top = RectPx::new(400, 40, 1200, 800); // only 40 px above
        g2.apply(ExtWindowId(8), Some(top), RectCause::Shown);
        let mut r2 = Run { sim: Sim::new(SimConfig::default(), 1), grid: &g2, bed: &bed2 };
        r2.build(Some(target(8, top)));
        assert!(r2.sim.rigs().is_empty());
    }

    #[test]
    fn sky_must_be_on_the_windows_own_monitor() {
        let top = RectPx::new(0, -1080, 1920, 0);
        let main = RectPx::new(0, 0, 1920, 1080);
        let mons = [
            MonitorInfo { id: MonitorId(0), bounds: main, work: main, dpi: 96, primary: true },
            MonitorInfo { id: MonitorId(1), bounds: top, work: top, dpi: 96, primary: false },
        ];
        let mut grid = OcclusionGrid::new(&mons, 16, 1);
        let w = RectPx::new(400, 32, 1200, 800); // title bar 32 px under the seam; the monitor above is bare
        grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
        let bed = vec![(main, 1000), (top, -100)];
        let mut sim = Sim::new(SimConfig::default(), 1);
        sim.step(&FrameInputs { pulses: PulseBatch::default(), cells: &[], swept: &[], moved: &[], target: Some(target(7, w)), grid: &grid, bedrock: &bed, shaft: None });
        assert!(sim.rigs().is_empty());
        assert_eq!(sim.build_block(), Some(BuildBlock::EdgeTooHigh));
    }

    #[test]
    fn miners_bring_scrap_home() {
        let (mut grid, bed) = world();
        let w = RectPx::new(400, 300, 1200, 800);
        grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
        let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
        let mut landed_on_bedrock = false;
        for _ in 0..1200 {
            for e in r.step(Some(target(7, w))) {
                landed_on_bedrock |= matches!(e, SimEvent::UnitLanded { on: LandingKind::Bedrock, .. });
            }
        }
        assert!(landed_on_bedrock);
        assert!(r.sim.ledger().delivered_total > 0, "scrap delivered: {:?}", r.sim.ledger());
    }

    #[test]
    fn units_land_on_a_window_platform() {
        let (mut grid, bed) = world();
        let w = RectPx::new(400, 300, 900, 500);
        let below = RectPx::new(100, 700, 1500, 900); // a wide window under the rope ends
        grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
        grid.apply(ExtWindowId(9), Some(below), RectCause::Shown);
        let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
        let mut on_platform = false;
        for _ in 0..800 {
            for e in r.step(Some(target(7, w))) {
                on_platform |= matches!(e, SimEvent::UnitLanded { on: LandingKind::Platform, .. });
            }
        }
        assert!(on_platform, "someone should land on window 9's top edge");
    }

    #[test]
    fn a_drag_sweep_crushes_and_salvage_comes_home() {
        let (mut grid, bed) = world();
        let w = RectPx::new(400, 300, 1200, 800);
        grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
        let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
        r.build(Some(target(7, w)));
        let rig = r.sim.rigs()[0].clone();
        let scrap_before = r.sim.ledger().scrap;
        // Another window is dragged right across the rig.
        let swept = grid.sweep(RectPx::new(0, 200, 300, 280), RectPx::new(1500, 200, 1800, 280));
        assert!(swept.iter().any(|c| grid.cells_in(rig.rect).contains(c)));
        let swept: Vec<(ExtWindowId, CellId)> = swept.into_iter().map(|c| (ExtWindowId(99), c)).collect();
        let ev = r.step_with(Some(target(7, w)), PulseBatch::default(), &[], &swept, &[]);
        let crushed = ev.iter().find_map(|e| if let SimEvent::StructureCrushed { salvage, .. } = e { Some(*salvage) } else { None });
        assert_eq!(crushed, Some(18), "60% of 30");
        assert!(r.sim.rigs().is_empty());
        assert!(ev.iter().any(|e| matches!(e, SimEvent::RopeSnapped(_))));
        for _ in 0..200 {
            r.step(None);
        }
        assert_eq!(r.sim.ledger().salvage_total, 18);
        assert!(r.sim.ledger().scrap >= scrap_before + 18);
    }

    #[test]
    fn covering_buries_and_uncovering_unburies() {
        let (mut grid, bed) = world();
        let w = RectPx::new(400, 300, 1200, 800);
        grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
        let rig_rect;
        {
            let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
            r.build(Some(target(7, w)));
            rig_rect = r.sim.rigs()[0].rect;
        }
        let mut sim = {
            let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
            r.build(Some(target(7, w)));
            r.sim
        };
        // A window opens over the rig (not a drag): buried, not crushed.
        let d = grid.apply(ExtWindowId(11), Some(RectPx::new(rig_rect.l - 50, 0, rig_rect.r + 50, 290)), RectCause::Shown);
        let ev = sim.step(&FrameInputs { pulses: PulseBatch::default(), cells: &[d], swept: &[], moved: &[], target: None, grid: &grid, bedrock: &bed, shaft: None });
        assert!(ev.iter().any(|e| matches!(e, SimEvent::StructureBuried { .. })));
        assert_eq!(sim.rigs().len(), 1, "burying isn't crushing");
        let d = grid.apply(ExtWindowId(11), None, RectCause::Destroyed);
        let ev = sim.step(&FrameInputs { pulses: PulseBatch::default(), cells: &[d], swept: &[], moved: &[], target: None, grid: &grid, bedrock: &bed, shaft: None });
        assert!(ev.iter().any(|e| matches!(e, SimEvent::StructureUnburied { .. })));
    }

    #[test]
    fn a_rig_rides_its_own_window() {
        let (mut grid, bed) = world();
        let w = RectPx::new(400, 300, 1200, 800);
        grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
        let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
        r.build(Some(target(7, w)));
        let before = r.sim.rigs()[0].clone();
        let w2 = RectPx::new(600, 400, 1400, 900); // dragged 200 right, 100 down
        // Its own drag sweeps right over where the rig was: that must not crush it (D45).
        let own: Vec<(ExtWindowId, CellId)> = grid.sweep(w, w2).into_iter().map(|c| (ExtWindowId(7), c)).collect();
        let ev = r.step_with(Some(target(7, w2)), PulseBatch::default(), &[], &own, &[(ExtWindowId(7), Some(w2), RectCause::DragSample)]);
        assert!(!ev.iter().any(|e| matches!(e, SimEvent::StructureCrushed { .. })), "{ev:?}");
        let after = r.sim.rigs()[0].clone();
        assert_eq!((after.rect.l - before.rect.l, after.rect.t - before.rect.t), (200, 100));
        let (b0, t0) = before.cranes[0].unwrap();
        let (b1, t1) = after.cranes[0].unwrap();
        assert_eq!((b1[0] - b0[0], b1[1] - b0[1], t1[0] - t0[0], t1[1] - t0[1]), (200.0, 100.0, 200.0, 100.0));
        for _ in 0..40 {
            r.step(Some(target(7, w2)));
        }
        let top = r.sim.rope_nodes(after.ropes[0].unwrap())[0];
        assert_eq!(top, t1, "the rope's top follows the davit");
    }

    #[test]
    fn closing_the_anchor_collapses_the_rig_and_minimizing_buries_it() {
        let (mut grid, bed) = world();
        let w = RectPx::new(400, 300, 1200, 800);
        grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
        let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
        r.build(Some(target(7, w)));
        let ev = r.step_with(None, PulseBatch::default(), &[], &[], &[(ExtWindowId(7), None, RectCause::Minimized)]);
        assert!(ev.iter().any(|e| matches!(e, SimEvent::StructureBuried { .. })));
        let ev = r.step_with(None, PulseBatch::default(), &[], &[], &[(ExtWindowId(7), Some(w), RectCause::Restored)]);
        assert!(ev.iter().any(|e| matches!(e, SimEvent::StructureUnburied { .. })));
        let ev = r.step_with(None, PulseBatch::default(), &[], &[], &[(ExtWindowId(7), None, RectCause::Destroyed)]);
        assert!(ev.iter().any(|e| matches!(e, SimEvent::StructureCrushed { salvage: 18, .. })));
        assert!(r.sim.rigs().is_empty());
    }

    #[test]
    fn broke_with_no_rig_still_builds_one() {
        let (mut grid, bed) = world();
        let w = RectPx::new(400, 300, 1200, 800);
        grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
        let mut r = Run { sim: Sim::new(SimConfig { start_scrap: 5, ..SimConfig::default() }, 1), grid: &grid, bed: &bed };
        let ev = r.build(Some(target(7, w)));
        assert!(ev.iter().any(|e| matches!(e, SimEvent::RigDeployed { .. })), "no dead end at 5 scrap");
        assert_eq!(r.sim.ledger().scrap, 0);
    }

    #[test]
    fn the_foundry_keeps_a_rig_in_reserve() {
        let (grid, bed) = world();
        let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
        let typing = PulseBatch { key_events: 20, ..Default::default() };
        for _ in 0..2000 {
            r.step_with(None, typing, &[], &[], &[]);
        }
        assert_eq!(r.sim.ledger().scrap, 30, "smelts down to one rig's cost, no further");
        assert_eq!(r.sim.ledger().fuel, 30);
    }

    /// Home monitor 2560x1440 with SHAFT at x 0..144 and the trench top at y 1288, like THOR.
    fn bedrock_world() -> (OcclusionGrid, Vec<(RectPx, Px)>, RectPx) {
        let b = RectPx::new(0, 0, 2560, 1440);
        let mons = [MonitorInfo { id: M, bounds: b, work: RectPx::new(144, 0, 2560, 1288), dpi: 96, primary: true }];
        (OcclusionGrid::new(&mons, 16, 1), vec![(b, 1288)], RectPx::new(0, 0, 144, 1288))
    }

    fn step_shaft(sim: &mut Sim, grid: &OcclusionGrid, bed: &[(RectPx, Px)], shaft: RectPx, t: Option<TargetWindow>, moved: &[(ExtWindowId, Option<RectPx>, RectCause)]) -> Vec<SimEvent> {
        sim.step(&FrameInputs { pulses: PulseBatch::default(), cells: &[], swept: &[], moved, target: t, grid, bedrock: bed, shaft: Some(shaft) })
    }

    #[test]
    fn maximized_window_gets_a_wall_run_that_brings_scrap_home() {
        let (mut grid, bed, shaft) = bedrock_world();
        let w = RectPx::new(144, 0, 2560, 1288);
        grid.apply(ExtWindowId(5), Some(w), RectCause::Shown);
        let t = TargetWindow { id: ExtWindowId(5), frame: w, monitor: M, maximized: true };
        let mut sim = Sim::new(SimConfig::default(), 1);
        let mut started = false;
        for _ in 0..1200 {
            for e in step_shaft(&mut sim, &grid, &bed, shaft, Some(t), &[]) {
                started |= matches!(e, SimEvent::WallRunStarted { .. });
            }
        }
        assert!(started);
        assert!(sim.rigs().is_empty(), "no frontier rig on a maximized window");
        assert!(sim.ledger().delivered_total > 0, "the wall crew hauled scrap: {:?}", sim.ledger());
        let wr = sim.wallrun().expect("still hooked");
        assert_eq!(wr.hook[0], 144.0, "hooked at the window's top-left corner, at SHAFT's inner edge");
        assert!(sim.units().iter().all(|u| u.pos[0] < 144.0), "the crew stays inside SHAFT");
    }

    #[test]
    fn moving_the_hooked_window_snaps_the_wall_run() {
        let (mut grid, bed, shaft) = bedrock_world();
        let w = RectPx::new(144, 0, 1300, 1288); // snapped left
        grid.apply(ExtWindowId(5), Some(w), RectCause::Shown);
        let t = TargetWindow { id: ExtWindowId(5), frame: w, monitor: M, maximized: false };
        let mut sim = Sim::new(SimConfig::default(), 1);
        for _ in 0..120 {
            step_shaft(&mut sim, &grid, &bed, shaft, Some(t), &[]);
        }
        assert!(sim.wallrun().is_some());
        let ev = step_shaft(&mut sim, &grid, &bed, shaft, Some(t), &[(ExtWindowId(5), Some(w), RectCause::DragStart)]);
        assert!(ev.iter().any(|e| matches!(e, SimEvent::WallRunSnapped)));
        assert!(sim.units().iter().all(|u| !matches!(u.state, UnitState::WallClimb { .. } | UnitState::WallMine { .. })));
    }

    #[test]
    fn the_drill_burns_fuel_and_drops_a_core() {
        let (grid, bed, shaft) = bedrock_world();
        let mut sim = Sim::new(SimConfig::default(), 1);
        sim.ledger.fuel = 40;
        let mut core = false;
        for _ in 0..(SIM_HZ as usize * 50) {
            for e in step_shaft(&mut sim, &grid, &bed, shaft, None, &[]) {
                core |= matches!(e, SimEvent::BossCore { .. });
            }
        }
        assert!(core, "a core lands after 40 s of drilling");
        assert_eq!(sim.ledger().cores, 1);
        assert!(sim.depth() >= 26 && sim.ledger().fuel < 40);
    }

    #[test]
    fn a_crush_leaves_whipping_ropes_and_sparks_that_fade() {
        let (mut grid, bed) = world();
        let w = RectPx::new(400, 300, 1200, 800);
        grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
        let mut r = Run { sim: Sim::new(SimConfig::default(), 1), grid: &grid, bed: &bed };
        r.build(Some(target(7, w)));
        r.step_with(None, PulseBatch::default(), &[], &[], &[(ExtWindowId(7), None, RectCause::Destroyed)]);
        assert_eq!(r.sim.loose_ropes().len(), 2);
        assert!(!r.sim.sparks().is_empty());
        for _ in 0..80 {
            r.step(None);
        }
        assert!(r.sim.loose_ropes().is_empty() && r.sim.sparks().is_empty(), "effects fade");
    }

    #[test]
    fn deterministic_per_seed() {
        let run = |seed: u64| {
            let (mut grid, bed) = world();
            let w = RectPx::new(400, 300, 1200, 800);
            grid.apply(ExtWindowId(7), Some(w), RectCause::Shown);
            let mut r = Run { sim: Sim::new(SimConfig::default(), seed), grid: &grid, bed: &bed };
            for i in 0..600 {
                let moved: Vec<(ExtWindowId, Option<RectPx>, RectCause)> = if i == 300 { vec![(ExtWindowId(7), Some(RectPx::new(420, 310, 1220, 810)), RectCause::Jumped)] } else { vec![] };
                r.step_with(Some(target(7, w)), PulseBatch { key_events: (i % 3) as u16, ..Default::default() }, &[], &[], &moved);
            }
            r.sim
        };
        assert_eq!(run(42), run(42));
    }
}
