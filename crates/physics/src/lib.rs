//! Rope verlet, grapple, falls, platform landing. (ADR-0001 §8)
//!
//! Units are physical pixels and seconds; +y points down (screen space).

use bezel_core::{ExtWindowId, Platform, Px};

/// Gravity in px/s². Tuned for miniature scale, not real metres.
pub const GRAVITY: f32 = 1800.0;
/// Terminal fall speed without a parachute, px/s.
pub const TERMINAL: f32 = 1400.0;
/// Terminal fall speed under a parachute, px/s.
pub const CHUTE_TERMINAL: f32 = 120.0;
/// Largest internal step, so stability doesn't depend on the caller's dt.
const MAX_SUBSTEP: f32 = 1.0 / 60.0;
const ROPE_ITERATIONS: usize = 24;
const ROPE_DAMPING: f32 = 0.985;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RopeId(pub u32);

#[derive(Clone, Debug, PartialEq)]
struct Rope {
    pos: Vec<[f32; 2]>,
    prev: Vec<[f32; 2]>,
    seg_len: f32,
    start_pin: Option<[f32; 2]>,
    end_pin: Option<[f32; 2]>,
}

/// Verlet rope chains on a fixed internal step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RopeWorld {
    ropes: Vec<Rope>,
}

impl RopeWorld {
    pub fn new() -> Self {
        Self::default()
    }

    /// A straight rope from `anchor` (pinned) to `end` (free), split into `segments` (at least 1).
    pub fn spawn(&mut self, anchor: [f32; 2], end: [f32; 2], segments: u8) -> RopeId {
        let n = segments.max(1) as usize;
        let pos: Vec<[f32; 2]> = (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32;
                [anchor[0] + (end[0] - anchor[0]) * t, anchor[1] + (end[1] - anchor[1]) * t]
            })
            .collect();
        let len = ((end[0] - anchor[0]).powi(2) + (end[1] - anchor[1]).powi(2)).sqrt();
        self.ropes.push(Rope { prev: pos.clone(), pos, seg_len: (len / n as f32).max(0.5), start_pin: Some(anchor), end_pin: None });
        RopeId((self.ropes.len() - 1) as u32)
    }

    /// Pin the free end (a rope strung between two points).
    pub fn pin_end(&mut self, r: RopeId, at: [f32; 2]) {
        if let Some(rope) = self.ropes.get_mut(r.0 as usize) {
            rope.end_pin = Some(at);
            let last = rope.pos.len() - 1;
            rope.pos[last] = at;
            rope.prev[last] = at;
        }
    }

    /// Snap: release both pins. The rope keeps its momentum and falls.
    pub fn cut(&mut self, r: RopeId) {
        if let Some(rope) = self.ropes.get_mut(r.0 as usize) {
            rope.start_pin = None;
            rope.end_pin = None;
        }
    }

    pub fn step(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let n = (dt / MAX_SUBSTEP).ceil().max(1.0) as usize;
        let h = dt / n as f32;
        for _ in 0..n {
            for rope in &mut self.ropes {
                step_rope(rope, h);
            }
        }
    }

    /// Node positions, anchor first. Empty for an unknown id.
    pub fn nodes(&self, r: RopeId) -> &[[f32; 2]] {
        self.ropes.get(r.0 as usize).map(|rp| rp.pos.as_slice()).unwrap_or(&[])
    }

    /// Length along the nodes (for tests and tuning).
    pub fn length(&self, r: RopeId) -> f32 {
        self.nodes(r).windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt()).sum()
    }
}

