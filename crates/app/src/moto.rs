//! Moto spike (2026-11-12, throwaway until Knox calls it): dirtbike riders run a course across every monitor.
//! Track = each monitor's floor (TRENCH on the home monitor) + the top edges of real windows. Window-top ends get
//! kickers. Riders launch, throw tricks sized to their predicted airtime, sometimes over-rotate and crash, and eat it
//! if the window they're riding moves. Typing revs the engines (pulse counts only, D5). `bezel-app --moto`.

use std::collections::HashMap;

use bezel_core::{ExtWindowId, MonitorInfo, RectPx, SurfaceId};
use bezel_render_api::{DrawCmd, DrawList, Rgba8};

use crate::atlas::Sprites;

pub const HZ: f32 = 60.0;
const DT: f32 = 1.0 / HZ;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SegKey {
    Floor(u64),
    Win(ExtWindowId),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    pub key: SegKey,
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    /// A window top: the crew can build a kicker on it. Floors drop off flat.
    pub kick: bool,
    /// The window's bottom edge (spiders hang from it); equals `y` for floors.
    pub bottom: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bike,
    /// Runs the course on foot: pops off every edge (no kicker needed), flips, and smoke-bombs out on a crash.
    Ninja,
    /// Rope-wrist man: runs and jumps like the ninja, and in the air fires his wrist grapple into the bottom edge
    /// of a window ahead, swings under it, and lets go at the top of the swing.
    Grapple,
}

pub const NINJA_TRICKS: [Trick; 4] = [
    Trick { name: "TUCK", frames: &["ninja_tuck"], secs: 0.35, points: 120 },
    Trick { name: "FLYING KICK", frames: &["ninja_kick"], secs: 0.45, points: 200 },
    Trick { name: "FRONT FLIP", frames: &["ninja_flip_0", "ninja_flip_1", "ninja_flip_2"], secs: 0.55, points: 350 },
    Trick { name: "DOUBLE FLIP", frames: &["ninja_flip_0", "ninja_flip_1", "ninja_flip_2", "ninja_flip_0", "ninja_flip_1", "ninja_flip_2"], secs: 0.95, points: 800 },
];

fn tricks(k: Kind) -> &'static [Trick] {
    match k {
        Kind::Bike => &TRICKS,
        Kind::Ninja => &NINJA_TRICKS,
        Kind::Grapple => &[],
    }
}

/// Black widows: drop on silk from a window's bottom edge, hang, climb back, move on. Riders who hit one eat it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpiderState {
    Hidden { t: f32 },
    Down { len: f32, target: f32 },
    Hang { len: f32, t: f32 },
    Up { len: f32 },
}

#[derive(Clone, Debug)]
pub struct Spider {
    pub id: u32,
    pub window: Option<ExtWindowId>,
    /// Where along the window's bottom edge, 0..1.
    pub frac: f32,
    pub state: SpiderState,
}

/// The chameleon: wanders window tops, flicks its tongue, and camouflages out to turn up on another window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamState {
    Gone { t: f32 },
    FadeIn { t: f32 },
    Walk { left: f32 },
    Tongue { t: f32 },
    FadeOut { t: f32 },
}

#[derive(Clone, Debug)]
pub struct Chameleon {
    pub window: Option<ExtWindowId>,
    pub x: f32,
    pub face: f32,
    pub state: ChamState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trick {
    pub name: &'static str,
    pub frames: &'static [&'static str],
    pub secs: f32,
    pub points: u32,
}

pub const TRICKS: [Trick; 7] = [
    Trick { name: "NO HANDER", frames: &["trick_nohands"], secs: 0.45, points: 100 },
    Trick { name: "HEEL CLICKER", frames: &["trick_heel"], secs: 0.5, points: 150 },
    Trick { name: "CAN-CAN", frames: &["trick_cancan"], secs: 0.55, points: 200 },
    Trick { name: "LEG OVER", frames: &["trick_legover"], secs: 0.6, points: 250 },
    Trick { name: "NAC-NAC", frames: &["trick_nacnac"], secs: 0.7, points: 300 },
    Trick { name: "SUPERMAN", frames: &["trick_superman_0", "trick_superman_1", "trick_superman_0"], secs: 0.9, points: 500 },
    Trick { name: "BACKFLIP", frames: &["flip_0", "flip_1", "flip_2", "flip_3"], secs: 1.05, points: 900 },
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Waiting { t: f32 },
    Ride { seg: SegKey, y_ref: f32 },
    Air { t: f32, trick: Option<(usize, f32)>, done: u32 },
    Land { t: f32, seg: SegKey, y_ref: f32 },
    Crash { t: f32 },
    /// Hanging off a grapple line hooked into a window's bottom edge at `frac` along it; pendulum angle from
    /// straight down (`ang`, radians, + = right) and its rate `w`. `len` runs from the hook to his hand.
    Swing { win: ExtWindowId, frac: f32, len: f32, ang: f32, w: f32, t: f32 },
    /// Ninja scaling the side of a window he ran into; `edge0` is where that side was when he started.
    Climb { win: ExtWindowId, edge0: f32 },
}

#[derive(Clone, Debug)]
pub struct Rider {
    pub id: u32,
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub state: State,
    pub wheelie: f32,
    pub fist: f32,
    pub combo: u32,
    pub line_pts: u32,
    pub kind: Kind,
    /// Already used the grapple on this jump.
    pub grappled: bool,
}

/// A kicker is keyed by its window and which end it sits on (-1 = left end x0, +1 = right end x1).
pub type KickerKey = (ExtWindowId, i8);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CrewState {
    Idle { t: f32 },
    /// Hopping along the floor to the foot of the window side.
    Walk,
    /// Up the outside of the window's side edge; `x0` is where the edge was when the climb started.
    Climb { x0: f32 },
    Build { t: f32 },
    Fall { vy: f32, vx: f32, hard: bool },
    Down { t: f32, hard: bool },
    /// Nothing to build: haul a beam along the floor to `to`, so the crew is always on screen and busy.
    Haul { to: f32 },
}

#[derive(Clone, Debug)]
pub struct Crew {
    pub id: u32,
    pub pos: [f32; 2],
    pub state: CrewState,
    pub job: Option<KickerKey>,
    /// The ironworker: hauls an I-beam over, climbs, and sledgehammers the kicker in.
    pub iron: bool,
    /// On a reinforce job: re-hammering a finished kicker (keeps the crew working once every lip has one).
    pub fix: bool,
}

const BUILD_SECS: f32 = 5.0;
/// Rope-wrist man's hand, art px above his feet (the grapple line runs from here).
const HAND: f32 = 30.0;
/// Ironworkers on the build crew; two can share a kicker.
pub const IRON_CREW: u32 = 5;
/// Multiplies the cast in the real game (tests run at 1). Knox asked for x10, then called it too chaotic: 3.
pub const CHAOS: u32 = 3;
/// Size of everyone but the giant, relative to the art scale.
pub const CAST: f32 = 1.6;

/// The big guy (Knox's walker sheets): strolls along one monitor's floor, stops to watch a jump, walks off the
/// edge and turns up on another monitor. Drawn only on his own monitor, so he's clipped at its edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GiantState {
    Walk,
    Watch { t: f32 },
    Gone { t: f32 },
}

#[derive(Clone, Debug)]
pub struct Giant {
    pub pos: [f32; 2],
    pub face: f32,
    pub floor: u64,
    pub phase: f32,
    pub state: GiantState,
    cooldown: f32,
}

/// 18 clean frames covering ~5 steps of his walk (tools/build_sprites.py drops the glitched ones).
pub const GIANT_FRAMES: usize = 18;
const GIANT_STEPS: f32 = 5.0;
const GIANT_CYCLE: f32 = 2.75;
const GIANT_H: f32 = 200.0;
/// Ground covered per step, as a fraction of his height.
const GIANT_STEP_LEN: f32 = 0.4;

#[derive(Clone, Debug)]
pub struct Popup {
    pub text: String,
    pub pos: [f32; 2],
    pub ttl: f32,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Score {
    pub total: u64,
    pub best_line: u32,
    pub tricks: u32,
    pub crashes: u32,
    pub eaten: u32,
    pub laps: u32,
    pub kickers_built: u32,
    pub crew_falls: u32,
    pub spider_hits: u32,
}

pub struct Moto {
    pub riders: Vec<Rider>,
    pub crew: Vec<Crew>,
    pub giant: Giant,
    pub spiders: Vec<Spider>,
    pub chams: Vec<Chameleon>,
    /// Build progress 0..1 per kicker; 1 = built. Kickers ride their window and go when it closes.
    pub kickers: HashMap<KickerKey, f32>,
    pub popups: Vec<Popup>,
    /// Hot sparks: [x, y, vx, vy, ttl, max_ttl]. They fall, bounce off track tops, and cool from white to red.
    pub sparks: Vec<[f32; 6]>,
    pub score: Score,
    /// Scale of the cast: physical px per art px, times CAST (Knox: everything bigger, but not giant-sized).
    pub s: f32,
    /// The plain art scale: the giant and the TRENCH dressing use this.
    pub base_s: f32,
    start: [f32; 2],
    dir: f32,
    world: RectPx,
    rng: u64,
    pub t: f32,
    pub throttle: f32,
}

impl Moto {
    /// Start on the highest monitor, riding toward the middle of the desk.
    #[cfg(test)]
    pub fn new(mons: &[MonitorInfo], floors: &[Seg], base_s: f32, riders: u32, seed: u64) -> Moto {
        Self::with_chaos(mons, floors, base_s, riders, seed, 1)
    }

