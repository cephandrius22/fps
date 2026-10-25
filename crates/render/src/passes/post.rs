//! Retro post-pass at low res: palette quantization and ordered dither.
//!
//! ```text
//!   scene (raw view) ─► post.wgsl ─► output
//!                          ▲
//!            uniforms: mode + up to 256 palette colors
//! ```
//!
//! Always runs, even with quantization off (a plain copy), so the blit
//! and capture read one fixed target.

use base::Image;

use crate::Quantize;
use crate::error::{RenderError, Result};
use crate::shader::{checked, compile};
use crate::target::{LowRes, RAW_FORMAT};

/// Palette entries the shader holds; 16×16, as in classic 8-bit games.
pub(crate) const MAX_COLORS: usize = 256;

/// `Post.mode` values; must match the QUANTIZE_* constants in post.wgsl.
mod quantize_mode {
    pub const OFF: u32 = 0;
    pub const NEAREST: u32 = 1;
    pub const DITHERED: u32 = 2;
}

/// Must match `Post` in post.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct PostUniforms {
    mode: u32,
    count: u32,
    /// WGSL aligns the array to 16 bytes.
    _pad: [u32; 2],
    /// sRGB-encoded RGB in 0..1; a unused.
    colors: [[f32; 4]; MAX_COLORS],
}

impl PostUniforms {
    fn empty() -> Self {
        bytemuck::Zeroable::zeroed()
    }

    /// Loads `image`'s pixels, row-major, as the palette. Checks before
    /// writing, so a rejected image leaves the old palette.
    fn set_palette(&mut self, image: &Image) -> Result<()> {
        let pixels = image.rgba().as_chunks::<4>().0;
        if pixels.len() > MAX_COLORS {
            return Err(RenderError::PaletteTooLarge {
                colors: pixels.len(),
                max: MAX_COLORS,
            });
        }

        let unorm = |c: u8| f32::from(c) / f32::from(u8::MAX);
        for (slot, &[r, g, b, _]) in self.colors.iter_mut().zip(pixels) {
            *slot = [unorm(r), unorm(g), unorm(b), 1.0];
        }
        self.count = pixels.len() as u32;

        Ok(())
    }
}

fn mode(quantize: Quantize) -> u32 {
    match quantize {
        Quantize::Off => quantize_mode::OFF,
        Quantize::Nearest => quantize_mode::NEAREST,
        Quantize::Dithered => quantize_mode::DITHERED,
    }
}

pub(crate) struct PostPass {
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// CPU copy of the uniforms; uploaded only when it changes.
    uniforms: PostUniforms,
    dirty: bool,
}

impl PostPass {
    pub fn new(device: &wgpu::Device, target: &LowRes, source: &str) -> Result<Self> {
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post-bind-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = build_pipeline(device, &pipeline_layout, source)?;

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post-uniforms"),
            size: size_of::<PostUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post-bind-group"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&target.scene_raw),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform_buf.as_entire_binding(),
                },
            ],
        });

        Ok(Self {
            pipeline_layout,
            pipeline,
            uniform_buf,
            bind_group,
            uniforms: PostUniforms::empty(),
            dirty: true,
        })
    }

    pub fn reload(&mut self, device: &wgpu::Device, source: &str) -> Result<()> {
        self.pipeline = build_pipeline(device, &self.pipeline_layout, source)?;
        Ok(())
    }

    /// Replaces the palette; on error the old one stays.
    pub fn set_palette(&mut self, image: &Image) -> Result<()> {
        self.uniforms.set_palette(image)?;
        self.dirty = true;
        Ok(())
    }

    /// Uploads this frame's mode, plus the palette if it changed.
    pub fn prepare(&mut self, queue: &wgpu::Queue, quantize: Quantize) {
        let mode = mode(quantize);
        if mode != self.uniforms.mode {
            self.uniforms.mode = mode;
            self.dirty = true;
        }
        if !self.dirty {
            return;
        }

        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&self.uniforms));
        self.dirty = false;
    }

    pub fn record(&self, encoder: &mut wgpu::CommandEncoder, target: &LowRes) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // Every pixel is overwritten.
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn build_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    source: &str,
) -> Result<wgpu::RenderPipeline> {
    let module = compile(device, "post.wgsl", source)?;

    checked(device, "post pipeline", || {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post-pipeline"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(RAW_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WGSL layout: two u32, padding to 16, then 256 × vec4.
    #[test]
    fn uniforms_match_wgsl_layout() {
        assert_eq!(size_of::<PostUniforms>(), 16 + MAX_COLORS * 16);
        assert_eq!(std::mem::offset_of!(PostUniforms, colors), 16);
    }

    #[test]
    fn palette_reads_pixels_row_major_as_unorm() {
        let image = Image::new(2, 1, vec![255, 0, 0, 9, 0, 51, 255, 9]).unwrap();
        let mut uniforms = PostUniforms::empty();

        uniforms.set_palette(&image).unwrap();

        assert_eq!(uniforms.count, 2);
        assert_eq!(uniforms.colors[0], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(uniforms.colors[1], [0.0, 0.2, 1.0, 1.0]);
    }

    #[test]
    fn oversized_palette_is_rejected_and_old_one_kept() {
        let mut uniforms = PostUniforms::empty();
        uniforms.set_palette(&Image::solid([1, 2, 3, 4])).unwrap();

        let big = Image::new(17, 16, vec![0; 17 * 16 * 4]).unwrap();
        let err = uniforms.set_palette(&big).unwrap_err();

        assert!(
            matches!(err, RenderError::PaletteTooLarge { colors: 272, .. }),
            "{err:?}"
        );
        assert_eq!(uniforms.count, 1);
    }

    #[test]
    fn every_quantize_mode_has_a_distinct_code() {
        let codes = [Quantize::Off, Quantize::Nearest, Quantize::Dithered].map(mode);
        assert_eq!(
            codes,
            [
                quantize_mode::OFF,
                quantize_mode::NEAREST,
                quantize_mode::DITHERED
            ]
        );
    }
}