fn step_rope(rope: &mut Rope, h: f32) {
    let g = GRAVITY * h * h;
    for (p, q) in rope.pos.iter_mut().zip(rope.prev.iter_mut()) {
        let v = [(p[0] - q[0]) * ROPE_DAMPING, (p[1] - q[1]) * ROPE_DAMPING];
        *q = *p;
        p[0] += v[0];
        p[1] += v[1] + g;
    }
    let last = rope.pos.len() - 1;
    for _ in 0..ROPE_ITERATIONS {
        if let Some(a) = rope.start_pin {
            rope.pos[0] = a;
        }
        if let Some(b) = rope.end_pin {
            rope.pos[last] = b;
        }
        for i in 0..last {
            let (a, b) = (rope.pos[i], rope.pos[i + 1]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let d = (dx * dx + dy * dy).sqrt().max(1e-6);
            let diff = (d - rope.seg_len) / d;
            // Pinned nodes don't move; their neighbour takes the whole correction.
            let wa = if (i == 0 && rope.start_pin.is_some()) || (i == last && rope.end_pin.is_some()) { 0.0 } else { 1.0 };
            let wb = if (i + 1 == last && rope.end_pin.is_some()) || (i + 1 == 0 && rope.start_pin.is_some()) { 0.0 } else { 1.0 };
            let sum = wa + wb;
            if sum == 0.0 {
                continue;
            }
            rope.pos[i][0] += dx * diff * wa / sum;
            rope.pos[i][1] += dy * diff * wa / sum;
            rope.pos[i + 1][0] -= dx * diff * wb / sum;
            rope.pos[i + 1][1] -= dy * diff * wb / sum;
        }
    }
    if let Some(a) = rope.start_pin {
        rope.pos[0] = a;
    }
    if let Some(b) = rope.end_pin {
        rope.pos[last] = b;
    }
}

/// A falling unit. `pos` is its feet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallBody {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub chute: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    Platform(ExtWindowId),
    Bedrock,
    StillFalling,
}

/// Falls through bare cells; lands on the first platform below (D24) or on bedrock at `bedrock_y` (the trench top).
/// Landing is swept over the whole step, so a fast fall can't tunnel through a platform.
pub fn fall_step(b: &mut FallBody, dt: f32, platforms: &[Platform], bedrock_y: Px) -> Landing {
    let term = if b.chute { CHUTE_TERMINAL } else { TERMINAL };
    b.vel[1] = (b.vel[1] + GRAVITY * dt).min(term);
    if b.chute {
        b.vel[0] *= (1.0 - 3.0 * dt).max(0.0); // canopy drag
    }
    let y0 = b.pos[1];
    let y1 = y0 + b.vel[1] * dt;
    let x = b.pos[0] + b.vel[0] * dt;
    b.pos[0] = x;
    let hit = platforms
        .iter()
        .filter(|p| x >= p.x0 as f32 && x < p.x1 as f32)
        .filter(|p| (p.y as f32) >= y0 && (p.y as f32) <= y1)
        .min_by_key(|p| p.y);
    if let Some(p) = hit {
        b.pos[1] = p.y as f32;
        b.vel = [0.0, 0.0];
        return Landing::Platform(p.window);
    }
    if y1 >= bedrock_y as f32 {
        b.pos[1] = bedrock_y as f32;
        b.vel = [0.0, 0.0];
        return Landing::Bedrock;
    }
    b.pos[1] = y1;
    Landing::StillFalling
}

#[cfg(test)]
mod tests {
    use super::*;
    use bezel_core::MonitorId;

    fn plat(w: u64, y: Px, x0: Px, x1: Px) -> Platform {
        Platform { window: ExtWindowId(w), monitor: MonitorId(0), y, x0, x1 }
    }

    fn fall_until(b: &mut FallBody, ps: &[Platform], floor: Px) -> (Landing, u32) {
        for i in 1..10_000 {
            let l = fall_step(b, 1.0 / 20.0, ps, floor);
            if l != Landing::StillFalling {
                return (l, i);
            }
        }
        panic!("never landed");
    }

