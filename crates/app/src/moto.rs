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
    /// Window tops get kickers at both ends; floors drop off flat.
    pub kick: bool,
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
}

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
}

pub struct Moto {
    pub riders: Vec<Rider>,
    pub popups: Vec<Popup>,
    pub score: Score,
    /// Scale: physical px per art px.
    pub s: f32,
    start: [f32; 2],
    dir: f32,
    world: RectPx,
    rng: u64,
    pub t: f32,
    pub throttle: f32,
}

impl Moto {
    /// Start on the highest monitor, riding toward the middle of the desk.
    pub fn new(mons: &[MonitorInfo], floors: &[Seg], s: f32, riders: u32, seed: u64) -> Moto {
        let world = mons.iter().skip(1).fold(mons[0].bounds, |a, m| a.union(&m.bounds));
        let top = mons.iter().min_by_key(|m| m.bounds.t).expect("a monitor");
        let mid = (world.l + world.r) as f32 / 2.0;
        let tcx = (top.bounds.l + top.bounds.r) as f32 / 2.0;
        let dir = if tcx > mid { -1.0 } else { 1.0 };
        let fy = floors.iter().find(|f| f.key == SegKey::Floor(top.id.0)).map(|f| f.y).unwrap_or(top.work.b as f32);
        let sx = if dir < 0.0 { top.bounds.r as f32 - 30.0 * s } else { top.bounds.l as f32 + 30.0 * s };
        let riders = (0..riders)
            .map(|i| Rider { id: i, pos: [sx, fy], vel: [0.0, 0.0], state: State::Waiting { t: 0.5 + i as f32 * 3.5 }, wheelie: 0.0, fist: 0.0, combo: 0, line_pts: 0 })
            .collect();
        Moto { riders, popups: Vec::new(), score: Score::default(), s, start: [sx, fy], dir, world, rng: seed | 1, t: 0.0, throttle: 0.0 }
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
        for i in 0..self.riders.len() {
            let mut r = self.riders[i].clone();
            self.step_rider(&mut r, segs);
            self.riders[i] = r;
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
        r.vel = [self.dir * self.speed(), if kick { -self.kick() } else { 0.0 }];
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
                            r.vel = [self.dir * self.speed(), 0.0];
                            r.state = State::Air { t: 0.0, trick: None, done: 0 };
                        }
                    }
                }
            }
            State::Ride { seg, y_ref } | State::Land { seg, y_ref, .. } => {
                let Some(s) = Self::seg(segs, seg).copied() else {
                    r.vel = [self.dir * self.speed(), 0.0];
                    r.state = State::Air { t: 0.0, trick: None, done: 0 };
                    return;
                };
                if (s.y - y_ref).abs() > 6.0 {
                    // The window under the rider moved: it threw him.
                    self.score.eaten += 1;
                    self.crash(r, "ATE IT");
                    return;
                }
                r.pos[0] += self.dir * self.speed() * DT;
                r.pos[1] = s.y;
                r.fist = (r.fist - DT).max(0.0);
                if let State::Land { t, .. } = r.state {
                    r.state = if t + DT > 0.3 { State::Ride { seg, y_ref: s.y } } else { State::Land { t: t + DT, seg, y_ref: s.y } };
                } else {
                    r.state = State::Ride { seg, y_ref: s.y };
                    if r.wheelie > 0.0 {
                        r.wheelie -= DT;
                    } else if self.rnd() < DT * 0.12 {
                        r.wheelie = 1.0 + self.rnd() * 1.2;
                    }
                }
                let off = if self.dir < 0.0 { r.pos[0] < s.x0 } else { r.pos[0] > s.x1 };
                if off {
                    self.launch(r, s.kick);
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
                        let tk = TRICKS[k];
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
                    let fits: Vec<usize> = (0..TRICKS.len()).filter(|&k| greedy || TRICKS[k].secs < air).collect();
                    if !fits.is_empty() && air > 0.3 && done < 2 {
                        // Lean toward the biggest trick that fits.
                        let pick = fits[(fits.len() as f32 * (0.4 + 0.6 * self.rnd())).min(fits.len() as f32 - 1.0) as usize];
                        trick = Some((pick, TRICKS[pick].secs));
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
            State::Crash { t } => {
                let t = t + DT;
                if t > 2.0 {
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
        out.push(Seg { key: SegKey::Floor(m.id.0), x0: m.bounds.l as f32, x1: m.bounds.r as f32, y, kick: false });
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
        out.push(Seg { key: SegKey::Win(*id), x0: r.l as f32, x1: r.r as f32, y: r.t as f32, kick: true });
    }
    out
}

const KICKER: Rgba8 = Rgba8([176, 120, 64, 255]);
const KICKER_DARK: Rgba8 = Rgba8([96, 62, 34, 255]);
const POP: Rgba8 = Rgba8([255, 236, 120, 255]);

fn rider_sprite(r: &Rider, t: f32) -> String {
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
        State::Crash { t: ct } => if ct < 0.25 { "crash_0".into() } else if ct < 0.65 { "crash_1".into() } else { "crash_2".into() },
    }
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
        if sid == SurfaceId::Trench {
            let (w, h) = (b.width() as f32, b.height() as f32);
            c.push(DrawCmd::Fill { rect: [0.0, 0.0, w, h], color: Rgba8([34, 26, 22, 255]) });
            let mut x = 0.0;
            while x < w {
                sp_draw(sp, &mut c, "tile_earth", x + 58.0 * s, h, false, s);
                x += 116.0 * s;
            }
            c.push(DrawCmd::Fill { rect: [0.0, h - 6.0 * s, w, h - 4.0 * s], color: KICKER_DARK });
            let sc = m.score;
            c.push(DrawCmd::Text {
                s: format!("MOTO TEST   score {}   best line {}   tricks {}   crashes {}   ate it {}   throttle {:.0}%", sc.total, sc.best_line, sc.tricks, sc.crashes, sc.eaten, m.throttle * 100.0),
                x: 16.0 * s,
                y: 8.0 * s,
                size: 16.0 * s,
                color: Rgba8([255, 230, 200, 255]),
            });
        } else {
            // Kickers at both ends of every window top on this monitor, facing the direction of travel.
            for sg in segs.iter().filter(|g| g.kick) {
                let lip = if m.dir() < 0.0 { sg.x0 } else { sg.x1 };
                if !near(lip, sg.y, 0.0) {
                    continue;
                }
                let (lx, ly) = (lip - ox, sg.y - oy);
                let back = -m.dir() * 30.0 * s;
                let rise = 12.0 * s;
                for k in 0..5 {
                    let f = k as f32 / 4.0;
                    c.push(DrawCmd::Polyline { pts: vec![[lx + back * f, ly - 1.0], [lx + back * f, ly - rise * (1.0 - f)]], width: 2.0 * s, color: KICKER_DARK, glow: false });
                }
                c.push(DrawCmd::Polyline { pts: vec![[lx + back, ly - 1.0], [lx, ly - rise]], width: 3.0 * s, color: KICKER, glow: false });
                c.push(DrawCmd::Polyline { pts: vec![[lx, ly - rise], [lx, ly - 1.0]], width: 2.0 * s, color: KICKER, glow: false });
            }
        }
        for r in &m.riders {
            if matches!(r.state, State::Waiting { .. }) || !near(r.pos[0], r.pos[1], 70.0 * s) {
                continue;
            }
            let name = rider_sprite(r, m.t);
            if !sp_draw(sp, &mut c, &name, r.pos[0] - ox, r.pos[1] - oy, flip, s) {
                c.push(DrawCmd::Fill { rect: [r.pos[0] - ox - 16.0 * s, r.pos[1] - oy - 30.0 * s, r.pos[0] - ox + 16.0 * s, r.pos[1] - oy], color: Rgba8([255, 140, 40, 255]) });
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
