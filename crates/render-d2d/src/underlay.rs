//! One monitor's underlay surface: presenter, wallpaper, draw.

use std::mem::ManuallyDrop;

use bezel_core::RectPx;
use bezel_render_api::{DrawCmd, DrawList, Rgba8};
use windows::core::Result;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1DeviceContext, ID2D1Image, ID2D1SolidColorBrush, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
};
use windows::Win32::Graphics::DirectComposition::{IDCompositionTarget, IDCompositionVisual};
use windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE, DXGI_ALPHA_MODE_IGNORE, DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGISurface, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

use windows_numerics::Vector2;

use crate::device::Gpu;
use crate::wallpaper::Wallpaper;

enum Presenter {
    DComp { _target: IDCompositionTarget, visual: IDCompositionVisual, sc: IDXGISwapChain1 },
    Flip { sc: IDXGISwapChain1 },
}

impl Presenter {
    fn sc(&self) -> &IDXGISwapChain1 {
        match self {
            Presenter::DComp { sc, .. } | Presenter::Flip { sc } => sc,
        }
    }
}

pub struct Underlay {
    pres: Presenter,
    rect: RectPx,
    w: u32,
    h: u32,
    _art_scale: u8,
    transparent: bool,
    wall: Option<Wallpaper>,
}

fn sc_desc(w: u32, h: u32, alpha: DXGI_ALPHA_MODE, seq: bool) -> DXGI_SWAP_CHAIN_DESC1 {
    DXGI_SWAP_CHAIN_DESC1 {
        Width: w,
        Height: h,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: if seq { DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL } else { DXGI_SWAP_EFFECT_FLIP_DISCARD },
        AlphaMode: alpha,
        Flags: 0,
    }
}

fn comp_sc(gpu: &Gpu, w: u32, h: u32, transparent: bool) -> Result<IDXGISwapChain1> {
    let alpha = if transparent { DXGI_ALPHA_MODE_PREMULTIPLIED } else { DXGI_ALPHA_MODE_IGNORE };
    unsafe { gpu.factory.CreateSwapChainForComposition(&gpu.d3d, &sc_desc(w, h, alpha, true), None) }
}

impl Underlay {
    /// DirectComposition first (S9: works on a Progman child under the raised structure), flip swap chain otherwise.
    pub fn new(gpu: &Gpu, hwnd: HWND, rect: RectPx, art_scale: u8) -> Result<Self> {
        let (w, h) = (rect.width().max(1) as u32, rect.height().max(1) as u32);
        let mut pres = None;
        if let Some(dc) = &gpu.dcomp {
            let r = (|| unsafe {
                let target = dc.CreateTargetForHwnd(hwnd, true)?;
                let sc = comp_sc(gpu, w, h, false)?;
                let visual = dc.CreateVisual()?;
                visual.SetContent(&sc)?;
                target.SetRoot(&visual)?;
                dc.Commit()?;
                Ok::<_, windows::core::Error>(Presenter::DComp { _target: target, visual, sc })
            })();
            pres = r.ok();
        }
        let pres = match pres {
            Some(p) => p,
            None => Presenter::Flip { sc: unsafe { gpu.factory.CreateSwapChainForHwnd(&gpu.d3d, hwnd, &sc_desc(w, h, DXGI_ALPHA_MODE_IGNORE, false), None, None)? } },
        };
        Ok(Self { pres, rect, w, h, _art_scale: art_scale, transparent: false, wall: None })
    }

