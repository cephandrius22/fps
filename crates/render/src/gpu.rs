//! GPU adapter, device and queue. Window-independent, so the renderer
//! can also run headless (tests, tools).

use std::sync::Arc;

use crate::error::Result;

pub(crate) struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Picks an adapter, preferring one that can present to `surface`.
    pub fn new(instance: &wgpu::Instance, surface: Option<&wgpu::Surface>) -> Result<Self> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface,
            ..Default::default()
        }))?;

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

        Ok(Self {
            adapter,
            device,
            queue,
        })
    }
}