    /// `chaos` multiplies the cast: ninjas, rope-wrist men, spiders and chameleons (×chaos), the iron crew (×2 max).
    /// Bikes are `riders` as given.
    pub fn with_chaos(mons: &[MonitorInfo], floors: &[Seg], base_s: f32, riders: u32, seed: u64, chaos: u32) -> Moto {
        let chaos = chaos.max(1);
        let s = base_s * CAST;
        let world = mons.iter().skip(1).fold(mons[0].bounds, |a, m| a.union(&m.bounds));
        let top = mons.iter().min_by_key(|m| m.bounds.t).expect("a monitor");
        let mid = (world.l + world.r) as f32 / 2.0;
        let tcx = (top.bounds.l + top.bounds.r) as f32 / 2.0;
        let dir = if tcx > mid { -1.0 } else { 1.0 };
        let fy = floors.iter().find(|f| f.key == SegKey::Floor(top.id.0)).map(|f| f.y).unwrap_or(top.work.b as f32);
        let sx = if dir < 0.0 { top.bounds.r as f32 - 30.0 * s } else { top.bounds.l as f32 + 30.0 * s };
        // `riders` bikes, plus the ninja and rope-wrist man when there are any bikes.
        let extra = if riders > 0 { 2 * chaos } else { 0 };
        let stagger = (3.5 / chaos as f32).max(0.35);
        let riders = (0..riders + extra)
            .map(|i| Rider {
                id: i,
                pos: [sx, fy],
                vel: [0.0, 0.0],
                state: State::Waiting { t: 0.5 + i as f32 * stagger },
                wheelie: 0.0,
                fist: 0.0,
                combo: 0,
                line_pts: 0,
                kind: match i.checked_sub(riders) {
                    Some(k) if k < chaos => Kind::Ninja,
                    Some(_) => Kind::Grapple,
                    None => Kind::Bike,
                },
                grappled: false,
            })
            .collect();
        // Knox: a full crew of ironworkers at all times.
        let crew = (0..IRON_CREW * chaos.min(2)).map(|i| Crew { id: i, pos: [sx, fy], state: CrewState::Idle { t: 0.1 + i as f32 * 0.8 }, job: None, iron: true, fix: false }).collect();
        let giant = Giant { pos: [0.0, 0.0], face: 1.0, floor: 0, phase: 0.0, state: GiantState::Gone { t: 4.0 }, cooldown: 0.0 };
        let spiders = (0..2 * chaos).map(|i| Spider { id: i, window: None, frac: 0.5, state: SpiderState::Hidden { t: 3.0 + i as f32 * 5.0 / chaos as f32 } }).collect();
        let chams = (0..chaos).map(|i| Chameleon { window: None, x: 0.0, face: 1.0, state: ChamState::Gone { t: 2.0 + i as f32 * 0.7 } }).collect();
        Moto { riders, crew, giant, spiders, chams, kickers: HashMap::new(), popups: Vec::new(), sparks: Vec::new(), score: Score::default(), s, base_s, start: [sx, fy], dir, world, rng: seed | 1, t: 0.0, throttle: 0.0 }
    }

    /// The end of a window top that riders leave from, given the direction of travel.
    pub fn lip_side(&self) -> i8 {
        if self.dir < 0.0 {
            -1
        } else {
            1
        }
    }

    fn built(&self, k: SegKey) -> bool {
        match k {
            SegKey::Win(w) => self.kickers.get(&(w, self.lip_side())).is_some_and(|p| *p >= 1.0),
            SegKey::Floor(_) => false,
        }
    }

    fn floor_under(segs: &[Seg], x: f32) -> Option<f32> {
        segs.iter().filter(|g| matches!(g.key, SegKey::Floor(_)) && x >= g.x0 && x <= g.x1).map(|g| g.y).next()
    }

    /// Climb line for a kicker: just outside the window's side edge, so the climber is on bare desktop.
    fn climb_x(&self, g: &Seg, side: i8) -> f32 {
        if side < 0 {
            g.x0 - 8.0 * self.s
        } else {
            g.x1 + 8.0 * self.s
        }
    }

    fn step_crew(&mut self, c: &mut Crew, segs: &[Seg]) {
        let s = self.s;
        let job_seg = c.job.and_then(|(w, _)| Self::seg(segs, SegKey::Win(w)).copied());
        if c.job.is_some() && job_seg.is_none() && !matches!(c.state, CrewState::Fall { .. } | CrewState::Down { .. }) {
            // Window closed or gone: drop the job (and fall if we were on it).
            c.job = None;
            c.state = if matches!(c.state, CrewState::Climb { .. } | CrewState::Build { .. }) { CrewState::Fall { vy: 0.0, vx: 0.0, hard: true } } else { CrewState::Idle { t: 0.5 } };
            return;
        }
        match c.state {
            CrewState::Idle { t } => {
                if t > DT {
                    c.state = CrewState::Idle { t: t - DT };
                    return;
                }
                // Next job: a window lip with no finished kicker and room for another worker (two per kicker).
                let side = self.lip_side();
                let jobs: Vec<KickerKey> = self.crew.iter().filter(|o| o.id != c.id).filter_map(|o| o.job).collect();
                let taken: Vec<KickerKey> = jobs.iter().copied().filter(|k| jobs.iter().filter(|j| *j == k).count() >= 2).collect();
                let open: Vec<ExtWindowId> = segs
                    .iter()
                    .filter_map(|g| match g.key {
                        SegKey::Win(w) if g.kick => Some(w),
                        _ => None,
                    })
                    .filter(|w| self.kickers.get(&(*w, side)).is_none_or(|p| *p < 1.0) && !taken.contains(&(*w, side)))
                    .collect();
                let fixable: Vec<ExtWindowId> = segs
                    .iter()
                    .filter_map(|g| match g.key {
                        SegKey::Win(w) if g.kick => Some(w),
                        _ => None,
                    })
                    .filter(|w| self.kickers.get(&(*w, side)).is_some_and(|p| *p >= 1.0) && !taken.contains(&(*w, side)))
                    .collect();
                let (pool, fix) = if !open.is_empty() { (open, false) } else { (fixable, true) };
                if pool.is_empty() {
                    // Nothing to build or reinforce: haul a beam somewhere along a floor.
                    let floors: Vec<Seg> = segs.iter().filter(|g| matches!(g.key, SegKey::Floor(_))).copied().collect();
                    let here = floors.iter().find(|g| (g.y - c.pos[1]).abs() < 2.0 && c.pos[0] >= g.x0 && c.pos[0] <= g.x1).copied();
                    let f = match here {
                        Some(f) => f,
                        None => {
                            let f = floors[(self.rnd() * floors.len() as f32) as usize % floors.len()];
                            c.pos = [f.x0 + (f.x1 - f.x0) * self.rnd(), f.y];
                            f
                        }
                    };
                    let to = f.x0 + 60.0 * s + (f.x1 - f.x0 - 120.0 * s).max(0.0) * self.rnd();
                    c.state = CrewState::Haul { to };
                    return;
                }
                let w = pool[(self.rnd() * pool.len() as f32) as usize % pool.len()];
                c.job = Some((w, side));
                c.fix = fix;
                let g = *Self::seg(segs, SegKey::Win(w)).expect("open job has a segment");
                let cx = self.climb_x(&g, side);
                let fy = Self::floor_under(segs, cx).unwrap_or(g.y + 200.0 * s);
                if (c.pos[1] - fy).abs() > 2.0 || (c.pos[0] - cx).abs() > 900.0 * s {
                    // Different floor or far away: hop in from just off to the side.
                    c.pos = [cx - side as f32 * 160.0 * s, fy];
                }
                c.state = CrewState::Walk;
            }
            CrewState::Haul { to } => {
                if let Some(fy) = Self::floor_under(segs, c.pos[0]) {
                    c.pos[1] = fy;
                }
                let d = to - c.pos[0];
                let step = 55.0 * s * DT;
                if d.abs() <= step {
                    c.state = CrewState::Idle { t: 1.0 + 2.0 * self.rnd() };
                } else {
                    c.pos[0] += step * d.signum();
                }
            }
            CrewState::Walk => {
                let (Some(g), Some((_, side))) = (job_seg, c.job) else { return };
                let cx = self.climb_x(&g, side);
                let fy = Self::floor_under(segs, cx).unwrap_or(c.pos[1]);
                c.pos[1] = fy;
                let d = cx - c.pos[0];
                let step = if c.iron { 55.0 } else { 75.0 } * s * DT;
                if d.abs() <= step {
                    c.pos[0] = cx;
                    c.state = CrewState::Climb { x0: if side < 0 { g.x0 } else { g.x1 } };
                } else {
                    c.pos[0] += step * d.signum();
                }
            }
            CrewState::Climb { x0 } => {
                let (Some(g), Some((_, side))) = (job_seg, c.job) else { return };
                let edge = if side < 0 { g.x0 } else { g.x1 };
                if (edge - x0).abs() > 6.0 {
                    // The window moved under the climber.
                    self.score.crew_falls += 1;
                    c.state = CrewState::Fall { vy: 0.0, vx: -(side as f32) * 40.0 * s, hard: true };
                    return;
                }
                c.pos[0] = self.climb_x(&g, side);
                c.pos[1] -= 85.0 * s * DT;
                if c.pos[1] <= g.y {
                    let slot = self.crew.iter().filter(|o| o.id < c.id && o.job == c.job).count() as f32;
                    c.pos = [edge - side as f32 * (40.0 + 34.0 * slot) * s, g.y];
                    c.state = CrewState::Build { t: 0.0 };
                }
            }
            CrewState::Build { t } => {
                let (Some(g), Some(key)) = (job_seg, c.job) else { return };
                let side = key.1;
                let edge = if side < 0 { g.x0 } else { g.x1 };
                let slot = self.crew.iter().filter(|o| o.id < c.id && o.job == c.job).count() as f32;
                let want = [edge - side as f32 * (40.0 + 34.0 * slot) * s, g.y];
                if (want[1] - c.pos[1]).abs() > 6.0 || (want[0] - c.pos[0]).abs() > 6.0 {
                    self.score.crew_falls += 1;
                    c.state = CrewState::Fall { vy: 0.0, vx: side as f32 * 40.0 * s, hard: true };
                    return;
                }
                c.pos = want;
                let t = t + DT;
                if c.iron && t > 0.4 {
                    // Sparks: a shower off every hammer strike, a steady spray while he welds.
                    let (weld, f) = iron_pose(t, self.t);
                    let face = side as f32;
                    if weld {
                        let tip = [c.pos[0] + face * 26.0 * s, c.pos[1] - 4.0 * s];
                        for _ in 0..4 {
                            let (a, b, d) = (self.rnd(), self.rnd(), self.rnd());
                            self.spark(tip, face * (60.0 + 140.0 * a), -(40.0 + 160.0 * b), 0.35 + 0.4 * d);
                        }
                    } else if f == 3 {
                        let head = [c.pos[0] + face * 24.0 * s, c.pos[1] - 2.0 * s];
                        // Frame 3 is the strike: a fan of sparks up and out from the hammer head.
                        for _ in 0..7 {
                            let v = 120.0 + 330.0 * self.rnd();
                            let vx = (self.rnd() * 2.0 - 1.0) * v * 0.9;
                            let vy = -(0.3 + 0.7 * self.rnd()) * v;
                            let life = 0.4 + 0.6 * self.rnd();
                            self.spark(head, vx, vy, life);
                        }
                    }
                }
                if c.fix {
                    // Reinforcing: a few seconds of work on a finished kicker, then hop off.
                    if t > 4.4 {
                        c.job = None;
                        c.fix = false;
                        c.pos[0] = edge + side as f32 * 6.0 * s;
                        c.state = CrewState::Fall { vy: -220.0 * s, vx: side as f32 * 70.0 * s, hard: false };
                    } else {
                        c.state = CrewState::Build { t };
                    }
                    return;
                }
                let p = self.kickers.entry(key).or_insert(0.0);
                if t > 0.4 {
                    *p = (*p + DT / BUILD_SECS).min(1.0);
                }
                if *p >= 1.0 {
                    self.score.kickers_built += 1;
                    self.pop("KICKER UP".into(), want);
                    c.job = None;
                    // Hop off the outside of the window.
                    c.pos[0] = edge + side as f32 * 6.0 * s;
                    c.state = CrewState::Fall { vy: -220.0 * s, vx: side as f32 * 70.0 * s, hard: false };
                } else {
                    c.state = CrewState::Build { t };
                }
            }
            CrewState::Fall { vy, vx, hard } => {
                if hard {
                    c.job = None;
                }
                let prev = c.pos[1];
                let vy = vy + self.gravity() * DT;
                c.pos[0] += vx * DT;
                c.pos[1] += vy * DT;
                if vy > 0.0 {
                    if let Some(g) = Self::landing(segs, c.pos[0], prev, c.pos[1]) {
                        c.pos[1] = g.y;
                        c.state = CrewState::Down { t: 0.0, hard };
                        return;
                    }
                }
                if c.pos[1] > self.world.b as f32 + 100.0 {
                    c.pos = self.start;
                    c.state = CrewState::Idle { t: 2.0 };
                    return;
                }
                c.state = CrewState::Fall { vy, vx, hard };
            }
            CrewState::Down { t, hard } => {
                let t = t + DT;
                c.state = if t > if hard { 1.6 } else { 1.0 } { CrewState::Idle { t: 0.4 } } else { CrewState::Down { t, hard } };
            }
        }
    }

