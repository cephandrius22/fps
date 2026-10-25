//! Low-resolution offscreen targets the frame is drawn into.
//!
//! ```text
//!   scene  (sRGB)  ◄── MeshPass writes linear, stored encoded
//!     │ raw view: PostPass reads the encoded bytes as is
//!     ▼
//!   output (raw)   ◄── PostPass writes encoded bytes (exact palette colors)
//!     │ sRGB view: BlitPass reads linear for the sRGB window surface
//!     ▼
//!   capture copies `output` bytes
//! ```
//!
//! One texture, two views of the same bytes: the post-pass works on
//! encoded values, where palettes are defined and dithering steps are
//! even, while lighting and the window stay linear.

/// Scene color: lit output is linear, stored sRGB-encoded.
pub(crate) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Same bytes without the sRGB conversion.
pub(crate) const RAW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub(crate) struct LowRes {
    pub width: u32,
    pub height: u32,
    /// Mesh pass color attachment.
    pub scene: wgpu::TextureView,
    /// Scene bytes as stored, for the post-pass.
    pub scene_raw: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    /// Final low-res image; capture reads it.
    pub output_texture: wgpu::Texture,
    /// Post-pass color attachment.
    pub output: wgpu::TextureView,
    /// Output decoded to linear, for the blit.
    pub output_srgb: wgpu::TextureView,
}

impl LowRes {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let size = (width, height);
        let sampled = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;

        let scene_texture = create(device, "lowres-scene", size, COLOR_FORMAT, sampled);
        let scene = scene_texture.create_view(&Default::default());
        let scene_raw = view_as(&scene_texture, RAW_FORMAT);

        let depth = create(
            device,
            "lowres-depth",
            size,
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
        .create_view(&Default::default());

        // COPY_SRC: screenshots read this target back.
        let output_usage = sampled | wgpu::TextureUsages::COPY_SRC;
        let output_texture = create(device, "lowres-output", size, RAW_FORMAT, output_usage);
        let output = output_texture.create_view(&Default::default());
        let output_srgb = view_as(&output_texture, COLOR_FORMAT);

        Self {
            width,
            height,
            scene,
            scene_raw,
            depth,
            output_texture,
            output,
            output_srgb,
        }
    }
}

fn view_as(texture: &wgpu::Texture, format: wgpu::TextureFormat) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(format),
        ..Default::default()
    })
}

fn create(
    device: &wgpu::Device,
    label: &str,
    (width, height): (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    // Views may toggle sRGB-ness. wgpu ignores entries equal to `format`,
    // so depth gets none.
    let view_formats = [format.add_srgb_suffix(), format.remove_srgb_suffix()];

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
        view_formats: &view_formats,
    })
}
