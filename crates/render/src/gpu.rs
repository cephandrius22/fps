//! GPU device and window surface (swapchain).

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::VSync;

pub(crate) struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Set when a frame came back suboptimal. Reconfigure before the next
    /// acquire; it cannot happen while that frame is alive.
    stale: bool,
}

impl Gpu {
    pub fn new(window: platform::Window, vsync: VSync) -> Result<Self> {
        let desc =
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone()));
        let instance = wgpu::Instance::new(desc);
        let surface = instance.create_surface(window.clone())?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .context("no compatible GPU adapter")?;

        let info = adapter.get_info();
        log::info!("GPU: {} ({:?})", info.name, info.backend);

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("device"),
                ..Default::default()
            }))?;

        // Log instead of wgpu's default panic; scoped checks still catch
        // errors where we can recover (see shader.rs).
        device.on_uncaptured_error(Arc::new(|err| log::error!("wgpu: {err}")));

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(caps.formats[0]);
        let present_mode = match vsync {
            VSync::On => wgpu::PresentMode::AutoVsync,
            VSync::Off => wgpu::PresentMode::AutoNoVsync,
        };

        let (width, height) = window.size();
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .context("surface unsupported by adapter")?;
        config.format = format;
        config.present_mode = present_mode;
        surface.configure(&device, &config);

        Ok(Self {
            device,
            queue,
            surface,
            config,
            stale: false,
        })
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        // Minimized windows report 0×0, which is invalid for a surface.
        if width == 0 || height == 0 {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Next swapchain image, or None when this frame should be skipped.
    pub fn acquire(&mut self) -> Option<wgpu::SurfaceTexture> {
        if self.stale {
            self.surface.configure(&self.device, &self.config);
            self.stale = false;
        }

        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.stale = true;
                Some(frame)
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                None
            }
            other => {
                log::debug!("skipping frame: {other:?}");
                None
            }
        }
    }
}
