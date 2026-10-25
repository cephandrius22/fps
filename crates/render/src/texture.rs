//! Textures: GPU storage, sampling, hot replacement. Two kinds share one
//! bind group layout but differ in sampling:
//!
//! ```text
//!   Albedo    nearest, repeat, mips per setting   fallback: 1×1 white
//!   Lightmap  bilinear, clamp, no mips             fallback: 1×1, alpha 0
//! ```
//!
//! Each texture owns a bind group (texture + its kind's sampler) so a
//! draw switches textures with one `set_bind_group`. The white albedo
//! fallback leaves vertex color unchanged; the lightmap fallback's zero
//! alpha tells the shader "no baked light, use the sun term".
//!
//! Mipmaps are a setting ([`Mipmaps`]): off, distant surfaces shimmer;
//! on, the chain is built on the CPU at upload time.
//!
//! ```text
//! 64×64 ─half─► 32×32 ─half─► … ─► 1×1      (Image::half, linear light)
//! ```

use base::{Image, SlotId, Slots};

use crate::Mipmaps;
use crate::error::{RenderError, Result};

/// sRGB so texels are linearized on sampling, matching the lit output.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const BYTES_PER_PIXEL: u32 = 4;
const WHITE: [u8; 4] = [u8::MAX; 4];
/// Alpha 0 marks "no lightmap" to mesh.wgsl.
const UNLIT: [u8; 4] = [0; 4];

/// How a texture is sampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextureKind {
    /// Surface colors: chunky nearest-neighbor texels.
    Albedo,
    /// Baked light: smooth between luxels, as GLQuake drew it.
    Lightmap,
}

/// Handle to a texture owned by the renderer. Detects use after destroy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(SlotId);

struct GpuTexture {
    // The bind group keeps the texture alive; held for clarity.
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    kind: TextureKind,
}

pub(crate) struct TextureStore {
    layout: wgpu::BindGroupLayout,
    albedo_sampler: wgpu::Sampler,
    lightmap_sampler: wgpu::Sampler,
    mipmaps: Mipmaps,
    white: GpuTexture,
    unlit: GpuTexture,
    textures: Slots<GpuTexture>,
}