    fn rnd(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn dir(&self) -> f32 {
        self.dir
    }

    fn speed(&self) -> f32 {
        (250.0 + 170.0 * self.throttle) * self.s
    }
    /// Velocity in art px/s; scaled to the cast here.
    fn spark(&mut self, at: [f32; 2], vx: f32, vy: f32, ttl: f32) {
        if self.sparks.len() < 4000 {
            self.sparks.push([at[0], at[1], vx * self.s, vy * self.s, ttl, ttl]);
        }
    }

    fn step_sparks(&mut self, segs: &[Seg]) {
        let g = self.gravity() * 0.7;
        for p in &mut self.sparks {
            let prev = p[1];
            p[3] += g * DT;
            p[0] += p[2] * DT;
            p[1] += p[3] * DT;
            p[4] -= DT;
            if p[3] > 0.0 {
                if let Some(sg) = Self::landing(segs, p[0], prev, p[1]) {
                    p[1] = sg.y - 1.0;
                    p[3] *= -0.35;
                    p[2] *= 0.6;
                }
            }
        }
        self.sparks.retain(|p| p[4] > 0.0);
    }

    /// Best hook point for rope-wrist man: a window bottom edge above him and ahead, within line range.
    fn grapple_target(&self, r: &Rider, segs: &[Seg]) -> Option<(ExtWindowId, f32, [f32; 2])> {
        let s = self.s;
        let hand = [r.pos[0], r.pos[1] - HAND * s];
        segs.iter()
            .filter_map(|g| {
                let SegKey::Win(win) = g.key else { return None };
                if g.bottom > hand[1] - 60.0 * s || g.x1 - g.x0 < 40.0 * s {
                    return None;
                }
                let ax = (r.pos[0] + self.dir * 160.0 * s).clamp(g.x0 + 10.0 * s, g.x1 - 10.0 * s);
                if (ax - r.pos[0]) * self.dir < 20.0 * s {
                    return None;
                }
                let d = ((ax - hand[0]).powi(2) + (g.bottom - hand[1]).powi(2)).sqrt();
                (120.0 * s..=460.0 * s).contains(&d).then_some((win, (ax - g.x0) / (g.x1 - g.x0), [ax, g.bottom], d))
            })
            .min_by(|a, b| a.3.total_cmp(&b.3))
            .map(|(w, f, a, _)| (w, f, a))
    }

    fn rspeed(&self, r: &Rider) -> f32 {
        self.speed()
            * match r.kind {
                Kind::Bike => 1.0,
                Kind::Ninja => 0.85,
                Kind::Grapple => 0.9,
            }
    }

    /// Where a spider's body is (bottom-centre), if it's out on its silk.
    pub fn spider_pos(sp: &Spider, segs: &[Seg]) -> Option<([f32; 2], f32)> {
        let g = Self::seg(segs, SegKey::Win(sp.window?))?;
        let x = g.x0 + sp.frac * (g.x1 - g.x0);
        let len = match sp.state {
            SpiderState::Down { len, .. } | SpiderState::Hang { len, .. } | SpiderState::Up { len } => len,
            _ => return None,
        };
        Some(([x, g.bottom + len], g.bottom))
    }

    fn step_spider(&mut self, sp: &mut Spider, segs: &[Seg]) {
        let s = self.s;
        let alive = matches!(sp.state, SpiderState::Down { .. } | SpiderState::Hang { .. } | SpiderState::Up { .. });
        if alive && sp.window.and_then(|w| Self::seg(segs, SegKey::Win(w))).is_none() {
            // Its window closed: it scuttles off and turns up somewhere else later.
            sp.window = None;
            sp.state = SpiderState::Hidden { t: 6.0 };
        }
        match sp.state {
            SpiderState::Hidden { t } => {
                if t > DT {
                    sp.state = SpiderState::Hidden { t: t - DT };
                    return;
                }
                let wins: Vec<Seg> = segs.iter().filter(|g| g.kick).copied().collect();
                if wins.is_empty() {
                    sp.state = SpiderState::Hidden { t: 2.0 };
                    return;
                }
                let g = wins[(self.rnd() * wins.len() as f32) as usize % wins.len()];
                sp.frac = 0.15 + 0.7 * self.rnd();
                let x = g.x0 + sp.frac * (g.x1 - g.x0);
                let gap = Self::floor_under(segs, x).map(|f| f - g.bottom).unwrap_or(0.0);
                if gap < 90.0 * s {
                    sp.state = SpiderState::Hidden { t: 2.0 };
                    return;
                }
                sp.window = match g.key {
                    SegKey::Win(w) => Some(w),
                    SegKey::Floor(_) => None,
                };
                let target = ((60.0 + 200.0 * self.rnd()) * s).min(gap - 50.0 * s);
                sp.state = SpiderState::Down { len: 0.0, target };
            }
            SpiderState::Down { len, target } => {
                let len = len + 70.0 * s * DT;
                sp.state = if len >= target { SpiderState::Hang { len: target, t: 0.0 } } else { SpiderState::Down { len, target } };
            }
            SpiderState::Hang { len, t } => {
                let t = t + DT;
                sp.state = if t > 5.0 + sp.id as f32 * 1.5 { SpiderState::Up { len } } else { SpiderState::Hang { len, t } };
            }
            SpiderState::Up { len } => {
                let len = len - 45.0 * s * DT;
                if len <= 0.0 {
                    sp.window = None;
                    sp.state = SpiderState::Hidden { t: 4.0 + self.rnd() * 8.0 };
                } else {
                    sp.state = SpiderState::Up { len };
                }
            }
        }
    }

    fn step_cham(&mut self, i: usize, segs: &[Seg]) {
        let s = self.s;
        let mut c = self.chams[i].clone();
        let seg = c.window.and_then(|w| Self::seg(segs, SegKey::Win(w))).copied();
        if seg.is_none() && !matches!(c.state, ChamState::Gone { .. }) {
            c.state = ChamState::Gone { t: 2.0 };
            c.window = None;
        }
        c.state = match c.state {
            ChamState::Gone { t } if t > DT => ChamState::Gone { t: t - DT },
            ChamState::Gone { .. } => {
                let wins: Vec<Seg> = segs.iter().filter(|g| g.kick && g.x1 - g.x0 > 260.0 * s).copied().collect();
                if wins.is_empty() {
                    ChamState::Gone { t: 2.0 }
                } else {
                    let g = wins[(self.rnd() * wins.len() as f32) as usize % wins.len()];
                    c.window = match g.key {
                        SegKey::Win(w) => Some(w),
                        SegKey::Floor(_) => None,
                    };
                    c.x = 0.2 + 0.6 * self.rnd();
                    c.face = if self.rnd() < 0.5 { 1.0 } else { -1.0 };
                    ChamState::FadeIn { t: 0.0 }
                }
            }
            ChamState::FadeIn { t } => if t + DT > 1.2 { ChamState::Walk { left: 2.0 + self.rnd() * 4.0 } } else { ChamState::FadeIn { t: t + DT } },
            ChamState::Walk { left } => {
                let g = seg.expect("walking chameleon has a window");
                let w = (g.x1 - g.x0).max(1.0);
                c.x += c.face * 28.0 * s * DT / w;
                let margin = 50.0 * s / w;
                if c.x < margin || c.x > 1.0 - margin {
                    c.face = -c.face;
                    c.x = c.x.clamp(margin, 1.0 - margin);
                }
                if left - DT > 0.0 {
                    ChamState::Walk { left: left - DT }
                } else if self.rnd() < 0.6 {
                    ChamState::Tongue { t: 0.0 }
                } else {
                    ChamState::FadeOut { t: 0.0 }
                }
            }
            ChamState::Tongue { t } => if t + DT > 0.8 { ChamState::Walk { left: 2.0 + self.rnd() * 3.0 } } else { ChamState::Tongue { t: t + DT } },
            ChamState::FadeOut { t } => {
                if t + DT > 1.2 {
                    c.window = None;
                    ChamState::Gone { t: 2.0 + self.rnd() * 5.0 }
                } else {
                    ChamState::FadeOut { t: t + DT }
                }
            }
        };
        self.chams[i] = c;
    }
    fn gravity(&self) -> f32 {
        1500.0 * self.s
    }
    fn kick(&self) -> f32 {
        560.0 * self.s
    }

    /// `key_rate` = key events per second this step (counts only); it smooths into the throttle.
    pub fn step(&mut self, segs: &[Seg], key_rate: f32) {
        self.t += DT;
        let want = (key_rate / 6.0).min(1.0);
        self.throttle += (want - self.throttle) * (DT * 1.5).min(1.0);
        for p in &mut self.popups {
            p.ttl -= DT;
            p.pos[1] -= 40.0 * self.s * DT;
        }
        self.popups.retain(|p| p.ttl > 0.0);
        self.kickers.retain(|(w, _), _| segs.iter().any(|g| g.key == SegKey::Win(*w)));
        for i in 0..self.riders.len() {
            let mut r = self.riders[i].clone();
            self.step_rider(&mut r, segs);
            self.riders[i] = r;
        }
        for i in 0..self.crew.len() {
            let mut c = self.crew[i].clone();
            self.step_crew(&mut c, segs);
            self.crew[i] = c;
        }
        let mut g = self.giant.clone();
        self.step_giant(&mut g, segs);
        self.giant = g;
        for i in 0..self.spiders.len() {
            let mut sp = self.spiders[i].clone();
            self.step_spider(&mut sp, segs);
            self.spiders[i] = sp;
        }
        for i in 0..self.chams.len() {
            self.step_cham(i, segs);
        }
        self.step_sparks(segs);
        // Riding into a hanging spider: the rider eats it.
        let bodies: Vec<[f32; 2]> = self.spiders.iter().filter_map(|sp| Self::spider_pos(sp, segs).map(|p| p.0)).collect();
        let s = self.s;
        for i in 0..self.riders.len() {
            let r = self.riders[i].clone();
            if !matches!(r.state, State::Ride { .. } | State::Air { .. } | State::Land { .. } | State::Swing { .. } | State::Climb { .. }) {
                continue;
            }
            let hit = bodies.iter().any(|b| (b[0] - r.pos[0]).abs() < 16.0 * s && (b[1] - 10.0 * s - (r.pos[1] - 20.0 * s)).abs() < 22.0 * s);
            if hit {
                let mut r = r;
                self.score.spider_hits += 1;
                self.crash(&mut r, "SPIDER!");
                self.riders[i] = r;
            }
        }
    }

    fn step_giant(&mut self, g: &mut Giant, segs: &[Seg]) {
        let s = self.base_s;
        g.cooldown = (g.cooldown - DT).max(0.0);
        let floors: Vec<Seg> = segs.iter().filter(|x| matches!(x.key, SegKey::Floor(_))).copied().collect();
        if floors.is_empty() {
            return;
        }
        let half = 70.0 * s;
        match g.state {
            GiantState::Gone { t } => {
                if t > DT {
                    g.state = GiantState::Gone { t: t - DT };
                    return;
                }
                // Turn up at the edge of some monitor's floor and walk in.
                let f = floors[(self.rnd() * floors.len() as f32) as usize % floors.len()];
                g.face = if self.rnd() < 0.5 { 1.0 } else { -1.0 };
                g.floor = match f.key {
                    SegKey::Floor(id) => id,
                    SegKey::Win(_) => unreachable!(),
                };
                g.pos = [if g.face > 0.0 { f.x0 - half } else { f.x1 + half }, f.y];
                g.state = GiantState::Walk;
            }
            GiantState::Walk => {
                let Some(f) = floors.iter().find(|x| x.key == SegKey::Floor(g.floor)) else {
                    g.state = GiantState::Gone { t: 2.0 };
                    return;
                };
                g.pos[1] = f.y;
                g.pos[0] += g.face * GIANT_STEPS * GIANT_STEP_LEN * GIANT_H / GIANT_CYCLE * s * DT;
                g.phase = (g.phase + DT / GIANT_CYCLE) % 1.0;
                if (g.face > 0.0 && g.pos[0] > f.x1 + half) || (g.face < 0.0 && g.pos[0] < f.x0 - half) {
                    g.state = GiantState::Gone { t: 3.0 + self.rnd() * 6.0 };
                    return;
                }
                // Stop and watch when a rider is in the air near him.
                let near = self.riders.iter().any(|r| matches!(r.state, State::Air { .. }) && (r.pos[0] - g.pos[0]).abs() < 600.0 * s && (r.pos[1] - f.y).abs() < 900.0 * s);
                if near && g.cooldown <= 0.0 && g.pos[0] > f.x0 + half && g.pos[0] < f.x1 - half {
                    g.state = GiantState::Watch { t: 0.0 };
                }
            }
            GiantState::Watch { t } => {
                let t = t + DT;
                if t > 2.5 {
                    g.cooldown = 8.0;
                    g.state = GiantState::Walk;
                } else {
                    g.state = GiantState::Watch { t };
                }
            }
        }
    }

    fn seg<'a>(segs: &'a [Seg], k: SegKey) -> Option<&'a Seg> {
        segs.iter().find(|s| s.key == k)
    }