    pub fn presenter_name(&self) -> &'static str {
        match self.pres {
            Presenter::DComp { .. } => "DComp",
            Presenter::Flip { .. } => "flip",
        }
    }

    pub fn transparent(&self) -> bool {
        self.transparent
    }

    pub fn set_wallpaper(&mut self, w: Wallpaper) {
        self.wall = Some(w);
    }

    pub fn resize(&mut self, gpu: &Gpu, size: (u32, u32)) {
        self.w = size.0.max(1);
        self.h = size.1.max(1);
        self.rect.r = self.rect.l + self.w as i32;
        self.rect.b = self.rect.t + self.h as i32;
        unsafe {
            gpu.dc.SetTarget(None::<&ID2D1Image>);
            let _ = self.pres.sc().ResizeBuffers(0, self.w, self.h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0));
        }
    }

    pub fn set_transparent(&mut self, gpu: &Gpu, on: bool) -> std::result::Result<(), String> {
        if on == self.transparent {
            return Ok(());
        }
        let Presenter::DComp { visual, sc, .. } = &mut self.pres else {
            return Err("transparent mode needs the DirectComposition presenter; this underlay fell back to a flip swap chain".into());
        };
        let new = comp_sc(gpu, self.w, self.h, on).map_err(|e| e.to_string())?;
        unsafe {
            visual.SetContent(&new).map_err(|e| e.to_string())?;
        }
        gpu.commit();
        *sc = new;
        self.transparent = on;
        Ok(())
    }

    pub fn draw(&mut self, gpu: &Gpu, dl: &DrawList) -> Result<()> {
        let dc = &gpu.dc;
        let r = (|| unsafe {
            let surf: IDXGISurface = self.pres.sc().GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: if self.transparent { D2D1_ALPHA_MODE_PREMULTIPLIED } else { D2D1_ALPHA_MODE_IGNORE },
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: ManuallyDrop::new(None),
            };
            let target = dc.CreateBitmapFromDxgiSurface(&surf, Some(&props))?;
            dc.SetTarget(&target);
            dc.BeginDraw();
            let (w, h) = (self.w as f32, self.h as f32);
            if self.transparent {
                dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            } else {
                let bg = self.wall.as_ref().map(|wl| wl.background).unwrap_or(D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 });
                dc.Clear(Some(&bg));
                if let Some(wl) = &self.wall {
                    wl.draw(dc, w, h, (self.rect.l as f32, self.rect.t as f32));
                }
            }
            for cmd in &dl.cmds {
                draw_cmd(gpu, dc, cmd)?;
            }
            dc.EndDraw(None, None)?;
            dc.SetTarget(None::<&ID2D1Image>);
            drop(target);
            drop(surf);
            // Full-surface present for now; dirty-rect Present1 comes with the frontier renderer.
            self.pres.sc().Present(0, DXGI_PRESENT(0)).ok()
        })();
        if r.is_err() {
            unsafe { dc.SetTarget(None::<&ID2D1Image>) };
        }
        r
    }
}

pub(crate) unsafe fn draw_cmd(gpu: &Gpu, dc: &ID2D1DeviceContext, cmd: &DrawCmd) -> Result<()> {
    let brush = |c: Rgba8| -> Result<ID2D1SolidColorBrush> { gpu.brush(c) };
    match cmd {
        DrawCmd::Fill { rect, color: c } => unsafe {
            dc.FillRectangle(&D2D_RECT_F { left: rect[0], top: rect[1], right: rect[2], bottom: rect[3] }, &brush(*c)?);
        },
        DrawCmd::Polyline { pts, width, color: c, glow } => unsafe {
            if *glow {
                let mut g = *c;
                g.0[3] = (g.0[3] as u16 * 2 / 7) as u8;
                let gb = brush(g)?;
                for p in pts.windows(2) {
                    dc.DrawLine(Vector2 { X: p[0][0], Y: p[0][1] }, Vector2 { X: p[1][0], Y: p[1][1] }, &gb, width * 3.0, None);
                }
            }
            let b = brush(*c)?;
            for p in pts.windows(2) {
                dc.DrawLine(Vector2 { X: p[0][0], Y: p[0][1] }, Vector2 { X: p[1][0], Y: p[1][1] }, &b, *width, None);
            }
        },
        DrawCmd::Particles { pts, color: c } => unsafe {
            let b = brush(*c)?;
            for p in pts {
                let s = p[2].max(1.0) / 2.0;
                dc.FillRectangle(&D2D_RECT_F { left: p[0] - s, top: p[1] - s, right: p[0] + s, bottom: p[1] + s }, &b);
            }
        },
        DrawCmd::Text { s, x, y, size, color: c } => unsafe {
            if let Some(f) = gpu.font(*size) {
                let text: Vec<u16> = s.encode_utf16().collect();
                let r = D2D_RECT_F { left: *x, top: *y, right: x + 4000.0, bottom: y + size * 4.0 };
                dc.DrawText(&text, &f, &r, &brush(*c)?, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
        },
        DrawCmd::Sprite { id, frame, x, y, flip_x, alpha, scale } => {
            let atlas = gpu.atlas.borrow();
            let Some(a) = atlas.as_ref() else { return Ok(()) };
            let f = if *flip_x { 1 } else { *frame };
            let Some((r, pv)) = a.frames.get(&(id.0, f)).or_else(|| a.frames.get(&(id.0, 0))) else { return Ok(()) };
            let (w, h) = (r[2] * scale, r[3] * scale);
            let (l, t) = ((x - pv[0] * w).round(), (y - pv[1] * h).round());
            let src = D2D_RECT_F { left: r[0], top: r[1], right: r[0] + r[2], bottom: r[1] + r[3] };
            unsafe {
                dc.DrawBitmap(&a.bmp, Some(&D2D_RECT_F { left: l, top: t, right: l + w, bottom: t + h }), *alpha as f32 / 255.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, Some(&src), None);
            }
        }
    }
    Ok(())
}