impl TextureStore {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, mipmaps: Mipmaps) -> Self {
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
        // Mip selection is the only filtering the setting may add.
        let albedo_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture-nearest-repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: mip_filter(mipmaps),
            ..Default::default()
        });
        // Bilinear: light fades smoothly across a face. Clamp, so an atlas
        // edge never wraps to the other side.
        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lightmap-linear-clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // 1×1 has no smaller mips, so neither fallback needs any.
        let fallback = |pixel, sampler, kind| {
            let image = Image::solid(pixel);
            create_gpu_texture(device, queue, &layout, sampler, &image, &[], kind)
        };
        let white = fallback(WHITE, &albedo_sampler, TextureKind::Albedo);
        let unlit = fallback(UNLIT, &lightmap_sampler, TextureKind::Lightmap);

        Self {
            layout,
            albedo_sampler,
            lightmap_sampler,
            mipmaps,
            white,
            unlit,
            textures: Slots::default(),
        }
    }

    /// Bind group layout for groups 1 (albedo) and 2 (lightmap) of
    /// textured pipelines: a texture and its sampler.
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    pub fn create(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Image,
        kind: TextureKind,
    ) -> Result<TextureId> {
        check_size(device, image)?;

        let gpu = self.upload(device, queue, image, kind);
        Ok(TextureId(self.textures.insert(gpu)))
    }

    /// Swaps in new pixels, any size; the handle and kind stay.
    pub fn replace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: TextureId,
        image: &Image,
    ) -> Result<()> {
        check_size(device, image)?;

        let kind = self
            .textures
            .get(id.0)
            .ok_or(RenderError::UnknownTexture)?
            .kind;
        let gpu = self.upload(device, queue, image, kind);
        if let Some(slot) = self.textures.get_mut(id.0) {
            *slot = gpu;
        }

        Ok(())
    }

    fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Image,
        kind: TextureKind,
    ) -> GpuTexture {
        let (sampler, mips) = match kind {
            TextureKind::Albedo => (&self.albedo_sampler, smaller_mips(image, self.mipmaps)),
            TextureKind::Lightmap => (&self.lightmap_sampler, Vec::new()),
        };

        create_gpu_texture(device, queue, &self.layout, sampler, image, &mips, kind)
    }

    /// Frees a texture. False if the handle was already stale.
    pub fn destroy(&mut self, id: TextureId) -> bool {
        self.textures.remove(id.0).is_some()
    }

    /// Bind group for `id`, or `kind`'s fallback for None. A destroyed
    /// texture is a caller bug: flagged, then drawn as the fallback.
    pub fn bind_group(&self, id: Option<TextureId>, kind: TextureKind) -> &wgpu::BindGroup {
        let fallback = match kind {
            TextureKind::Albedo => &self.white.bind_group,
            TextureKind::Lightmap => &self.unlit.bind_group,
        };
        let Some(id) = id else {
            return fallback;
        };

        let texture = self.textures.get(id.0);
        base::verify!(texture.is_some(), "draw uses destroyed texture {id:?}");

        texture.map_or(fallback, |t| &t.bind_group)
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

fn mip_filter(mipmaps: Mipmaps) -> wgpu::MipmapFilterMode {
    match mipmaps {
        // Irrelevant with a single level; Nearest is the default.
        Mipmaps::Off | Mipmaps::Nearest => wgpu::MipmapFilterMode::Nearest,
        Mipmaps::Linear => wgpu::MipmapFilterMode::Linear,
    }
}

/// Mip levels below `image`, largest first, down to 1×1; none when
/// mipmaps are off. Sizes follow the GPU's rule, max(1, size >> level).
fn smaller_mips(image: &Image, mipmaps: Mipmaps) -> Vec<Image> {
    let mut mips: Vec<Image> = Vec::new();
    if mipmaps == Mipmaps::Off {
        return mips;
    }

    loop {
        let last = mips.last().unwrap_or(image);
        if last.width() == 1 && last.height() == 1 {
            return mips;
        }

        let next = last.half();
        mips.push(next);
    }
}

/// Creates a texture of `kind` from `base` plus its smaller `mips`, with
/// the bind group draws use.
fn create_gpu_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    base: &Image,
    mips: &[Image],
    kind: TextureKind,
) -> GpuTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture"),
        size: extent(base),
        mip_level_count: 1 + mips.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // Unlike buffer readback, texture writes need no row padding.
    let levels = std::iter::once(base).chain(mips);
    for (level, image) in levels.enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.rgba(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width() * BYTES_PER_PIXEL),
                rows_per_image: Some(image.height()),
            },
            extent(image),
        );
    }

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
        kind,
    }
}

fn extent(image: &Image) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: image.width(),
        height: image.height(),
        depth_or_array_layers: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(width: u32, height: u32) -> Image {
        Image::new(width, height, vec![0; (width * height * 4) as usize]).unwrap()
    }

    fn sizes(levels: &[Image]) -> Vec<(u32, u32)> {
        levels.iter().map(|l| (l.width(), l.height())).collect()
    }

    #[test]
    fn mipmaps_off_adds_no_levels() {
        assert!(smaller_mips(&blank(64, 64), Mipmaps::Off).is_empty());
    }

    #[test]
    fn mips_run_down_to_one_pixel() {
        let mips = smaller_mips(&blank(8, 2), Mipmaps::Nearest);
        assert_eq!(sizes(&mips), [(4, 1), (2, 1), (1, 1)]);
    }

    #[test]
    fn mips_round_odd_sizes_down() {
        let mips = smaller_mips(&blank(5, 3), Mipmaps::Linear);
        assert_eq!(sizes(&mips), [(2, 1), (1, 1)]);
        assert!(smaller_mips(&Image::solid(WHITE), Mipmaps::Nearest).is_empty());
    }
}