    /// First segment the ballistic path lands on between y0 and y1 at x.
    fn landing(segs: &[Seg], x: f32, y0: f32, y1: f32) -> Option<&Seg> {
        segs.iter().filter(|s| x >= s.x0 && x <= s.x1 && s.y >= y0 - 0.5 && s.y <= y1).min_by(|a, b| a.y.total_cmp(&b.y))
    }

    /// Seconds until the rider lands, simulated forward (3 s cap = falls out of the world).
    fn predict_air(&self, r: &Rider, segs: &[Seg]) -> f32 {
        let (mut x, mut y, vx, mut vy) = (r.pos[0], r.pos[1], r.vel[0], r.vel[1]);
        let mut t = 0.0;
        while t < 3.0 {
            vy += self.gravity() * DT;
            let ny = y + vy * DT;
            x += vx * DT;
            if vy > 0.0 && Self::landing(segs, x, y, ny).is_some() {
                return t;
            }
            y = ny;
            t += DT;
        }
        3.0
    }

    fn respawn(&self, r: &mut Rider) {
        r.pos = self.start;
        r.vel = [0.0, 0.0];
        r.state = State::Waiting { t: 0.8 };
        r.combo = 0;
        r.line_pts = 0;
    }

    fn pop(&mut self, text: String, pos: [f32; 2]) {
        self.popups.push(Popup { text, pos: [pos[0], pos[1] - 60.0 * self.s], ttl: 1.4 });
    }

    fn crash(&mut self, r: &mut Rider, why: &str) {
        r.state = State::Crash { t: 0.0 };
        r.vel = [0.0, 0.0];
        r.combo = 0;
        r.line_pts = 0;
        self.score.crashes += 1;
        self.pop(why.to_string(), r.pos);
    }

    fn launch(&mut self, r: &mut Rider, kick: bool) {
        let kick = kick || r.kind != Kind::Bike;
        r.vel = [self.dir * self.rspeed(r), if kick { -self.kick() } else { 0.0 }];
        r.state = State::Air { t: 0.0, trick: None, done: 0 };
        r.wheelie = 0.0;
    }

