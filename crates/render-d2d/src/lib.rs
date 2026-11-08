//! Direct2D / DirectComposition renderer (ADR-0001 §6, §7). Windows only.
//!
//! Phase 2 scope: underlays (from S9: DirectComposition first, flip swap chain fallback; opaque repaints the wallpaper,
//! transparent clears to alpha 0 over a foreign live wallpaper, D38) and bedrock bars (from S0: DComp surfaces, one
//! Commit per frame for every bar). Topmost surfaces (BAND, fallback canopy) land later.

#[cfg(windows)]
mod bar;
#[cfg(windows)]
mod device;
#[cfg(windows)]
mod underlay;
#[cfg(windows)]
mod wallpaper;

#[cfg(windows)]
pub use renderer::D2dRenderer;

#[cfg(windows)]
mod renderer {
    use std::collections::HashMap;
    use std::time::Instant;

    use bezel_core::{MonitorId, RectPx, SurfaceId};
    use bezel_render_api::{AtlasDesc, DrawList, FrameStats, RenderError, Renderer, Rgba8, SurfaceKind, WallpaperFitR};
    use raw_window_handle::RawWindowHandle;
    use windows::Win32::Foundation::HWND;

    use crate::bar::Bar;
    use crate::device::Gpu;
    use crate::underlay::Underlay;
    use crate::wallpaper::Wallpaper;

    pub struct D2dRenderer {
        gpu: Gpu,
        underlays: HashMap<MonitorId, Underlay>,
        bars: HashMap<SurfaceId, Bar>,
    }

    impl D2dRenderer {
        pub fn new() -> Result<Self, RenderError> {
            Ok(Self { gpu: Gpu::new().map_err(|e| RenderError::Device(e.to_string()))?, underlays: HashMap::new(), bars: HashMap::new() })
        }

        /// "DComp" or "flip", for logs and the label.
        pub fn presenter_name(&self, m: MonitorId) -> Option<&'static str> {
            self.underlays.get(&m).map(|u| u.presenter_name())
        }

        pub fn is_transparent(&self, m: MonitorId) -> bool {
            self.underlays.get(&m).is_some_and(|u| u.transparent())
        }

        pub fn dcomp_available(&self) -> bool {
            self.gpu.dcomp.is_some()
        }
    }

    fn monitor_of(s: SurfaceId) -> Result<MonitorId, RenderError> {
        match s {
            SurfaceId::Underlay(m) => Ok(m),
            other => Err(RenderError::Unsupported(format!("{other:?}: only underlays render in phase 2"))),
        }
    }

    impl Renderer for D2dRenderer {
        fn attach(&mut self, s: SurfaceId, kind: SurfaceKind, h: RawWindowHandle, rect: RectPx, art_scale: u8) -> Result<(), RenderError> {
            let RawWindowHandle::Win32(w) = h else { return Err(RenderError::Unsupported("non-Win32 handle".into())) };
            let hwnd = HWND(w.hwnd.get() as *mut _);
            match kind {
                SurfaceKind::BarComposited => {
                    if self.gpu.dcomp.is_none() {
                        return Err(RenderError::Unsupported("bars need DirectComposition".into()));
                    }
                    self.bars.remove(&s);
                    let b = Bar::new(&self.gpu, hwnd, rect).map_err(|e| RenderError::Device(e.to_string()))?;
                    self.bars.insert(s, b);
                    return Ok(());
                }
                SurfaceKind::Topmost => return Err(RenderError::Unsupported("topmost surfaces land in a later phase".into())),
                SurfaceKind::Underlay => {}
            }
            let m = monitor_of(s)?;
            self.underlays.remove(&m);
            let u = Underlay::new(&self.gpu, hwnd, rect, art_scale).map_err(|e| RenderError::Device(e.to_string()))?;
            self.underlays.insert(m, u);
            Ok(())
        }

        fn resize(&mut self, s: SurfaceId, size: (u32, u32)) {
            if let Some(b) = self.bars.get_mut(&s) {
                let _ = b.resize(&self.gpu, size);
                return;
            }
            if let Ok(m) = monitor_of(s) {
                if let Some(u) = self.underlays.get_mut(&m) {
                    u.resize(&self.gpu, size);
                }
            }
        }

        fn detach(&mut self, s: SurfaceId) {
            if self.bars.remove(&s).is_some() {
                self.gpu.commit();
                return;
            }
            if let Ok(m) = monitor_of(s) {
                self.underlays.remove(&m);
                self.gpu.commit();
            }
        }

        fn load_atlas(&mut self, _atlas: &AtlasDesc) -> Result<(), RenderError> {
            Err(RenderError::Unsupported("atlas pipeline lands in phase 4".into()))
        }

        fn set_wallpaper(&mut self, m: MonitorId, image: Option<&std::path::Path>, fit: WallpaperFitR, background: Rgba8) -> Result<(), RenderError> {
            let u = self.underlays.get_mut(&m).ok_or(RenderError::UnknownSurface(SurfaceId::Underlay(m)))?;
            let w = Wallpaper::load(&self.gpu, image, fit, background).map_err(|e| RenderError::Device(e.to_string()))?;
            u.set_wallpaper(w);
            Ok(())
        }

        fn set_underlay_transparent(&mut self, m: MonitorId, transparent: bool) -> Result<(), RenderError> {
            let u = self.underlays.get_mut(&m).ok_or(RenderError::UnknownSurface(SurfaceId::Underlay(m)))?;
            u.set_transparent(&self.gpu, transparent).map_err(RenderError::Unsupported)
        }

        fn submit(&mut self, frame: &[DrawList]) -> Result<FrameStats, RenderError> {
            let t = Instant::now();
            let mut drawn = 0u8;
            let mut first_err = None;
            let mut bars_drawn = false;
            for dl in frame {
                if let Some(b) = self.bars.get_mut(&dl.surface) {
                    match b.draw(&self.gpu, dl) {
                        Ok(()) => {
                            drawn = drawn.saturating_add(1);
                            bars_drawn = true;
                        }
                        Err(e) => {
                            first_err.get_or_insert(RenderError::Device(e.to_string()));
                        }
                    }
                    continue;
                }
                let Ok(m) = monitor_of(dl.surface) else { continue };
                let Some(u) = self.underlays.get_mut(&m) else { continue };
                match u.draw(&self.gpu, dl) {
                    Ok(()) => drawn = drawn.saturating_add(1),
                    Err(e) => {
                        first_err.get_or_insert(RenderError::Device(e.to_string()));
                    }
                }
            }
            // ONE commit for every bar this frame (S0): TRENCH and SHAFT can't show different frames.
            if bars_drawn {
                self.gpu.commit();
            }
            if let Some(e) = first_err {
                return Err(e);
            }
            Ok(FrameStats { surfaces_drawn: drawn, cpu_us: t.elapsed().as_micros().min(u32::MAX as u128) as u32 })
        }
    }
}
