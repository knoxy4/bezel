//! A bedrock bar (TRENCH / SHAFT): a DirectComposition target whose content is an IDCompositionSurface. Every bar draws
//! into its surface, then the renderer issues ONE Commit for all of them, so cross-bar motion can't tear (S0 PASS).

use std::mem::ManuallyDrop;

use bezel_core::RectPx;
use bezel_render_api::DrawList;
use windows::core::Result;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Image, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
};
use windows::Win32::Graphics::DirectComposition::{IDCompositionSurface, IDCompositionTarget, IDCompositionVisual};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM};
use windows::Win32::Graphics::Dxgi::IDXGISurface;
use windows_numerics::Matrix3x2;

use crate::device::Gpu;
use crate::underlay::draw_cmd;

pub struct Bar {
    _target: IDCompositionTarget,
    visual: IDCompositionVisual,
    surface: IDCompositionSurface,
    w: u32,
    h: u32,
}

impl Bar {
    pub fn new(gpu: &Gpu, hwnd: HWND, rect: RectPx) -> Result<Self> {
        let dc = gpu.dcomp.as_ref().ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_NOTIMPL))?;
        let (w, h) = (rect.width().max(1) as u32, rect.height().max(1) as u32);
        unsafe {
            let target = dc.CreateTargetForHwnd(hwnd, true)?;
            let visual = dc.CreateVisual()?;
            let surface = dc.CreateSurface(w, h, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_PREMULTIPLIED)?;
            visual.SetContent(&surface)?;
            target.SetRoot(&visual)?;
            Ok(Self { _target: target, visual, surface, w, h })
        }
    }

    /// New surface at the new size; the caller's next commit shows it.
    pub fn resize(&mut self, gpu: &Gpu, size: (u32, u32)) -> Result<()> {
        let Some(dc) = gpu.dcomp.as_ref() else { return Ok(()) };
        let (w, h) = (size.0.max(1), size.1.max(1));
        unsafe {
            let surface = dc.CreateSurface(w, h, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_PREMULTIPLIED)?;
            self.visual.SetContent(&surface)?;
            self.surface = surface;
        }
        self.w = w;
        self.h = h;
        Ok(())
    }

    /// Draws `dl` into the surface. Doesn't commit: the renderer commits once for all bars.
    pub fn draw(&mut self, gpu: &Gpu, dl: &DrawList) -> Result<()> {
        let dc = &gpu.dc;
        let mut off = POINT::default();
        let surf: IDXGISurface = unsafe { self.surface.BeginDraw(None, &mut off)? };
        let r = (|| unsafe {
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: ManuallyDrop::new(None),
            };
            let target = dc.CreateBitmapFromDxgiSurface(&surf, Some(&props))?;
            dc.SetTarget(&target);
            dc.BeginDraw();
            // The surface can be a region of a shared atlas: clip to our region before Clear, then draw in local px.
            let (ox, oy) = (off.x as f32, off.y as f32);
            dc.PushAxisAlignedClip(&D2D_RECT_F { left: ox, top: oy, right: ox + self.w as f32, bottom: oy + self.h as f32 }, D2D1_ANTIALIAS_MODE_ALIASED);
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            dc.SetTransform(&Matrix3x2::translation(ox, oy));
            let mut res = Ok(());
            for cmd in &dl.cmds {
                if let Err(e) = draw_cmd(gpu, dc, cmd) {
                    res = Err(e);
                    break;
                }
            }
            dc.SetTransform(&Matrix3x2::identity());
            dc.PopAxisAlignedClip();
            let end = dc.EndDraw(None, None);
            dc.SetTarget(None::<&ID2D1Image>);
            res.and(end)
        })();
        unsafe { self.surface.EndDraw()? };
        r
    }
}