    fn step_rider(&mut self, r: &mut Rider, segs: &[Seg]) {
        let out_of_world = |p: [f32; 2], w: &RectPx| p[0] < w.l as f32 - 200.0 || p[0] > w.r as f32 + 200.0 || p[1] > w.b as f32 + 200.0;
        match r.state {
            State::Waiting { t } => {
                if t > DT {
                    r.state = State::Waiting { t: t - DT };
                } else {
                    match Self::landing(segs, self.start[0], self.start[1] - 2.0, self.start[1] + 2.0).map(|s| (s.key, s.y)) {
                        Some((k, y)) => {
                            r.pos = [self.start[0], y];
                            r.state = State::Ride { seg: k, y_ref: y };
                        }
                        None => {
                            r.pos = self.start;
                            r.vel = [self.dir * self.rspeed(r), 0.0];
                            r.state = State::Air { t: 0.0, trick: None, done: 0 };
                        }
                    }
                }
            }
            State::Ride { seg, y_ref } | State::Land { seg, y_ref, .. } => {
                let Some(s) = Self::seg(segs, seg).copied() else {
                    r.vel = [self.dir * self.rspeed(r), 0.0];
                    r.state = State::Air { t: 0.0, trick: None, done: 0 };
                    return;
                };
                if (s.y - y_ref).abs() > 6.0 {
                    // The window under the rider moved: it threw him.
                    self.score.eaten += 1;
                    self.crash(r, "ATE IT");
                    return;
                }
                r.pos[0] += self.dir * self.rspeed(r) * DT;
                r.pos[1] = s.y;
                r.fist = (r.fist - DT).max(0.0);
                if let State::Land { t, .. } = r.state {
                    r.state = if t + DT > 0.3 { State::Ride { seg, y_ref: s.y } } else { State::Land { t: t + DT, seg, y_ref: s.y } };
                } else {
                    r.state = State::Ride { seg, y_ref: s.y };
                    if r.wheelie > 0.0 {
                        r.wheelie -= DT;
                    } else if r.kind == Kind::Bike && self.rnd() < DT * 0.12 {
                        r.wheelie = 1.0 + self.rnd() * 1.2;
                    }
                }
                // Ninja: run into the side of a taller window and climb it.
                if r.kind == Kind::Ninja && matches!(r.state, State::Ride { .. }) {
                    let sc = self.s;
                    let prev_x = r.pos[0] - self.dir * self.rspeed(r) * DT;
                    let wall = segs.iter().find_map(|g| {
                        let SegKey::Win(w) = g.key else { return None };
                        let edge = if self.dir < 0.0 { g.x1 } else { g.x0 };
                        let line = edge - self.dir * 8.0 * sc;
                        let crossed = (prev_x - line) * self.dir <= 0.0 && (r.pos[0] - line) * self.dir >= 0.0;
                        (crossed && g.y < s.y - 60.0 * sc && g.bottom >= s.y - 260.0 * sc).then_some((w, edge, line))
                    });
                    if let Some((w, edge, line)) = wall {
                        if self.rnd() < 0.8 {
                            r.pos[0] = line;
                            r.state = State::Climb { win: w, edge0: edge };
                            return;
                        }
                    }
                }
                let off = if self.dir < 0.0 { r.pos[0] < s.x0 } else { r.pos[0] > s.x1 };
                if off {
                    let kicked = self.built(s.key);
                    self.launch(r, kicked);
                }
            }
            State::Air { t, trick, done } => {
                let prev_y = r.pos[1];
                r.vel[1] += self.gravity() * DT;
                r.pos[0] += r.vel[0] * DT;
                r.pos[1] += r.vel[1] * DT;
                let t = t + DT;
                // Pick a trick once airborne, sized to the predicted airtime; greedy riders sometimes overcommit.
                let mut trick = trick;
                let mut done = done;
                if let Some((k, left)) = trick {
                    let left = left - DT;
                    if left <= 0.0 {
                        let tk = tricks(r.kind)[k];
                        done += 1;
                        r.combo += 1;
                        r.line_pts += tk.points * r.combo;
                        self.score.tricks += 1;
                        let label = if r.combo > 1 { format!("{} +{} x{}", tk.name, tk.points, r.combo) } else { format!("{} +{}", tk.name, tk.points) };
                        self.pop(label, r.pos);
                        trick = None;
                    } else {
                        trick = Some((k, left));
                    }
                } else if t > 0.1 {
                    let air = self.predict_air(r, segs) - 0.12;
                    let greedy = self.rnd() < 0.12;
                    let tl = tricks(r.kind);
                    let fits: Vec<usize> = (0..tl.len()).filter(|&k| greedy || tl[k].secs < air).collect();
                    if !fits.is_empty() && air > 0.3 && done < 2 {
                        // Lean toward the biggest trick that fits.
                        let pick = fits[(fits.len() as f32 * (0.4 + 0.6 * self.rnd())).min(fits.len() as f32 - 1.0) as usize];
                        trick = Some((pick, tl[pick].secs));
                    }
                }
                // Rope-wrist man: fire the grapple into a window bottom ahead and swing.
                if r.kind == Kind::Grapple && !r.grappled && t > 0.12 {
                    if let Some((win, frac, anchor)) = self.grapple_target(r, segs) {
                        let hand = [r.pos[0], r.pos[1] - HAND * self.s];
                        let (dx, dy) = (hand[0] - anchor[0], hand[1] - anchor[1]);
                        let len = (dx * dx + dy * dy).sqrt().max(1.0);
                        let ang = dx.atan2(dy);
                        let w = (r.vel[0] * ang.cos() - r.vel[1] * ang.sin()) / len;
                        r.grappled = true;
                        r.state = State::Swing { win, frac, len, ang, w, t: 0.0 };
                        return;
                    }
                }
                if r.vel[1] > 0.0 {
                    if let Some(s) = Self::landing(segs, r.pos[0], prev_y, r.pos[1]).copied() {
                        r.pos[1] = s.y;
                        if trick.is_some() {
                            self.crash(r, "CASED IT");
                            return;
                        }
                        r.vel = [0.0, 0.0];
                        if done > 0 && r.line_pts > 0 {
                            self.score.total += r.line_pts as u64;
                            if r.line_pts > self.score.best_line {
                                self.score.best_line = r.line_pts;
                                self.pop(format!("BEST LINE {}", r.line_pts), [r.pos[0], r.pos[1] - 24.0 * self.s]);
                            }
                            if r.combo >= 2 || done >= 2 {
                                r.fist = 0.8;
                            }
                        }
                        r.grappled = false;
                        r.state = State::Land { t: 0.0, seg: s.key, y_ref: s.y };
                        return;
                    }
                }
                if out_of_world(r.pos, &self.world) || t > 4.0 {
                    if r.line_pts > 0 {
                        self.score.total += r.line_pts as u64;
                    }
                    self.score.laps += 1;
                    self.respawn(r);
                    return;
                }
                r.state = State::Air { t, trick, done };
            }
            State::Climb { win, edge0 } => {
                let sc = self.s;
                let Some(g) = Self::seg(segs, SegKey::Win(win)).copied() else {
                    r.vel = [0.0, 0.0];
                    r.state = State::Air { t: 0.3, trick: None, done: 0 };
                    return;
                };
                let edge = if self.dir < 0.0 { g.x1 } else { g.x0 };
                if (edge - edge0).abs() > 6.0 {
                    // The wall moved out from under him.
                    r.vel = [-self.dir * 60.0 * sc, 0.0];
                    r.state = State::Air { t: 0.3, trick: None, done: 0 };
                    return;
                }
                r.pos[0] = edge - self.dir * 8.0 * sc;
                r.pos[1] -= 210.0 * sc * DT;
                if r.pos[1] <= g.y {
                    // Over the top and straight into a run.
                    r.pos = [edge + self.dir * 6.0 * sc, g.y];
                    r.combo += 1;
                    r.line_pts += 150 * r.combo;
                    self.score.tricks += 1;
                    self.pop("WALL CLIMB +150".into(), r.pos);
                    r.state = State::Ride { seg: SegKey::Win(win), y_ref: g.y };
                }
            }
            State::Swing { win, frac, len, ang, w, t } => {
                let Some(g) = Self::seg(segs, SegKey::Win(win)).copied() else {
                    // The window went: line goes slack, he drops.
                    r.state = State::Air { t: 0.3, trick: None, done: 0 };
                    return;
                };
                let anchor = [g.x0 + frac * (g.x1 - g.x0), g.bottom];
                let w = w - self.gravity() / len * ang.sin() * DT;
                let ang = ang + w * DT;
                let t = t + DT;
                let prev_y = r.pos[1];
                let hand = [anchor[0] + len * ang.sin(), anchor[1] + len * ang.cos()];
                let feet = [hand[0], hand[1] + HAND * self.s];
                if feet[1] > prev_y {
                    if let Some(sg) = Self::landing(segs, feet[0], prev_y, feet[1]).copied() {
                        r.pos = [feet[0], sg.y];
                        r.vel = [0.0, 0.0];
                        r.grappled = false;
                        r.state = State::Land { t: 0.0, seg: sg.key, y_ref: sg.y };
                        return;
                    }
                }
                r.pos = feet;
                let ahead = ang * self.dir;
                let swinging_back = w * self.dir < 0.0;
                if (ahead > 0.8 && !swinging_back) || t > 2.5 || (t > 0.4 && swinging_back && ahead > 0.0) {
                    // Let go at the top of the swing and fly on.
                    r.vel = [len * w * ang.cos(), -len * w * ang.sin()];
                    let min_vx = self.rspeed(r) * 0.6;
                    if r.vel[0] * self.dir < min_vx {
                        r.vel[0] = self.dir * min_vx;
                    }
                    r.combo += 1;
                    r.line_pts += 300 * r.combo;
                    self.score.tricks += 1;
                    let label = if r.combo > 1 { format!("SWING +300 x{}", r.combo) } else { "SWING +300".to_string() };
                    self.pop(label, r.pos);
                    r.state = State::Air { t: 0.3, trick: None, done: 1 };
                } else {
                    r.state = State::Swing { win, frac, len, ang, w, t };
                }
            }
            State::Crash { t } => {
                let t = t + DT;
                if r.kind == Kind::Ninja && t > 1.2 {
                    // Smoke bomb: gone, back at the start.
                    self.respawn(r);
                } else if t > 2.0 {
                    // Back on the bike where he fell, or from the top if he fell into nothing.
                    match Self::landing(segs, r.pos[0], r.pos[1] - 2.0, r.pos[1] + 2.0).map(|s| (s.key, s.y)) {
                        Some((k, y)) => r.state = State::Ride { seg: k, y_ref: y },
                        None => self.respawn(r),
                    }
                } else {
                    r.state = State::Crash { t };
                }
            }
        }
        if let State::Ride { .. } = r.state {
            if out_of_world(r.pos, &self.world) {
                self.score.laps += 1;
                self.respawn(r);
            }
        }
    }
}

/// Floors (the TRENCH floor on the home monitor, else the work-area bottom) + usable window tops.
pub fn segments(mons: &[MonitorInfo], trench: Option<RectPx>, windows: &HashMap<ExtWindowId, RectPx>, s: f32) -> Vec<Seg> {
    let mut out = Vec::new();
    for m in mons {
        let y = match trench {
            Some(t) if m.bounds.contains((t.l + t.r) / 2, (t.t + t.b) / 2) => t.b as f32 - 6.0 * s,
            _ => m.work.b as f32,
        };
        out.push(Seg { key: SegKey::Floor(m.id.0), x0: m.bounds.l as f32, x1: m.bounds.r as f32, y, kick: false, bottom: y });
    }
    for (id, r) in windows {
        let (cx, ty) = ((r.l + r.r) / 2, r.t + 1);
        let Some(m) = mons.iter().find(|m| m.bounds.contains(cx, ty)) else { continue };
        let taskbar = r.height() <= 64 && r.width() >= m.bounds.width() - 2;
        let near_top = r.t <= m.bounds.t + (48.0 * s) as i32;
        let too_small = r.width() < (140.0 * s) as i32 || r.height() < (60.0 * s) as i32;
        let below_floor = r.t as f32 >= m.work.b as f32 - 8.0;
        if taskbar || near_top || too_small || below_floor || trench.is_some_and(|t| t == *r) {
            continue;
        }
        out.push(Seg { key: SegKey::Win(*id), x0: r.l as f32, x1: r.r as f32, y: r.t as f32, kick: true, bottom: r.b as f32 });
    }
    out
}

