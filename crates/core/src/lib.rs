//! POD types, ids, geometry, events. No deps. (ADR-0001 §8)

/// Physical pixels, always (invariant 5).
pub type Px = i32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RectPx {
    pub l: Px,
    pub t: Px,
    pub r: Px,
    pub b: Px,
}

impl RectPx {
    pub const fn new(l: Px, t: Px, r: Px, b: Px) -> Self {
        Self { l, t, r, b }
    }
    pub const fn width(&self) -> Px {
        self.r - self.l
    }
    pub const fn height(&self) -> Px {
        self.b - self.t
    }
    pub const fn is_empty(&self) -> bool {
        self.r <= self.l || self.b <= self.t
    }
    /// Overlap of two rects, or `None` when they don't share any area.
    pub fn intersect(&self, o: &RectPx) -> Option<RectPx> {
        let r = RectPx::new(self.l.max(o.l), self.t.max(o.t), self.r.min(o.r), self.b.min(o.b));
        (!r.is_empty()).then_some(r)
    }
    /// Smallest rect containing both.
    pub fn union(&self, o: &RectPx) -> RectPx {
        RectPx::new(self.l.min(o.l), self.t.min(o.t), self.r.max(o.r), self.b.max(o.b))
    }
    pub const fn contains(&self, x: Px, y: Px) -> bool {
        x >= self.l && x < self.r && y >= self.t && y < self.b
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MonitorId(pub u64);

/// Opaque; an HWND never leaves os-win32.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtWindowId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tick(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellId {
    pub monitor: MonitorId,
    pub cx: u16,
    pub cy: u16,
}

pub const SIM_HZ: u32 = 20;
/// Fallback canopy only.
pub const DROP_POOL: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceId {
    Trench,
    Shaft,
    Underlay(MonitorId),
    Band,
    Diorama,
    // Fallback canopy (S9 fail) only.
    Ledge,
    LaneL,
    LaneR,
    Drop(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Top,
    Right,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Counts only. By design, no field can hold which key was pressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PulseBatch {
    pub bucket_ms: u16,
    pub key_events: u16,
    pub mouse_clicks: u16,
    pub wheel_ticks: u16,
    pub mouse_travel_px: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorInfo {
    pub id: MonitorId,
    pub bounds: RectPx,
    pub work: RectPx,
    pub dpi: u32,
    pub primary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetWindow {
    pub id: ExtWindowId,
    pub frame: RectPx,
    pub monitor: MonitorId,
    pub maximized: bool,
}

/// Why a window's rect changed. The sim maps causes to crush / bury / unbury (D21).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RectCause {
    DragStart,
    DragSample,
    DragEnd,
    Shown,
    Hidden,
    Jumped,
    Minimized,
    Restored,
    Cloaked,
    Uncloaked,
    Destroyed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsEvent {
    /// Every tracked top-level window.
    WindowRect { id: ExtWindowId, rect: Option<RectPx>, cause: RectCause },
    Foreground(Option<TargetWindow>),
    DisplaysChanged(Vec<MonitorInfo>),
    WallpaperChanged(MonitorId),
    /// Explorer restart / underlay torn down → reattach.
    DesktopLayerReset,
    Docked { surface: SurfaceId, rect: RectPx },
    StandDown { monitor: MonitorId, on: bool },
    SessionActive(bool),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_intersect_and_union() {
        let a = RectPx::new(0, 0, 10, 10);
        let b = RectPx::new(5, 5, 20, 20);
        assert_eq!(a.intersect(&b), Some(RectPx::new(5, 5, 10, 10)));
        assert_eq!(a.intersect(&RectPx::new(10, 0, 20, 10)), None);
        assert_eq!(a.union(&b), RectPx::new(0, 0, 20, 20));
        assert!(RectPx::new(3, 3, 3, 9).is_empty());
    }
}
