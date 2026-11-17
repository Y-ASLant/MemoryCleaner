//! TrafficMonitor 的不闪路径：D2D 画进合成交换链，`Present(0, 0)` 后 `Commit`。
//! 不调用 `UpdateLayeredWindow`。

use anyhow::Context;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COMPOSITE_MODE_SOURCE_COPY, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext,
    ID2D1Factory1, ID2D1Image,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2,
    IDXGISurface, IDXGISwapChain1,
};
use windows::core::Interface;

pub struct Comp {
    dcomp: IDCompositionDevice,
    _d3d: ID3D11Device,
    context2d: ID2D1DeviceContext,
    swap: IDXGISwapChain1,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    target: Option<ID2D1Bitmap1>,
    width: u32,
    height: u32,
}

pub fn attach(hwnd: HWND, width: u32, height: u32) -> anyhow::Result<Comp> {
    let width = width.max(1);
    let height = height.max(1);
    let (d3d, _context) = create_device()?;
    let dxgi = d3d.cast::<IDXGIDevice>().context("IDXGIDevice")?;
    let factory =
        unsafe { D2D1CreateFactory::<ID2D1Factory1>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
            .context("D2D1CreateFactory")?;
    let d2d = unsafe { factory.CreateDevice(&dxgi) }.context("ID2D1Factory1::CreateDevice")?;
    let context2d = unsafe { d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) }
        .context("CreateDeviceContext")?;

    let dxgi_factory = unsafe { CreateDXGIFactory2::<IDXGIFactory2>(Default::default()) }
        .context("CreateDXGIFactory2")?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: 0,
    };
    let swap = unsafe { dxgi_factory.CreateSwapChainForComposition(&d3d, &desc, None) }
        .context("CreateSwapChainForComposition")?;

    let dcomp: IDCompositionDevice =
        unsafe { DCompositionCreateDevice(&dxgi) }.context("DCompositionCreateDevice")?;
    let target = unsafe { dcomp.CreateTargetForHwnd(hwnd, true) }.context("CreateTargetForHwnd")?;
    let visual = unsafe { dcomp.CreateVisual() }.context("CreateVisual")?;
    unsafe { visual.SetContent(&swap) }.context("SetContent")?;
    unsafe { target.SetRoot(&visual) }.context("SetRoot")?;
    unsafe { dcomp.Commit() }.context("Commit")?;

    let mut comp = Comp {
        dcomp,
        _d3d: d3d,
        context2d,
        swap,
        _target: target,
        _visual: visual,
        target: None,
        width,
        height,
    };
    comp.bind_target()?;
    Ok(comp)
}

impl Comp {
    pub fn present(&mut self, pixels: &[u32], width: u32, height: u32) -> anyhow::Result<()> {
        let width = width.max(1);
        let height = height.max(1);
        if pixels.len() < (width as usize) * (height as usize) {
            anyhow::bail!("pixel buffer short");
        }
        if self.width != width || self.height != height {
            self.resize(width, height)?;
        } else if self.target.is_none() {
            self.bind_target()?;
        }
        self.draw(pixels, width, height)?;
        // 翻转模型下后台缓冲 Present 后就换掉。不松开的话，下一帧 Clear 打在正在显示的缓冲上，每秒闪一下。
        unsafe { self.context2d.SetTarget(None) };
        self.target = None;
        let presented = unsafe { self.swap.Present(0, DXGI_PRESENT(0)) };
        if presented.is_err() {
            anyhow::bail!("Present: {presented}");
        }
        unsafe { self.dcomp.Commit() }.context("Commit")?;
        Ok(())
    }

    /// 任务栏把子窗口失效时，用上一帧重新挂上视觉，避免空画一帧。
    pub fn recommit(&self) -> anyhow::Result<()> {
        unsafe { self.dcomp.Commit() }.context("Commit")?;
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) -> anyhow::Result<()> {
        self.target = None;
        unsafe { self.context2d.SetTarget(None) };
        unsafe {
            self.swap.ResizeBuffers(
                0,
                width,
                height,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                windows::Win32::Graphics::Dxgi::DXGI_SWAP_CHAIN_FLAG(0),
            )
        }
        .context("ResizeBuffers")?;
        self.width = width;
        self.height = height;
        self.bind_target()
    }

    fn bind_target(&mut self) -> anyhow::Result<()> {
        let surface = unsafe { self.swap.GetBuffer::<IDXGISurface>(0) }.context("GetBuffer")?;
        let props = bitmap_props(D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW);
        let bitmap = unsafe {
            self.context2d
                .CreateBitmapFromDxgiSurface(&surface, Some(&props))
        }
        .context("CreateBitmapFromDxgiSurface")?;
        unsafe { self.context2d.SetTarget(&bitmap) };
        self.target = Some(bitmap);
        Ok(())
    }

    fn draw(&self, pixels: &[u32], width: u32, height: u32) -> anyhow::Result<()> {
        let props = bitmap_props(D2D1_BITMAP_OPTIONS_NONE);
        let bitmap = unsafe {
            self.context2d.CreateBitmap(
                D2D_SIZE_U { width, height },
                Some(pixels.as_ptr().cast()),
                width * 4,
                &props,
            )
        }
        .context("CreateBitmap")?;
        unsafe { self.context2d.BeginDraw() };
        let image = bitmap.cast::<ID2D1Image>().context("ID2D1Image")?;
        unsafe {
            self.context2d.DrawImage(
                &image,
                None,
                None,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                D2D1_COMPOSITE_MODE_SOURCE_COPY,
            );
        }
        unsafe { self.context2d.EndDraw(None, None) }.context("EndDraw")?;
        Ok(())
    }
}

fn bitmap_props(
    options: windows::Win32::Graphics::Direct2D::D2D1_BITMAP_OPTIONS,
) -> D2D1_BITMAP_PROPERTIES1 {
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: options,
        colorContext: core::mem::ManuallyDrop::new(None),
    }
}

fn create_device() -> anyhow::Result<(ID3D11Device, ID3D11DeviceContext)> {
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        let mut context = None;
        let created = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };
        if created.is_ok()
            && let (Some(device), Some(context)) = (device, context)
        {
            return Ok((device, context));
        }
    }
    anyhow::bail!("D3D11CreateDevice failed")
}
