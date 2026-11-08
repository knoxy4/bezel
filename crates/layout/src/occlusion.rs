//! Occlusion grid (ADR-0001 §2.2): per-monitor coverage counts over cells of `cell_art_px × art_scale` physical px.
//!
//! A cell is bare when no tracked window overlaps it. Any overlap, even one pixel, covers a cell.
//! `apply` is incremental: work is proportional to the old and new rects' cell areas, never a full rescan.

use std::collections::{HashMap, HashSet};

use bezel_core::{CellId, ExtWindowId, MonitorId, MonitorInfo, Px, RectCause, RectPx};

use crate::platforms::Platform;

/// Cells that changed state (bare ↔ covered) in one `apply`, tagged with the cause (D21).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellDelta {
    pub cause: RectCause,
    pub window: ExtWindowId,
    pub covered: Vec<CellId>,
    pub bared: Vec<CellId>,
}

/// Half-open cell range `[cx0, cx1) × [cy0, cy1)` on one monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellRange {
    cx0: u16,
    cy0: u16,
    cx1: u16,
    cy1: u16,
}

impl CellRange {
    fn contains(&self, cx: u16, cy: u16) -> bool {
        cx >= self.cx0 && cx < self.cx1 && cy >= self.cy0 && cy < self.cy1
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MonitorGrid {
    id: MonitorId,
    bounds: RectPx,
    cols: u16,
    rows: u16,
    /// Coverage count per cell, row-major. u16, not the u8 in the ADR §8 sketch: u8 can wrap with 256 stacked windows.
    counts: Vec<u16>,
}

impl MonitorGrid {
    fn idx(&self, cx: u16, cy: u16) -> usize {
        cy as usize * self.cols as usize + cx as usize
    }

    fn cell_range(&self, cell: Px, r: &RectPx) -> Option<CellRange> {
        let i = self.bounds.intersect(r)?;
        let (bl, bt) = (self.bounds.l, self.bounds.t);
        let cx0 = ((i.l - bl) / cell) as u16;
        let cy0 = ((i.t - bt) / cell) as u16;
        let cx1 = (((i.r - bl) + cell - 1) / cell).min(self.cols as Px) as u16;
        let cy1 = (((i.b - bt) + cell - 1) / cell).min(self.rows as Px) as u16;
        (cx1 > cx0 && cy1 > cy0).then_some(CellRange { cx0, cy0, cx1, cy1 })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OcclusionGrid {
    cell_px: Px,
    monitors: Vec<MonitorGrid>,
    /// Last rect per tracked window, in physical px.
    windows: HashMap<ExtWindowId, RectPx>,
}

impl OcclusionGrid {
    pub fn new(monitors: &[MonitorInfo], cell_art_px: u16, art_scale: u8) -> Self {
        let cell_px = (cell_art_px.max(1) as Px) * (art_scale.max(1) as Px);
        let monitors = monitors
            .iter()
            .map(|m| {
                let cells = |len: Px| ((len.max(0) + cell_px - 1) / cell_px).min(u16::MAX as Px) as u16;
                let (cols, rows) = (cells(m.bounds.width()), cells(m.bounds.height()));
                MonitorGrid { id: m.id, bounds: m.bounds, cols, rows, counts: vec![0; cols as usize * rows as usize] }
            })
            .collect();
        Self { cell_px, monitors, windows: HashMap::new() }
    }

    /// Incremental: −1 on old cells, +1 on new cells. Returns only cells that changed state (bare ↔ covered).
    /// `new: None` stops tracking the window.
    pub fn apply(&mut self, id: ExtWindowId, new: Option<RectPx>, cause: RectCause) -> CellDelta {
        let old = match new {
            Some(r) => self.windows.insert(id, r),
            None => self.windows.remove(&id),
        };
        let mut delta = CellDelta { cause, window: id, covered: Vec::new(), bared: Vec::new() };
        if old == new {
            return delta;
        }
        let cell = self.cell_px;
        for g in &mut self.monitors {
            let o = old.and_then(|r| g.cell_range(cell, &r));
            let n = new.and_then(|r| g.cell_range(cell, &r));
            if o == n {
                continue;
            }
            if let Some(o) = o {
                for cy in o.cy0..o.cy1 {
                    for cx in o.cx0..o.cx1 {
                        if n.is_some_and(|n| n.contains(cx, cy)) {
                            continue;
                        }
                        let i = g.idx(cx, cy);
                        debug_assert!(g.counts[i] > 0, "coverage underflow");
                        g.counts[i] = g.counts[i].saturating_sub(1);
                        if g.counts[i] == 0 {
                            delta.bared.push(CellId { monitor: g.id, cx, cy });
                        }
                    }
                }
            }
            if let Some(n) = n {
                for cy in n.cy0..n.cy1 {
                    for cx in n.cx0..n.cx1 {
                        if o.is_some_and(|o| o.contains(cx, cy)) {
                            continue;
                        }
                        let i = g.idx(cx, cy);
                        g.counts[i] = g.counts[i].saturating_add(1);
                        if g.counts[i] == 1 {
                            delta.covered.push(CellId { monitor: g.id, cx, cy });
                        }
                    }
                }
            }
        }
        delta
    }

    /// Union of every cell crossed while a rect moves (and resizes) from `from` to `to`, for sweep-crush.
    /// Interpolates in steps of at most one cell per edge, so a fast fling leaves no gap.
    pub fn sweep(&self, from: RectPx, to: RectPx) -> Vec<CellId> {
        let cell = self.cell_px;
        let max_d = [to.l - from.l, to.t - from.t, to.r - from.r, to.b - from.b]
            .iter()
            .map(|d| d.abs())
            .max()
            .unwrap_or(0);
        let steps = ((max_d + cell - 1) / cell).max(1);
        let lerp = |a: Px, b: Px, i: Px| a + (((b - a) as i64 * i as i64) / steps as i64) as Px;
        let mut out = Vec::new();
        for g in &self.monitors {
            let mut marked = vec![false; g.counts.len()];
            let mut seen: Option<CellRange> = None;
            for i in 0..=steps {
                let r = RectPx::new(lerp(from.l, to.l, i), lerp(from.t, to.t, i), lerp(from.r, to.r, i), lerp(from.b, to.b, i));
                let Some(cr) = g.cell_range(cell, &r) else { continue };
                for cy in cr.cy0..cr.cy1 {
                    for cx in cr.cx0..cr.cx1 {
                        marked[g.idx(cx, cy)] = true;
                    }
                }
                seen = Some(match seen {
                    None => cr,
                    Some(s) => CellRange {
                        cx0: s.cx0.min(cr.cx0),
                        cy0: s.cy0.min(cr.cy0),
                        cx1: s.cx1.max(cr.cx1),
                        cy1: s.cy1.max(cr.cy1),
                    },
                });
            }
            if let Some(s) = seen {
                for cy in s.cy0..s.cy1 {
                    for cx in s.cx0..s.cx1 {
                        if marked[g.idx(cx, cy)] {
                            out.push(CellId { monitor: g.id, cx, cy });
                        }
                    }
                }
            }
        }
        out
    }

    /// False for cells outside every monitor.
    pub fn is_bare(&self, c: CellId) -> bool {
        self.coverage(c) == Some(0)
    }

    /// Top edges of tracked windows on monitor `m`, split wherever the cell row directly above isn't bare.
    /// Sorted by (y, x0, window) for determinism.
    pub fn platforms(&self, m: MonitorId) -> Vec<Platform> {
        let Some(g) = self.grid(m) else { return Vec::new() };
        let cell = self.cell_px;
        let b = g.bounds;
        let mut out = Vec::new();
        for (&id, r) in &self.windows {
            if r.is_empty() || r.t < b.t || r.t >= b.b || r.r <= b.l || r.l >= b.r {
                continue;
            }
            let cy_top = ((r.t - b.t) / cell) as u16;
            if cy_top == 0 {
                continue; // nothing above the top row of the monitor
            }
            let above = cy_top - 1;
            let cx0 = ((r.l.max(b.l) - b.l) / cell) as u16;
            let cx1 = (((r.r.min(b.r) - b.l) + cell - 1) / cell).min(g.cols as Px) as u16;
            let mut run: Option<u16> = None;
            for cx in cx0..=cx1 {
                let bare = cx < cx1 && g.counts[g.idx(cx, above)] == 0;
                match (bare, run) {
                    (true, None) => run = Some(cx),
                    (false, Some(start)) => {
                        let x0 = r.l.max(b.l + start as Px * cell);
                        let x1 = r.r.min(b.l + cx as Px * cell);
                        if x1 > x0 {
                            out.push(Platform { window: id, monitor: m, y: r.t, x0, x1 });
                        }
                        run = None;
                    }
                    _ => {}
                }
            }
        }
        out.sort_by_key(|p| (p.y, p.x0, p.window.0));
        out
    }

    /// Boot / reattach / rescan: `windows` becomes the truth for monitor `m`. Tracked windows that touch `m` but
    /// aren't listed are dropped; listed ones take the given rect. Counts are then recomputed from scratch.
    pub fn rebuild(&mut self, m: MonitorId, windows: &[(ExtWindowId, RectPx)]) {
        let Some(bounds) = self.grid(m).map(|g| g.bounds) else { return };
        let listed: HashSet<ExtWindowId> = windows.iter().map(|(id, _)| *id).collect();
        self.windows.retain(|id, r| listed.contains(id) || r.intersect(&bounds).is_none());
        for (id, r) in windows {
            self.windows.insert(*id, *r);
        }
        let cell = self.cell_px;
        for g in &mut self.monitors {
            g.counts.iter_mut().for_each(|c| *c = 0);
            for r in self.windows.values() {
                if let Some(cr) = g.cell_range(cell, r) {
                    for cy in cr.cy0..cr.cy1 {
                        for cx in cr.cx0..cr.cx1 {
                            let i = g.idx(cx, cy);
                            g.counts[i] = g.counts[i].saturating_add(1);
                        }
                    }
                }
            }
        }
    }

    // ── helpers beyond the §8 contract (debug views, spikes, tests) ──

    /// Cell size in physical px.
    pub fn cell_px(&self) -> Px {
        self.cell_px
    }

    /// (cols, rows) of monitor `m`'s grid.
    pub fn dims(&self, m: MonitorId) -> Option<(u16, u16)> {
        self.grid(m).map(|g| (g.cols, g.rows))
    }

    /// How many tracked windows overlap cell `c`; `None` outside every monitor.
    pub fn coverage(&self, c: CellId) -> Option<u16> {
        let g = self.grid(c.monitor)?;
        (c.cx < g.cols && c.cy < g.rows).then(|| g.counts[g.idx(c.cx, c.cy)])
    }

    /// The cell under a physical-px point, if it's on a monitor.
    pub fn cell_at(&self, x: Px, y: Px) -> Option<CellId> {
        let g = self.monitors.iter().find(|g| g.bounds.contains(x, y))?;
        let cx = ((x - g.bounds.l) / self.cell_px) as u16;
        let cy = ((y - g.bounds.t) / self.cell_px) as u16;
        (cx < g.cols && cy < g.rows).then_some(CellId { monitor: g.id, cx, cy })
    }

    /// Every cell a rect touches, across monitors. Empty if the rect is off every monitor.
    pub fn cells_in(&self, r: RectPx) -> Vec<CellId> {
        let mut out = Vec::new();
        for g in &self.monitors {
            if let Some(cr) = g.cell_range(self.cell_px, &r) {
                for cy in cr.cy0..cr.cy1 {
                    for cx in cr.cx0..cr.cx1 {
                        out.push(CellId { monitor: g.id, cx, cy });
                    }
                }
            }
        }
        out
    }

    /// True if the rect lies on at least one monitor and every cell it touches is bare.
    pub fn all_bare(&self, r: RectPx) -> bool {
        let cells = self.cells_in(r);
        !cells.is_empty() && cells.iter().all(|c| self.is_bare(*c))
    }

    /// Number of tracked windows.
    pub fn tracked(&self) -> usize {
        self.windows.len()
    }

    fn grid(&self, m: MonitorId) -> Option<&MonitorGrid> {
        self.monitors.iter().find(|g| g.id == m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    const M0: MonitorId = MonitorId(0);
    const M1: MonitorId = MonitorId(1);
    const M2: MonitorId = MonitorId(2);

    fn mon(id: MonitorId, l: Px, t: Px, r: Px, b: Px) -> MonitorInfo {
        let bounds = RectPx::new(l, t, r, b);
        MonitorInfo { id, bounds, work: bounds, dpi: 96, primary: id == M0 }
    }

    /// Three monitors: 1080p primary, a raised 1440p to the right, a 1280×1024 to the left (negative x).
    fn monitors() -> Vec<MonitorInfo> {
        vec![mon(M0, 0, 0, 1920, 1080), mon(M1, 1920, -200, 4480, 1240), mon(M2, -1280, 0, 0, 1024)]
    }

    fn grid() -> OcclusionGrid {
        OcclusionGrid::new(&monitors(), 16, 1)
    }

    fn w(n: u64) -> ExtWindowId {
        ExtWindowId(n)
    }

    #[test]
    fn apply_reports_state_changes_only() {
        let mut g = grid();
        let d = g.apply(w(1), Some(RectPx::new(0, 0, 32, 16)), RectCause::Shown);
        assert_eq!(d.covered.len(), 2);
        assert!(d.bared.is_empty());
        // Second window over the same cells: counts go up, nothing changes state.
        let d = g.apply(w(2), Some(RectPx::new(0, 0, 32, 16)), RectCause::Shown);
        assert!(d.covered.is_empty() && d.bared.is_empty());
        assert_eq!(g.coverage(CellId { monitor: M0, cx: 0, cy: 0 }), Some(2));
        g.apply(w(1), None, RectCause::Destroyed);
        assert!(!g.is_bare(CellId { monitor: M0, cx: 0, cy: 0 }));
        let d = g.apply(w(2), None, RectCause::Hidden);
        assert_eq!(d.bared.len(), 2);
        assert!(g.is_bare(CellId { monitor: M0, cx: 1, cy: 0 }));
    }

    #[test]
    fn partial_overlap_covers_a_cell() {
        let mut g = grid();
        g.apply(w(1), Some(RectPx::new(15, 15, 17, 17)), RectCause::Shown);
        for (cx, cy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert!(!g.is_bare(CellId { monitor: M0, cx, cy }));
        }
        assert!(g.is_bare(CellId { monitor: M0, cx: 2, cy: 0 }));
    }

    #[test]
    fn window_spanning_two_monitors_covers_both() {
        let mut g = grid();
        let d = g.apply(w(1), Some(RectPx::new(1900, 0, 1940, 10)), RectCause::Shown);
        assert!(d.covered.iter().any(|c| c.monitor == M0));
        assert!(d.covered.iter().any(|c| c.monitor == M1));
    }

    #[test]
    fn sweep_fling_has_no_gaps() {
        let g = grid();
        let cells = g.sweep(RectPx::new(0, 0, 100, 100), RectPx::new(1000, 0, 1100, 100));
        // Columns 0..69 (1100 / 16 rounded up), rows 0..7 (100 / 16 rounded up).
        assert_eq!(cells.len(), 69 * 7);
        assert!(cells.contains(&CellId { monitor: M0, cx: 30, cy: 3 }));
    }

    #[test]
    fn sweep_diagonal_stays_on_the_path() {
        let g = grid();
        let cells: BTreeSet<_> = g.sweep(RectPx::new(0, 0, 32, 32), RectPx::new(320, 320, 352, 352)).into_iter().collect();
        assert!(cells.contains(&CellId { monitor: M0, cx: 10, cy: 10 }));
        assert!(!cells.contains(&CellId { monitor: M0, cx: 20, cy: 0 }));
        assert!(!cells.contains(&CellId { monitor: M0, cx: 0, cy: 20 }));
        // Every row from 0 to 21 is touched: no skipped band.
        for cy in 0..22 {
            assert!(cells.iter().any(|c| c.cy == cy), "row {cy} missing");
        }
    }

    #[test]
    fn sweep_crosses_monitors() {
        let g = grid();
        let cells = g.sweep(RectPx::new(1800, 100, 1850, 150), RectPx::new(2000, 100, 2050, 150));
        assert!(cells.iter().any(|c| c.monitor == M0));
        assert!(cells.iter().any(|c| c.monitor == M1));
    }

    #[test]
    fn platform_detection() {
        let mut g = grid();
        let a = w(1);
        g.apply(a, Some(RectPx::new(160, 320, 800, 600)), RectCause::Shown);
        assert_eq!(g.platforms(M0), vec![Platform { window: a, monitor: M0, y: 320, x0: 160, x1: 800 }]);

        // B hangs down over A's top edge and covers the row above it at x 400..560: A's platform splits.
        let b = w(2);
        g.apply(b, Some(RectPx::new(400, 0, 560, 330)), RectCause::Shown);
        let p = g.platforms(M0);
        assert_eq!(
            p,
            vec![
                Platform { window: a, monitor: M0, y: 320, x0: 160, x1: 400 },
                Platform { window: a, monitor: M0, y: 320, x0: 560, x1: 800 },
            ],
            "B sits at the monitor's top row, so it has no platform"
        );

        // Mid-cell top edge: still a platform, clipped to the window's own x range.
        let c = w(3);
        g.apply(c, Some(RectPx::new(1005, 700, 1203, 900)), RectCause::Shown);
        assert!(g.platforms(M0).contains(&Platform { window: c, monitor: M0, y: 700, x0: 1005, x1: 1203 }));

        // A window whose top edge is on another monitor isn't a platform here.
        assert!(g.platforms(M1).is_empty());
    }

    #[test]
    fn rebuild_drops_unlisted_windows_on_that_monitor_only() {
        let mut g = grid();
        g.apply(w(1), Some(RectPx::new(0, 0, 100, 100)), RectCause::Shown);
        g.apply(w(2), Some(RectPx::new(2000, 0, 2100, 100)), RectCause::Shown);
        g.rebuild(M0, &[(w(3), RectPx::new(500, 500, 600, 600))]);
        assert!(g.is_bare(CellId { monitor: M0, cx: 0, cy: 0 }));
        assert!(!g.is_bare(CellId { monitor: M0, cx: 32, cy: 32 }));
        assert_eq!(g.tracked(), 2); // w2 lives on M1 and survives
    }

    fn rect() -> impl Strategy<Value = RectPx> {
        (-1500..4600i32, -400..1500i32, 0..2000i32, 0..1300i32).prop_map(|(l, t, ww, hh)| RectPx::new(l, t, l + ww, t + hh))
    }

    fn op() -> impl Strategy<Value = (u64, Option<RectPx>)> {
        (0..8u64, prop::option::weighted(0.8, rect()))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Incremental `apply` matches a from-scratch `rebuild`, and the deltas track bare/covered state exactly.
        #[test]
        fn incremental_apply_matches_rebuild(ops in prop::collection::vec(op(), 1..60), scale in 1u8..3) {
            let mons = monitors();
            let mut g = OcclusionGrid::new(&mons, 16, scale);
            let mut model: HashMap<ExtWindowId, RectPx> = HashMap::new();
            let mut covered: BTreeSet<CellId> = BTreeSet::new();
            for (n, r) in ops {
                let d = g.apply(w(n), r, RectCause::Jumped);
                for c in &d.covered { prop_assert!(covered.insert(*c), "covered twice: {c:?}"); }
                for c in &d.bared { prop_assert!(covered.remove(c), "bared but not covered: {c:?}"); }
                match r { Some(r) => { model.insert(w(n), r); } None => { model.remove(&w(n)); } }
            }

            let list: Vec<_> = model.iter().map(|(k, v)| (*k, *v)).collect();
            let mut fresh = OcclusionGrid::new(&mons, 16, scale);
            for m in &mons { fresh.rebuild(m.id, &list); }
            prop_assert_eq!(&fresh, &g);

            let mut again = g.clone();
            for m in &mons { again.rebuild(m.id, &list); }
            prop_assert_eq!(&again, &g);

            for m in &mons {
                let (cols, rows) = g.dims(m.id).unwrap();
                for cy in 0..rows { for cx in 0..cols {
                    let c = CellId { monitor: m.id, cx, cy };
                    prop_assert_eq!(!g.is_bare(c), covered.contains(&c));
                }}
            }
        }
    }
}
