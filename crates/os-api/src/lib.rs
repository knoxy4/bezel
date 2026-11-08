//! OS port traits (ADR-0001 §8). Adapters: os-win32 (real), os-fake (tests, diorama).
//!
//! Phase 2 scope: `DesktopLayer`. The other ports (pulses, window tracking, dock, fullscreen guard, surfaces) land with
//! the work that needs them.

use std::sync::mpsc::Sender;

use bezel_core::{Edge, ExtWindowId, MonitorId, MonitorInfo, OsEvent, PulseBatch, Px, RectPx, SurfaceId};

/// Input activity as counts (ADR §5.6). Implementations must never let key identity into memory.
/// (ADR §8 sketched crossbeam's Sender; std's mpsc is enough and keeps the port dependency-free.)
pub trait InputPulseSource: Send {
    fn start(&mut self, tx: Sender<PulseBatch>) -> Result<(), OsError>;
    fn set_paused(&self, paused: bool);
    fn stop(&mut self);
}

/// Every tracked top-level window, event-driven (ADR §5.4).
pub trait WindowTracker: Send {
    /// Global, out-of-context. Emits `WindowRect` (rect already DWM-resolved and coverage-filtered) and `Foreground`.
    fn start(&mut self, tx: Sender<OsEvent>) -> Result<(), OsError>;
    /// DWM extended frame bounds if the window counts toward coverage.
    fn frame_of(&self, id: ExtWindowId) -> Option<RectPx>;
    /// Full rescan of windows that count, on monitor `m`'s rect (boot, reattach, backstop).
    fn snapshot(&self, m: MonitorId) -> Vec<(ExtWindowId, RectPx)>;
    fn stop(&mut self);
}
use raw_window_handle::RawWindowHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockMode {
    Reserve,
    Yield,
}

/// The bedrock bars as shell AppBars (ADR §5.2, D2, D3).
pub trait ShellDock {
    fn monitors(&self) -> Vec<MonitorInfo>;
    fn taskbar_edge(&self, m: MonitorId) -> Edge;
    /// Creates the surface's window on first use. TRENCH must be docked before SHAFT (§2.5: the corner is TRENCH's).
    fn dock(&mut self, s: SurfaceId, m: MonitorId, edge: Edge, thickness: Px, mode: DockMode) -> Result<RectPx, OsError>;
    /// Every exit path.
    fn undock_all(&mut self);
    /// Phase 2: the bar's window, for the renderer.
    fn surface_handle(&self, s: SurfaceId) -> Option<RawWindowHandle>;
    /// Phase 2 (S3): re-docks after `ABN_POSCHANGED` / an Explorer restart and reports fullscreen stand-down.
    /// Returns `Docked` for every bar whose rect changed and `StandDown` on fullscreen changes. Call ~once a frame.
    fn service(&mut self) -> Vec<OsEvent>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsError {
    /// The shell isn't in a state we can attach to yet (e.g. mid Explorer restart). Retry.
    NotReady(String),
    /// A Win32 call failed; message carries the call and HRESULT, never a window title or path.
    Win32(String),
    UnknownMonitor(MonitorId),
}

impl std::fmt::Display for OsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OsError::NotReady(s) => write!(f, "not ready: {s}"),
            OsError::Win32(s) => write!(f, "win32: {s}"),
            OsError::UnknownMonitor(m) => write!(f, "unknown monitor {}", m.0),
        }
    }
}

impl std::error::Error for OsError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopTopology {
    Legacy,
    Raised,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WallpaperFit {
    Fill,
    Fit,
    Stretch,
    Tile,
    Center,
    Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallpaperDesc {
    pub path: Option<std::path::PathBuf>,
    pub fit: WallpaperFit,
    pub background: [u8; 4],
}

/// D38. `TransparentOverForeign` needs a presenter with per-pixel alpha (DirectComposition).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnderlayMode {
    Opaque,
    TransparentOverForeign,
    Excluded,
}

/// What a `DesktopLayer::health` pass found. Added in phase 2: S9 showed the layer itself must notice Explorer restarts,
/// outside teardown and z-order drift, so the app can reattach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerHealth {
    Ok,
    /// Something slid between the icons and an underlay; the layer put it back.
    ZOrderFixed,
    /// The desktop host or an underlay is gone (Explorer restarted, the window was destroyed). Detach and reattach.
    ResetNeeded,
}

pub trait DesktopLayer {
    fn topology(&self) -> DesktopTopology;
    /// Create this monitor's underlay window in the desktop layer, beneath the icons.
    fn attach(&mut self, m: MonitorId, rect: RectPx) -> Result<RawWindowHandle, OsError>;
    /// Every exit path (invariant 6).
    fn detach_all(&mut self);
    /// Read-only. BEZEL never writes the wallpaper (invariant 6).
    fn wallpaper(&self, m: MonitorId) -> Option<WallpaperDesc>;
    /// A live-wallpaper app or desktop widget occupies this monitor's part of the desktop layer.
    fn foreign_occupant(&self, m: MonitorId) -> bool;
    fn set_mode(&mut self, m: MonitorId, mode: UnderlayMode) -> Result<(), OsError>;
    /// Call about once a second, and after any shell notification. Pumps nothing; reads state only.
    fn health(&mut self) -> LayerHealth;
}
