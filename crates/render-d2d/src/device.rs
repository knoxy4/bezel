//! Shared D3D11 / D2D / DComp / DirectWrite / WIC devices.

use std::collections::HashMap;
use std::cell::RefCell;

use windows::core::{w, Interface, Result};
use windows::Win32::Foundation::HMODULE;
use bezel_render_api::Rgba8;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{D2D1CreateFactory, ID2D1DeviceContext, ID2D1SolidColorBrush, ID2D1Factory1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1};
use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION};
use windows::Win32::Graphics::DirectComposition::{DCompositionCreateDevice, IDCompositionDevice};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD,
};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice, IDXGIFactory2};
use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, IWICImagingFactory};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};

pub struct Gpu {
    pub d3d: ID3D11Device,
    pub factory: IDXGIFactory2,
    pub dc: ID2D1DeviceContext,
    pub dcomp: Option<IDCompositionDevice>,
    pub dwrite: IDWriteFactory,
    pub wic: IWICImagingFactory,
    fonts: RefCell<HashMap<u32, IDWriteTextFormat>>,
    brushes: RefCell<HashMap<[u8; 4], ID2D1SolidColorBrush>>,
}

impl Gpu {
    pub fn new() -> Result<Self> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let mut d3d: Option<ID3D11Device> = None;
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0]),
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )?;
            let d3d = d3d.ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
            let d2f: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dc = d2f.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            dc.SetDpi(96.0, 96.0); // DIPs = physical px (invariant 5)
            let dcomp = DCompositionCreateDevice::<_, IDCompositionDevice>(&dxgi).ok();
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self { d3d, factory, dc, dcomp, dwrite, wic, fonts: RefCell::new(HashMap::new()), brushes: RefCell::new(HashMap::new()) })
        }
    }

    pub fn commit(&self) {
        if let Some(d) = &self.dcomp {
            let _ = unsafe { d.Commit() };
        }
    }

    /// Solid brush for `c`, cached per color (brushes belong to the one device context).
    pub fn brush(&self, c: Rgba8) -> Result<ID2D1SolidColorBrush> {
        if let Some(b) = self.brushes.borrow().get(&c.0) {
            return Ok(b.clone());
        }
        let col = D2D1_COLOR_F { r: c.0[0] as f32 / 255.0, g: c.0[1] as f32 / 255.0, b: c.0[2] as f32 / 255.0, a: c.0[3] as f32 / 255.0 };
        let b = unsafe { self.dc.CreateSolidColorBrush(&col, None)? };
        let mut cache = self.brushes.borrow_mut();
        if cache.len() > 256 {
            cache.clear();
        }
        cache.insert(c.0, b.clone());
        Ok(b)
    }

    /// Text format at `size` px, cached per size (rounded to 0.5 px).
    pub fn font(&self, size: f32) -> Option<IDWriteTextFormat> {
        let key = (size * 2.0).round().max(1.0) as u32;
        if let Some(f) = self.fonts.borrow().get(&key) {
            return Some(f.clone());
        }
        let f = unsafe {
            self.dwrite
                .CreateTextFormat(w!("Segoe UI"), None, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, key as f32 / 2.0, w!("en-us"))
                .ok()?
        };
        self.fonts.borrow_mut().insert(key, f.clone());
        Some(f)
    }
}
