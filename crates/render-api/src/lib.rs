//! DrawList, Renderer trait, atlas desc, SurfaceKind (ADR-0001 §8).

use bezel_core::{MonitorId, RectPx, SurfaceId};
use raw_window_handle::RawWindowHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpriteId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba8(pub [u8; 4]);

/// The backend picks a presenter per kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    BarComposited,
    Underlay,
    Topmost,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCmd {
    Sprite { id: SpriteId, frame: u16, x: f32, y: f32, flip_x: bool, alpha: u8 },
    Polyline { pts: Vec<[f32; 2]>, width: f32, color: Rgba8, glow: bool },
    Particles { pts: Vec<[f32; 3]>, color: Rgba8 },
    Fill { rect: [f32; 4], color: Rgba8 },
    Text { s: String, x: f32, y: f32, size: f32, color: Rgba8 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawList {
    pub surface: SurfaceId,
    /// Empty = the whole surface.
    pub dirty: Vec<RectPx>,
    pub cmds: Vec<DrawCmd>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub surfaces_drawn: u8,
    pub cpu_us: u32,
}

/// Mirrors os-api's WallpaperFit; render-api can't depend on os-api (dependency rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WallpaperFitR {
    Fill,
    Fit,
    Stretch,
    Tile,
    Center,
    /// Spanned across the virtual screen; `virtual_screen` gives its bounds, this surface's offset comes from attach.
    Span { virtual_screen: RectPx },
}

/// Sprite sheet + frame map. Filled in when the atlas pipeline lands (phase 4).
#[derive(Clone, Debug, PartialEq)]
pub struct AtlasDesc {
    pub image: std::path::PathBuf,
    /// (sprite, frame) → source rect in the sheet, px.
    pub frames: Vec<(SpriteId, u16, [u32; 4])>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderError {
    Device(String),
    UnknownSurface(SurfaceId),
    Unsupported(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Device(s) => write!(f, "device: {s}"),
            RenderError::UnknownSurface(s) => write!(f, "unknown surface {s:?}"),
            RenderError::Unsupported(s) => write!(f, "unsupported: {s}"),
        }
    }
}

impl std::error::Error for RenderError {}

pub trait Renderer {
    /// `origin` is the surface's top-left in physical screen px (needed for span wallpapers and screen-space draws).
    fn attach(&mut self, s: SurfaceId, kind: SurfaceKind, h: RawWindowHandle, rect: RectPx, art_scale: u8) -> Result<(), RenderError>;
    fn resize(&mut self, s: SurfaceId, size: (u32, u32));
    fn detach(&mut self, s: SurfaceId);
    fn load_atlas(&mut self, atlas: &AtlasDesc) -> Result<(), RenderError>;
    fn set_wallpaper(&mut self, m: MonitorId, image: Option<&std::path::Path>, fit: WallpaperFitR, background: Rgba8) -> Result<(), RenderError>;
    /// Underlays only: opaque (wallpaper repainted underneath) or transparent over a foreign live wallpaper (D38).
    fn set_underlay_transparent(&mut self, m: MonitorId, transparent: bool) -> Result<(), RenderError>;
    /// Bars: dirty rects + ONE atomic commit. Underlays: wallpaper repaint under dirty rects + present.
    fn submit(&mut self, frame: &[DrawList]) -> Result<FrameStats, RenderError>;
}
