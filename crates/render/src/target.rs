//! Low-resolution offscreen target the scene is drawn into.

pub(crate) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub(crate) struct LowRes {
    pub width: u32,
    pub height: u32,
    pub color_texture: wgpu::Texture,
    pub color: wgpu::TextureView,
    pub depth: wgpu::TextureView,
}

impl LowRes {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        // COPY_SRC: screenshots read this target back.
        let color_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        let color_texture = create(
            device,
            "lowres-color",
            width,
            height,
            COLOR_FORMAT,
            color_usage,
        );
        let color = color_texture.create_view(&Default::default());
        let depth = create(
            device,
            "lowres-depth",
            width,
            height,
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
        .create_view(&Default::default());

        Self {
            width,
            height,
            color_texture,
            color,
            depth,
        }
    }
}

fn create(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}
