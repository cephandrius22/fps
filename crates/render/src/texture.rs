//! Textures: GPU storage, nearest-neighbor sampling, hot replacement.
//!
//! Each texture owns a bind group (texture + shared sampler) so a draw
//! switches textures with one `set_bind_group`. Untextured draws use a
//! 1×1 white fallback, which leaves vertex color unchanged.
//!
//! No mipmaps yet: minified textures shimmer, which is period-correct.

use base::{Image, SlotId, Slots};

use crate::error::{RenderError, Result};

/// sRGB so texels are linearized on sampling, matching the lit output.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const BYTES_PER_PIXEL: u32 = 4;
const WHITE: [u8; 4] = [u8::MAX; 4];

/// Handle to a texture owned by the renderer. Detects use after destroy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(SlotId);

struct GpuTexture {
    // The bind group keeps the texture alive; held for clarity.
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

pub(crate) struct TextureStore {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    fallback: GpuTexture,
    textures: Slots<GpuTexture>,
}

impl TextureStore {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture-bind-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // Nearest for the chunky texel look; repeat so UVs past 1 tile.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture-nearest-repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let fallback = upload(device, queue, &layout, &sampler, &Image::solid(WHITE));

        Self {
            layout,
            sampler,
            fallback,
            textures: Slots::default(),
        }
    }

    /// Bind group layout for group 1 of textured pipelines.
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    pub fn create(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Image,
    ) -> Result<TextureId> {
        check_size(device, image)?;

        let gpu = upload(device, queue, &self.layout, &self.sampler, image);
        Ok(TextureId(self.textures.insert(gpu)))
    }

    /// Swaps in new pixels, any size; the handle stays valid.
    pub fn replace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: TextureId,
        image: &Image,
    ) -> Result<()> {
        check_size(device, image)?;

        let gpu = upload(device, queue, &self.layout, &self.sampler, image);
        let slot = self
            .textures
            .get_mut(id.0)
            .ok_or(RenderError::UnknownTexture)?;
        *slot = gpu;

        Ok(())
    }

    /// Frees a texture. False if the handle was already stale.
    pub fn destroy(&mut self, id: TextureId) -> bool {
        self.textures.remove(id.0).is_some()
    }

    /// Bind group for `id`; the white fallback for None. A destroyed
    /// texture is a caller bug: flagged, then drawn white.
    pub fn bind_group(&self, id: Option<TextureId>) -> &wgpu::BindGroup {
        let Some(id) = id else {
            return &self.fallback.bind_group;
        };

        let texture = self.textures.get(id.0);
        base::verify!(texture.is_some(), "draw uses destroyed texture {id:?}");

        texture.map_or(&self.fallback.bind_group, |t| &t.bind_group)
    }
}

fn check_size(device: &wgpu::Device, image: &Image) -> Result<()> {
    let max = device.limits().max_texture_dimension_2d;
    if image.width() > max || image.height() > max {
        return Err(RenderError::TextureTooLarge {
            width: image.width(),
            height: image.height(),
            max,
        });
    }

    Ok(())
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    image: &Image,
) -> GpuTexture {
    let size = wgpu::Extent3d {
        width: image.width(),
        height: image.height(),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // Unlike buffer readback, texture writes need no row padding.
    queue.write_texture(
        texture.as_image_copy(),
        image.rgba(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width() * BYTES_PER_PIXEL),
            rows_per_image: Some(image.height()),
        },
        size,
    );

    let view = texture.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("texture-bind-group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });

    GpuTexture {
        _texture: texture,
        bind_group,
    }
}