const KICKER: Rgba8 = Rgba8([176, 120, 64, 255]);
const KICKER_DARK: Rgba8 = Rgba8([96, 62, 34, 255]);
const POP: Rgba8 = Rgba8([255, 236, 120, 255]);

fn rider_sprite(r: &Rider, t: f32) -> String {
    if r.kind == Kind::Grapple {
        return match r.state {
            State::Waiting { .. } | State::Land { .. } => "grap_run_0".into(),
            State::Ride { .. } => format!("grap_run_{}", ((t * 10.0) as usize + r.id as usize) % 4),
            State::Air { .. } => "grap_run_3".into(),
            State::Swing { .. } => if ((t * 3.0) as usize) % 2 == 0 { "grap_swing_0".into() } else { "grap_swing_2".into() },
            State::Climb { .. } => "grap_climb_0".into(),
            State::Crash { .. } => "grap_fall".into(),
        };
    }
    if r.kind == Kind::Ninja {
        return match r.state {
            State::Waiting { .. } => "ninja_stance".into(),
            State::Ride { .. } if r.fist > 0.0 => "ninja_stand".into(),
            State::Ride { .. } => ["ninja_run_0", "ninja_run_1", "ninja_run_2", "ninja_run_3"][((t * 10.0) as usize + r.id as usize) % 4].into(),
            State::Land { .. } => "ninja_land".into(),
            State::Air { trick: Some((k, left)), .. } => {
                let tk = NINJA_TRICKS[k];
                let f = (((tk.secs - left) / tk.secs) * tk.frames.len() as f32) as usize;
                tk.frames[f.min(tk.frames.len() - 1)].into()
            }
            State::Air { .. } | State::Swing { .. } => if r.vel[1] < 0.0 { "ninja_leap".into() } else { "ninja_air".into() },
            State::Climb { .. } => if ((t * 8.0) as usize) % 2 == 0 { "ninja_cling_0".into() } else { "ninja_cling_1".into() },
            State::Crash { t: ct } => if ct < 0.6 { "ninja_hit".into() } else if ct < 0.9 { "smoke_0".into() } else { "smoke_1".into() },
        };
    }
    match r.state {
        State::Waiting { .. } => "ride_0".into(),
        State::Ride { .. } if r.fist > 0.0 => format!("fist_{}", ((0.8 - r.fist) * 5.0).min(2.0) as usize),
        State::Ride { .. } if r.wheelie > 0.0 => format!("wheelie_{}", ((t * 6.0) as usize + r.id as usize) % 2),
        State::Ride { .. } => ["ride_0", "ride_1", "ride_2"][((t * 12.0) as usize + r.id as usize) % 3].into(),
        State::Land { t: lt, .. } => if lt < 0.15 { "land_0".into() } else { "land_1".into() },
        State::Air { trick: Some((k, left)), .. } => {
            let tk = TRICKS[k];
            let f = (((tk.secs - left) / tk.secs) * tk.frames.len() as f32) as usize;
            tk.frames[f.min(tk.frames.len() - 1)].into()
        }
        State::Air { t: at, .. } => {
            if r.vel[1] < -120.0 {
                "air_up".into()
            } else if r.vel[1] < 160.0 || at < 0.2 {
                "air_mid".into()
            } else {
                "air_down".into()
            }
        }
        State::Swing { .. } | State::Climb { .. } => "air_mid".into(),
        State::Crash { t: ct } => if ct < 0.25 { "crash_0".into() } else if ct < 0.65 { "crash_1".into() } else { "crash_2".into() },
    }
}

/// The ironworker's build pose: (welding?, frame). Sledgehammer swings; every third beat he welds instead.
/// `bt` = time on this build, `t` = game clock. Shared by the sprite and the spark emitter so they agree.
fn iron_pose(bt: f32, t: f32) -> (bool, usize) {
    if (bt / 1.6) as u32 % 3 == 2 {
        (true, ((t * 8.0) as usize) % 3)
    } else {
        (false, ((t * 5.0) as usize) % 4)
    }
}

/// (full sprite name, flip, bob in art px). Sheet poses face right.
fn crew_sprite(c: &Crew, t: f32) -> (String, bool, f32) {
    let ph = ((t * 6.0) as u32 + c.id) % 2 == 0;
    let side = c.job.map(|j| j.1).unwrap_or(1);
    if c.iron {
        let cyc = |n: usize, hz: f32| ((t * hz) as usize) % n;
        let (name, flip): (String, bool) = match c.state {
            CrewState::Idle { .. } => ("iron_stand".into(), false),
            // Hauls the I-beam over to the window.
            CrewState::Walk => (format!("iron_carry_{}", cyc(4, 6.0)), side > 0),
            CrewState::Haul { to } => (format!("iron_carry_{}", cyc(4, 6.0)), to < c.pos[0]),
            CrewState::Climb { .. } => (format!("iron_climb_{}", cyc(4, 5.0)), false),
            CrewState::Build { t: bt } if bt < 0.4 => ("iron_stand".into(), side < 0),
            // Sledgehammer swings; every third beat he welds instead.
            CrewState::Build { t: bt } => match iron_pose(bt, t) {
                (true, f) => (format!("iron_weld_{f}"), side < 0),
                (false, f) => (format!("iron_sledge_{f}"), side < 0),
            },
            CrewState::Fall { vx, .. } => (format!("iron_fall_{}", u32::from(!ph)), vx < 0.0),
            CrewState::Down { hard: true, .. } => ("iron_kneel".into(), false),
            CrewState::Down { .. } => ("iron_flex".into(), false),
        };
        return (name, flip, 0.0);
    }
    let pre = ["a", "b", "c"][c.id as usize % 3];
    let (name, flip, bob) = match c.state {
        CrewState::Idle { .. } => ("crouch", c.id % 2 == 1, 0.0),
        CrewState::Walk => (if ph { "crouch" } else { "jump_0" }, side > 0, if ph { 0.0 } else { 3.0 }),
        CrewState::Haul { to } => (if ph { "crouch" } else { "jump_0" }, to < c.pos[0], if ph { 0.0 } else { 3.0 }),
        // On the wall: the window is the wall, so face it.
        CrewState::Climb { .. } => (["climb_0", "climb_1", "climb_3", "climb_4"][((t * 5.0) as usize + c.id as usize) % 4], side > 0, 0.0),
        CrewState::Build { t: bt } if bt < 0.4 => ("mantle", side < 0, 0.0),
        CrewState::Build { .. } => (if ((t * 4.0) as u32 + c.id) % 2 == 0 { "reach_0" } else { "reach_1" }, side < 0, 0.0),
        CrewState::Fall { vx, .. } => (if ph { "fall_0" } else { "fall_1" }, vx < 0.0, 0.0),
        CrewState::Down { hard: true, .. } => ("fallen", false, 0.0),
        CrewState::Down { .. } => (if ph { "cheer_0" } else { "cheer_1" }, false, 0.0),
    };
    (format!("crew{pre}_{name}"), flip, bob)
}

