//! Window surface (swapchain): format choice, resize, frame acquire.

use anyhow::{Context, Result};

use crate::VSync;
use crate::gpu::Gpu;

pub(crate) struct Swapchain {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Set when a frame came back suboptimal. Reconfigure before the next
    /// acquire; it cannot happen while that frame is alive.
    stale: bool,
}

impl Swapchain {
    pub fn new(
        surface: wgpu::Surface<'static>,
        gpu: &Gpu,
        size: (u32, u32),
        vsync: VSync,
    ) -> Result<Self> {
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| caps.formats.first().copied())
            .context("surface has no formats")?;

        let mut config = surface
            .get_default_config(&gpu.adapter, size.0.max(1), size.1.max(1))
            .context("surface unsupported by adapter")?;
        config.format = format;
        config.present_mode = present_mode(vsync);
        surface.configure(&gpu.device, &config);

        Ok(Self {
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

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        // Minimized windows report 0×0, which is invalid for a surface.
        if width == 0 || height == 0 {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(device, &self.config);
    }

    /// Next swapchain image, or None when this frame should be skipped.
    pub fn acquire(&mut self, device: &wgpu::Device) -> Option<wgpu::SurfaceTexture> {
        if self.stale {
            self.surface.configure(device, &self.config);
            self.stale = false;
        }

        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.stale = true;
                Some(frame)
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(device, &self.config);
                None
            }
            other => {
                log::debug!("skipping frame: {other:?}");
                None
            }
        }
    }
}

fn present_mode(vsync: VSync) -> wgpu::PresentMode {
    match vsync {
        VSync::On => wgpu::PresentMode::AutoVsync,
        VSync::Off => wgpu::PresentMode::AutoNoVsync,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vsync_maps_to_auto_modes() {
        assert_eq!(present_mode(VSync::On), wgpu::PresentMode::AutoVsync);
        assert_eq!(present_mode(VSync::Off), wgpu::PresentMode::AutoNoVsync);
    }
}
