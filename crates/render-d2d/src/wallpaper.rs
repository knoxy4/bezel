//! The player's wallpaper, loaded read-only and drawn with Windows' fit modes (D22). Invariant 6: never written.

use std::mem::ManuallyDrop;

use bezel_render_api::{Rgba8, WallpaperFitR};
use windows::core::{Result, HSTRING};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap1, ID2D1DeviceContext, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{GUID_WICPixelFormat32bppPBGRA, IWICPalette, WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand};

use crate::device::Gpu;

pub struct Wallpaper {
    bmp: Option<(ID2D1Bitmap1, f32, f32)>,
    fit: WallpaperFitR,
    pub background: D2D1_COLOR_F,
}

fn rectf(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: l, top: t, right: r, bottom: b }
}

impl Wallpaper {
    /// A missing or unreadable image falls back to the background color; that isn't an error.
    pub fn load(gpu: &Gpu, image: Option<&std::path::Path>, fit: WallpaperFitR, bg: Rgba8) -> Result<Self> {
        let background = D2D1_COLOR_F { r: bg.0[0] as f32 / 255.0, g: bg.0[1] as f32 / 255.0, b: bg.0[2] as f32 / 255.0, a: 1.0 };
        let bmp = image.and_then(|p| load_image(gpu, p).ok());
        Ok(Self { bmp, fit, background })
    }

    /// Draw over a `w × h` surface whose top-left sits at `origin` in screen px (span fit needs it).
    pub fn draw(&self, dc: &ID2D1DeviceContext, w: f32, h: f32, origin: (f32, f32)) {
        let Some((bmp, iw, ih)) = &self.bmp else { return };
        let (iw, ih) = (*iw, *ih);
        let draw = |d: D2D_RECT_F| unsafe { dc.DrawBitmap(bmp, Some(&d), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
        match self.fit {
            WallpaperFitR::Stretch => draw(rectf(0.0, 0.0, w, h)),
            WallpaperFitR::Center => {
                let (x, y) = ((w - iw) / 2.0, (h - ih) / 2.0);
                draw(rectf(x, y, x + iw, y + ih));
            }
            WallpaperFitR::Tile => {
                let mut y = 0.0;
                while y < h {
                    let mut x = 0.0;
                    while x < w {
                        draw(rectf(x, y, x + iw, y + ih));
                        x += iw.max(1.0);
                    }
                    y += ih.max(1.0);
                }
            }
            WallpaperFitR::Fit | WallpaperFitR::Fill => {
                let s = if self.fit == WallpaperFitR::Fit { (w / iw).min(h / ih) } else { (w / iw).max(h / ih) };
                let (dw, dh) = (iw * s, ih * s);
                let (x, y) = ((w - dw) / 2.0, (h - dh) / 2.0);
                draw(rectf(x, y, x + dw, y + dh));
            }
            WallpaperFitR::Span { virtual_screen: v } => {
                let (vw, vh) = (v.width() as f32, v.height() as f32);
                let s = (vw / iw).max(vh / ih);
                let (dw, dh) = (iw * s, ih * s);
                let (x, y) = ((vw - dw) / 2.0 - (origin.0 - v.l as f32), (vh - dh) / 2.0 - (origin.1 - v.t as f32));
                draw(rectf(x, y, x + dw, y + dh));
            }
        }
    }
}

fn load_image(gpu: &Gpu, path: &std::path::Path) -> Result<(ID2D1Bitmap1, f32, f32)> {
    unsafe {
        let dec = gpu.wic.CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
        let frame = dec.GetFrame(0)?;
        let conv = gpu.wic.CreateFormatConverter()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None::<&IWICPalette>, 0.0, WICBitmapPaletteTypeMedianCut)?;
        let (mut w, mut h) = (0u32, 0u32);
        conv.GetSize(&mut w, &mut h)?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: ManuallyDrop::new(None),
        };
        let bmp = gpu.dc.CreateBitmapFromWicBitmap(&conv, Some(&props))?;
        Ok((bmp, w as f32, h as f32))
    }
}