/// One frame: every underlay (monitor-local coords) plus the TRENCH track (bar-local coords).
pub fn frame(m: &Moto, mons: &[MonitorInfo], trench: Option<RectPx>, segs: &[Seg], sp: &Sprites) -> Vec<DrawList> {
    let s = m.s;
    let flip = m.dir() < 0.0;
    let mut surfaces: Vec<(SurfaceId, RectPx)> = mons.iter().map(|mo| (SurfaceId::Underlay(mo.id), mo.bounds)).collect();
    if let Some(t) = trench {
        surfaces.push((SurfaceId::Trench, t));
    }
    let mut out = Vec::new();
    for (sid, b) in surfaces {
        let (ox, oy) = (b.l as f32, b.t as f32);
        let near = |x: f32, y: f32, pad: f32| x >= b.l as f32 - pad && x < b.r as f32 + pad && y >= b.t as f32 - pad && y < b.b as f32 + pad;
        let mut c = Vec::new();
        let giant_here = match sid {
            SurfaceId::Underlay(id) => id.0 == m.giant.floor,
            SurfaceId::Trench => mons.iter().any(|mo| mo.id.0 == m.giant.floor && mo.bounds.contains((b.l + b.r) / 2, (b.t + b.b) / 2)),
            _ => false,
        };
        let draw_giant = |c: &mut Vec<DrawCmd>| {
            let g = &m.giant;
            if !giant_here || matches!(g.state, GiantState::Gone { .. }) {
                return;
            }
            let name = match g.state {
                GiantState::Watch { .. } => "giant_idle_00".to_string(),
                _ => format!("giant_walk_{:02}", ((g.phase * GIANT_FRAMES as f32) as usize).min(GIANT_FRAMES - 1)),
            };
            sp_draw(sp, c, &name, g.pos[0] - ox, g.pos[1] - oy, g.face < 0.0 && !matches!(g.state, GiantState::Watch { .. }), m.base_s);
        };
        if sid == SurfaceId::Trench {
            let (w, h) = (b.width() as f32, b.height() as f32);
            c.push(DrawCmd::Fill { rect: [0.0, 0.0, w, h], color: Rgba8([34, 26, 22, 255]) });
            draw_giant(&mut c);
            let mut x = 0.0;
            while x < w {
                sp_draw(sp, &mut c, "tile_earth", x + 58.0 * m.base_s, h, false, m.base_s);
                x += 116.0 * m.base_s;
            }
            c.push(DrawCmd::Fill { rect: [0.0, h - 6.0 * m.base_s, w, h - 4.0 * m.base_s], color: KICKER_DARK });
            let sc = m.score;
            c.push(DrawCmd::Text {
                s: format!(
                    "MOTO TEST   score {}   best line {}   tricks {}   crashes {}   ate it {}   kickers {}   throttle {:.0}%",
                    sc.total,
                    sc.best_line,
                    sc.tricks,
                    sc.crashes,
                    sc.eaten,
                    m.kickers.values().filter(|p| **p >= 1.0).count(),
                    m.throttle * 100.0
                ),
                x: 16.0 * m.base_s,
                y: 8.0 * m.base_s,
                size: 16.0 * m.base_s,
                color: Rgba8([255, 230, 200, 255]),
            });
        } else {
            draw_giant(&mut c);
            // Kickers: the crew's work. Uprights go in first (from the lip inward), the deck goes on when it's done.
            for ((w, side), prog) in &m.kickers {
                let Some(sg) = segs.iter().find(|g| g.key == SegKey::Win(*w)) else { continue };
                let lip = if *side < 0 { sg.x0 } else { sg.x1 };
                if !near(lip, sg.y, 0.0) {
                    continue;
                }
                let (lx, ly) = (lip - ox, sg.y - oy);
                let back = -(*side as f32) * 30.0 * s;
                let rise = 12.0 * s;
                let posts = (prog * 5.0).ceil() as usize;
                for k in 0..posts.min(5) {
                    let f = k as f32 / 4.0;
                    c.push(DrawCmd::Polyline { pts: vec![[lx + back * f, ly - 1.0], [lx + back * f, ly - rise * (1.0 - f)]], width: 2.0 * s, color: KICKER_DARK, glow: false });
                }
                if *prog >= 1.0 {
                    c.push(DrawCmd::Polyline { pts: vec![[lx + back, ly - 1.0], [lx, ly - rise]], width: 3.0 * s, color: KICKER, glow: false });
                    c.push(DrawCmd::Polyline { pts: vec![[lx, ly - rise], [lx, ly - 1.0]], width: 2.0 * s, color: KICKER, glow: false });
                }
            }
            // Spiders on their silk, hanging under window bottoms.
            for spd in &m.spiders {
                let Some(([x, y], top)) = Moto::spider_pos(spd, segs) else { continue };
                if !near(x, y, 40.0 * s) {
                    continue;
                }
                c.push(DrawCmd::Polyline { pts: vec![[x - ox, top - oy], [x - ox, y - oy - 14.0 * s]], width: 1.0 * s.max(1.0), color: Rgba8([235, 235, 245, 200]), glow: false });
                let name = match spd.state {
                    SpiderState::Up { .. } => ["spider_climb_0", "spider_climb_1"][((m.t * 6.0) as usize) % 2],
                    _ => ["spider_hang_0", "spider_hang_3"][((m.t * 2.0) as usize + spd.id as usize) % 2],
                };
                sp_draw(sp, &mut c, name, x - ox, y - oy, false, s);
            }
            // The chameleon on its window top, fading in and out of sight.
            for cham in &m.chams {
            if let (Some(w), st) = (cham.window, cham.state) {
                if let Some(g) = segs.iter().find(|g| g.key == SegKey::Win(w)) {
                    let x = g.x0 + cham.x * (g.x1 - g.x0);
                    if near(x, g.y, 40.0 * s) {
                        let (name, a) = match st {
                            ChamState::FadeIn { t } => ("cham_idle", (t / 1.2 * 255.0) as u8),
                            ChamState::FadeOut { t } => ("cham_idle", ((1.0 - t / 1.2) * 255.0) as u8),
                            ChamState::Tongue { t } => (if t < 0.3 || t > 0.6 { "cham_tongue_0" } else { "cham_tongue_1" }, 255),
                            ChamState::Walk { .. } => (["cham_walk_0", "cham_walk_1"][((m.t * 4.0) as usize) % 2], 255),
                            ChamState::Gone { .. } => ("", 0),
                        };
                        if a > 0 {
                            if let Some(id) = sp.get(name) {
                                c.push(DrawCmd::Sprite { id, frame: 0, x: x - ox, y: g.y - oy, flip_x: cham.face < 0.0, alpha: a, scale: s });
                            }
                        }
                    }
                }
            }
            }
            for cr in &m.crew {
                if !near(cr.pos[0], cr.pos[1], 50.0 * s) {
                    continue;
                }
                let (full, flip, bob) = crew_sprite(cr, m.t);
                if !sp_draw(sp, &mut c, &full, cr.pos[0] - ox, cr.pos[1] - oy - bob * s, flip, s) {
                    c.push(DrawCmd::Fill { rect: [cr.pos[0] - ox - 5.0 * s, cr.pos[1] - oy - 30.0 * s, cr.pos[0] - ox + 5.0 * s, cr.pos[1] - oy], color: Rgba8([90, 200, 210, 255]) });
                }
            }
        }
        for r in &m.riders {
            if matches!(r.state, State::Waiting { .. }) || !near(r.pos[0], r.pos[1], 70.0 * s) {
                continue;
            }
            if let State::Swing { win, frac, .. } = r.state {
                if let Some(g) = segs.iter().find(|g| g.key == SegKey::Win(win)) {
                    let a = [g.x0 + frac * (g.x1 - g.x0), g.bottom];
                    c.push(DrawCmd::Polyline { pts: vec![[r.pos[0] - ox, r.pos[1] - oy - HAND * s], [a[0] - ox, a[1] - oy]], width: 1.5 * s, color: Rgba8([225, 230, 240, 230]), glow: false });
                }
            }
            let name = rider_sprite(r, m.t);
            if !sp_draw(sp, &mut c, &name, r.pos[0] - ox, r.pos[1] - oy, flip, s) {
                c.push(DrawCmd::Fill { rect: [r.pos[0] - ox - 16.0 * s, r.pos[1] - oy - 30.0 * s, r.pos[0] - ox + 16.0 * s, r.pos[1] - oy], color: Rgba8([255, 140, 40, 255]) });
            }
        }
        // Sparks, bucketed by heat: white-hot, orange, cooling red.
        let mut heat: [Vec<[f32; 3]>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for p in &m.sparks {
            if !near(p[0], p[1], 0.0) {
                continue;
            }
            let k = p[4] / p[5];
            let (b, size) = if k > 0.66 { (0, 2.2) } else if k > 0.33 { (1, 1.8) } else { (2, 1.4) };
            heat[b].push([p[0] - ox, p[1] - oy, size * m.base_s.max(1.0)]);
        }
        for (pts, col) in heat.into_iter().zip([Rgba8([255, 250, 210, 255]), Rgba8([255, 180, 60, 240]), Rgba8([220, 70, 30, 200])]) {
            if !pts.is_empty() {
                c.push(DrawCmd::Particles { pts, color: col });
            }
        }
        for p in &m.popups {
            if near(p.pos[0], p.pos[1], 0.0) {
                let a = (255.0 * (p.ttl / 0.5).min(1.0)) as u8;
                c.push(DrawCmd::Text { s: p.text.clone(), x: p.pos[0] - ox - 60.0 * s, y: p.pos[1] - oy, size: 15.0 * s, color: Rgba8([POP.0[0], POP.0[1], POP.0[2], a]) });
            }
        }
        out.push(DrawList { surface: sid, dirty: Vec::new(), cmds: c });
    }
    out
}

fn sp_draw(sp: &Sprites, c: &mut Vec<DrawCmd>, name: &str, x: f32, y: f32, flip: bool, s: f32) -> bool {
    match sp.get(name) {
        Some(id) => {
            c.push(DrawCmd::Sprite { id, frame: 0, x, y, flip_x: flip, alpha: 255, scale: s });
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bezel_core::MonitorId;

    fn mon(id: u64, l: i32, t: i32, r: i32, b: i32) -> MonitorInfo {
        MonitorInfo { id: MonitorId(id), bounds: RectPx::new(l, t, r, b), work: RectPx::new(l, t, r, b - 40), dpi: 96, primary: id == 1 }
    }

    /// Knox's desk: portrait left, home, one above-right.
    fn desk() -> Vec<MonitorInfo> {
        vec![mon(0, -1440, -389, 0, 2171), mon(1, 0, 0, 2560, 1440), mon(2, 1360, -1440, 3920, 0)]
    }

    fn run(m: &mut Moto, segs: &[Seg], secs: f32) {
        for _ in 0..(secs * HZ) as u32 {
            m.step(segs, 0.0);
        }
    }

    #[test]
    fn starts_top_monitor_heading_left_and_crosses_every_monitor() {
        let mons = desk();
        let segs = segments(&mons, None, &HashMap::new(), 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 1, 7);
        assert_eq!(m.dir(), -1.0);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..(40.0 * HZ) as u32 {
            m.step(&segs, 0.0);
            if let State::Ride { seg: SegKey::Floor(id), .. } = m.riders[0].state {
                seen.insert(id);
            }
        }
        assert_eq!(seen.len(), 3, "rode the floor of all three monitors: {seen:?}");
        assert!(m.score.laps >= 1);
    }

    #[test]
    fn window_kicker_launches_and_tricks_score() {
        let mons = desk();
        let mut w = HashMap::new();
        w.insert(ExtWindowId(9), RectPx::new(1200, 700, 2200, 1300));
        let segs = segments(&mons, None, &w, 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 1, 11);
        run(&mut m, &segs, 60.0);
        assert!(m.score.tricks + m.score.crashes > 0, "{:?}", m.score);
    }

    #[test]
    fn crew_builds_the_lip_kicker_then_riders_pop_off_it() {
        let mons = desk();
        let mut w = HashMap::new();
        w.insert(ExtWindowId(9), RectPx::new(1200, 700, 2200, 1300));
        let segs = segments(&mons, None, &w, 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 1, 5);
        run(&mut m, &segs, 30.0);
        assert_eq!(m.kickers.get(&(ExtWindowId(9), -1)), Some(&1.0), "heading left, so the left end gets the kicker");
        assert!(m.score.kickers_built >= 1);
        // Riders leaving that window top now launch upward.
        m.riders[0].pos = [1202.0, 700.0];
        m.riders[0].state = State::Ride { seg: SegKey::Win(ExtWindowId(9)), y_ref: 700.0 };
        m.step(&segs, 0.0);
        assert!(matches!(m.riders[0].state, State::Air { .. }) && m.riders[0].vel[1] < 0.0);
    }

    #[test]
    fn dragging_the_window_knocks_the_climber_off() {
        let mons = desk();
        let mut w = HashMap::new();
        w.insert(ExtWindowId(9), RectPx::new(1200, 400, 2200, 1300));
        let segs = segments(&mons, None, &w, 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 0, 5);
        for _ in 0..(20.0 * HZ) as u32 {
            m.step(&segs, 0.0);
            if m.crew.iter().any(|c| matches!(c.state, CrewState::Climb { .. })) {
                break;
            }
        }
        assert!(m.crew.iter().any(|c| matches!(c.state, CrewState::Climb { .. })), "someone started climbing");
        w.insert(ExtWindowId(9), RectPx::new(1300, 400, 2300, 1300));
        let segs = segments(&mons, None, &w, 1.0);
        m.step(&segs, 0.0);
        assert!(m.score.crew_falls >= 1);
        assert!(m.crew.iter().any(|c| matches!(c.state, CrewState::Fall { hard: true, .. })));
    }

    #[test]
    fn giant_walks_a_floor_and_moves_on() {
        let mons = desk();
        let segs = segments(&mons, None, &HashMap::new(), 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 0, 21);
        let mut floors = std::collections::HashSet::new();
        let mut walked = 0;
        for _ in 0..(180.0 * HZ) as u32 {
            m.step(&segs, 0.0);
            if m.giant.state == GiantState::Walk {
                walked += 1;
                floors.insert(m.giant.floor);
                let f = segs.iter().find(|g| g.key == SegKey::Floor(m.giant.floor)).unwrap();
                assert_eq!(m.giant.pos[1], f.y, "feet on his floor");
            }
        }
        assert!(walked > 0 && floors.len() >= 2, "walked on {floors:?}");
    }

    /// A window high on the home monitor with lots of bare desktop under it.
    fn high_window() -> HashMap<ExtWindowId, RectPx> {
        let mut w = HashMap::new();
        w.insert(ExtWindowId(9), RectPx::new(600, 200, 2000, 500));
        w
    }

    #[test]
    fn a_spider_drops_from_a_window_and_a_rider_hitting_it_crashes() {
        let mons = desk();
        let segs = segments(&mons, None, &high_window(), 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 1, 9);
        let mut hanging = None;
        for _ in 0..(30.0 * HZ) as u32 {
            m.step(&segs, 0.0);
            if let Some(sp) = m.spiders.iter().find(|sp| matches!(sp.state, SpiderState::Hang { .. })) {
                hanging = Moto::spider_pos(sp, &segs).map(|p| p.0);
                break;
            }
        }
        let [x, y] = hanging.expect("a spider came down");
        assert!(y > 500.0 && y < 1400.0, "hangs below the window, above the floor: {y}");
        let before = m.score.spider_hits;
        m.riders[0].pos = [x, y + 10.0 * m.s];
        m.riders[0].vel = [-400.0, 0.0];
        m.riders[0].state = State::Air { t: 0.5, trick: None, done: 0 };
        m.step(&segs, 0.0);
        assert_eq!(m.score.spider_hits, before + 1);
        assert!(matches!(m.riders[0].state, State::Crash { .. }));
    }

    #[test]
    fn ninja_runs_with_the_bikes_and_pops_off_edges_without_kickers() {
        let mons = desk();
        let segs = segments(&mons, None, &high_window(), 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 1, 4);
        assert_eq!(m.riders.iter().filter(|r| r.kind == Kind::Ninja).count(), 1);
        let n = m.riders.iter().position(|r| r.kind == Kind::Ninja).unwrap();
        m.riders[n].pos = [602.0, 200.0];
        m.riders[n].state = State::Ride { seg: SegKey::Win(ExtWindowId(9)), y_ref: 200.0 };
        m.step(&segs, 0.0);
        assert!(matches!(m.riders[n].state, State::Air { .. }) && m.riders[n].vel[1] < 0.0, "no kicker there, he jumps anyway");
    }

    #[test]
    fn chameleon_turns_up_on_a_window_and_the_ironworker_builds() {
        let mons = desk();
        let segs = segments(&mons, None, &high_window(), 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 0, 13);
        let (mut cham_seen, mut iron_built, mut most_sparks) = (false, false, 0);
        for _ in 0..(60.0 * HZ) as u32 {
            m.step(&segs, 0.0);
            most_sparks = most_sparks.max(m.sparks.len());
            cham_seen |= m.chams.iter().any(|c| matches!(c.state, ChamState::Walk { .. }) && c.window == Some(ExtWindowId(9)));
            iron_built |= m.crew.iter().any(|c| c.iron && matches!(c.state, CrewState::Build { .. }));
        }
        assert!(cham_seen && iron_built, "cham {cham_seen} iron {iron_built}");
        assert!(most_sparks > 60, "the ironworker throws lots of sparks: {most_sparks}");
    }

    #[test]
    fn rope_wrist_man_hooks_a_window_bottom_and_swings_on() {
        let mons = desk();
        let segs = segments(&mons, None, &high_window(), 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 1, 17);
        let g = m.riders.iter().position(|r| r.kind == Kind::Grapple).expect("rope-wrist man is on the course");
        // Mid-air under and behind the window (heading left), moving left.
        m.riders[g].pos = [2300.0, 1000.0];
        m.riders[g].vel = [-400.0, -200.0];
        m.riders[g].state = State::Air { t: 0.2, trick: None, done: 0 };
        m.step(&segs, 0.0);
        assert!(matches!(m.riders[g].state, State::Swing { win: ExtWindowId(9), .. }), "{:?}", m.riders[g].state);
        let tricks = m.score.tricks;
        for _ in 0..(3.0 * HZ) as u32 {
            m.step(&segs, 0.0);
            if !matches!(m.riders[g].state, State::Swing { .. }) {
                break;
            }
        }
        assert!(!matches!(m.riders[g].state, State::Swing { .. }), "lets go");
        assert!(m.score.tricks > tricks || matches!(m.riders[g].state, State::Land { .. }), "scored the swing or landed off it");
    }

    #[test]
    fn full_ironworker_crew_is_always_out_working() {
        let mons = desk();
        let segs = segments(&mons, None, &high_window(), 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 0, 23);
        assert_eq!(m.crew.len(), IRON_CREW as usize);
        assert!(m.crew.iter().all(|c| c.iron));
        run(&mut m, &segs, 40.0);
        // One window, one kicker: it's built, and after that nobody stands idle for long.
        let mut busy = 0;
        let mut ticks = 0;
        for _ in 0..(20.0 * HZ) as u32 {
            m.step(&segs, 0.0);
            ticks += m.crew.len();
            busy += m.crew.iter().filter(|c| !matches!(c.state, CrewState::Idle { .. })).count();
        }
        assert!(m.score.kickers_built >= 1);
        assert!(busy * 10 >= ticks * 6, "crew busy {busy}/{ticks}");
        assert!(m.crew.iter().filter(|c| c.job.is_some()).count() <= 4, "two per kicker at most");
    }

    #[test]
    fn ninja_climbs_a_window_he_runs_into() {
        let mons = desk();
        let mut w = HashMap::new();
        // Sits near the home floor (1400), so its right side is a wall a running ninja hits.
        w.insert(ExtWindowId(9), RectPx::new(800, 900, 1600, 1350));
        let segs = segments(&mons, None, &w, 1.0);
        let mut climbed = false;
        for seed in 1..20u64 {
            let mut m = Moto::new(&mons, &segs, 1.0, 1, seed);
            let n = m.riders.iter().position(|r| r.kind == Kind::Ninja).unwrap();
            m.riders[n].pos = [1700.0, 1400.0];
            m.riders[n].state = State::Ride { seg: SegKey::Floor(1), y_ref: 1400.0 };
            for _ in 0..(4.0 * HZ) as u32 {
                m.step(&segs, 0.0);
                if let State::Ride { seg: SegKey::Win(ExtWindowId(9)), .. } = m.riders[n].state {
                    climbed = true;
                    assert_eq!(m.riders[n].pos[1], 900.0, "on top of the window");
                    break;
                }
            }
            if climbed {
                break;
            }
        }
        assert!(climbed, "a ninja scaled the wall");
    }

    #[test]
    fn chaos_scales_the_whole_cast() {
        let mons = desk();
        let segs = segments(&mons, None, &high_window(), 1.0);
        let m = Moto::with_chaos(&mons, &segs, 1.0, 3 * CHAOS, 1, CHAOS);
        let count = |k: Kind| m.riders.iter().filter(|r| r.kind == k).count() as u32;
        let c = CHAOS;
        assert_eq!((count(Kind::Bike), count(Kind::Ninja), count(Kind::Grapple)), (3 * c, c, c));
        assert_eq!((m.spiders.len() as u32, m.chams.len() as u32, m.crew.len() as u32), (2 * c, c, IRON_CREW * c.min(2)));
    }

    #[test]
    fn moving_the_ridden_window_throws_the_rider() {
        let mons = desk();
        let mut w = HashMap::new();
        w.insert(ExtWindowId(9), RectPx::new(200, 700, 2400, 1300));
        let segs = segments(&mons, None, &w, 1.0);
        let mut m = Moto::new(&mons, &segs, 1.0, 1, 3);
        m.riders[0].pos = [1500.0, 700.0];
        m.riders[0].state = State::Ride { seg: SegKey::Win(ExtWindowId(9)), y_ref: 700.0 };
        m.step(&segs, 0.0);
        w.insert(ExtWindowId(9), RectPx::new(200, 760, 2400, 1360));
        let segs = segments(&mons, None, &w, 1.0);
        m.step(&segs, 0.0);
        assert!(matches!(m.riders[0].state, State::Crash { .. }));
        assert_eq!(m.score.eaten, 1);
    }
}