    #[test]
    fn lands_on_nearest_platform_below() {
        let ps = [plat(1, 800, 0, 400), plat(2, 500, 0, 400), plat(3, 300, 600, 900)];
        let mut b = FallBody { pos: [200.0, 100.0], vel: [0.0, 0.0], chute: false };
        assert_eq!(fall_until(&mut b, &ps, 1300).0, Landing::Platform(ExtWindowId(2)));
        assert_eq!(b.pos[1], 500.0);
    }

    #[test]
    fn ignores_platforms_above_and_to_the_side() {
        let ps = [plat(1, 50, 0, 400), plat(2, 500, 600, 900)];
        let mut b = FallBody { pos: [200.0, 100.0], vel: [0.0, 0.0], chute: false };
        assert_eq!(fall_until(&mut b, &ps, 1300).0, Landing::Bedrock);
        assert_eq!(b.pos[1], 1300.0);
    }

    #[test]
    fn fast_fall_does_not_tunnel() {
        let ps = [plat(7, 500, 0, 400)];
        let mut b = FallBody { pos: [200.0, 480.0], vel: [0.0, TERMINAL], chute: false };
        // One 0.5 s step would carry it 700 px, far past the platform.
        assert_eq!(fall_step(&mut b, 0.5, &ps, 5000), Landing::Platform(ExtWindowId(7)));
    }

    #[test]
    fn chute_caps_speed() {
        let mut b = FallBody { pos: [0.0, 0.0], vel: [80.0, 0.0], chute: true };
        for _ in 0..40 {
            fall_step(&mut b, 0.05, &[], 1_000_000);
        }
        assert!((b.vel[1] - CHUTE_TERMINAL).abs() < 1e-3);
        assert!(b.vel[0] < 80.0, "canopy drag slows drift");
    }

    #[test]
    fn hanging_rope_settles_below_anchor_at_rest_length() {
        let mut w = RopeWorld::new();
        let r = w.spawn([100.0, 100.0], [300.0, 100.0], 16); // horizontal, 200 px
        for _ in 0..200 {
            w.step(1.0 / 20.0);
        }
        let n = w.nodes(r);
        assert_eq!(n[0], [100.0, 100.0]);
        let tip = n[n.len() - 1];
        assert!((tip[0] - 100.0).abs() < 5.0, "tip hangs under the anchor, got {tip:?}");
        assert!(tip[1] > 280.0, "tip near full length below, got {tip:?}");
        let len = w.length(r);
        assert!((len - 200.0).abs() / 200.0 < 0.03, "length {len}");
    }

    #[test]
    fn pinned_rope_keeps_both_ends_and_sags() {
        let mut w = RopeWorld::new();
        let r = w.spawn([0.0, 0.0], [100.0, 0.0], 10);
        w.pin_end(r, [80.0, 0.0]); // slack: 100 px of rope over 80 px
        for _ in 0..200 {
            w.step(1.0 / 20.0);
        }
        let n = w.nodes(r);
        assert_eq!(n[0], [0.0, 0.0]);
        assert_eq!(n[n.len() - 1], [80.0, 0.0]);
        assert!(n[5][1] > 15.0, "middle sags, got {:?}", n[5]);
    }

    #[test]
    fn cut_rope_falls() {
        let mut w = RopeWorld::new();
        let r = w.spawn([0.0, 0.0], [0.0, 100.0], 8);
        for _ in 0..40 {
            w.step(1.0 / 20.0);
        }
        let before: Vec<f32> = w.nodes(r).iter().map(|p| p[1]).collect();
        w.cut(r);
        for _ in 0..10 {
            w.step(1.0 / 20.0);
        }
        for (a, b) in before.iter().zip(w.nodes(r)) {
            assert!(b[1] > a + 50.0, "every node drops after a cut");
        }
    }

    #[test]
    fn deterministic() {
        let run = || {
            let mut w = RopeWorld::new();
            let r = w.spawn([10.0, 10.0], [200.0, 40.0], 12);
            for i in 0..100 {
                if i == 60 {
                    w.cut(r);
                }
                w.step(0.05);
            }
            w
        };
        assert_eq!(run(), run());
    }
}
