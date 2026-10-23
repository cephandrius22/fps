//! Upscale pass: low-res target → window, nearest-neighbor, letterboxed.
//!
//!   ┌──────────── window ────────────┐
//!   │ bar │      640×360 × k     │ bar │
//!   └────────────────────────────────┘

use anyhow::Result;

use crate::Scaling;
use crate::shader::{checked, compile};
use crate::target::LowRes;

pub(crate) struct BlitPass {
    format: wgpu::TextureFormat,
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
}

impl BlitPass {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        source_target: &LowRes,
        source: &str,
    ) -> Result<Self> {
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit-bind-layout"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = build_pipeline(device, &pipeline_layout, format, source)?;

        // Nearest on both axes: the chunky-pixel look.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit-nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit-bind-group"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source_target.color),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Ok(Self {
            format,
            pipeline_layout,
            pipeline,
            bind_group,
        })
    }

    pub fn reload(&mut self, device: &wgpu::Device, source: &str) -> Result<()> {
        self.pipeline = build_pipeline(device, &self.pipeline_layout, self.format, source)?;
        Ok(())
    }

    pub fn record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface: &wgpu::TextureView,
        surface_size: (u32, u32),
        source_target: &LowRes,
        scaling: Scaling,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // Black letterbox bars.
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });

        let src = (source_target.width, source_target.height);
        let rect = letterbox(src, surface_size, scaling);

        pass.set_viewport(rect.x, rect.y, rect.w, rect.h, 0.0, 1.0);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn build_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    source: &str,
) -> Result<wgpu::RenderPipeline> {
    let module = compile(device, "blit.wgsl", source)?;

    checked(device, "blit pipeline", || {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit-pipeline"),
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
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        })
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// Centered, aspect-preserving rect for `src` inside `dst`.
fn letterbox(src: (u32, u32), dst: (u32, u32), scaling: Scaling) -> Rect {
    let (sw, sh) = (src.0 as f32, src.1 as f32);
    let (dw, dh) = (dst.0 as f32, dst.1 as f32);
    let mut scale = (dw / sw).min(dh / sh);

    // Integer scaling only when at least 1×; smaller windows fall back to fit.
    if scaling == Scaling::Integer && scale >= 1.0 {
        scale = scale.floor();
    }

    let (w, h) = (sw * scale, sh * scale);

    Rect {
        x: ((dw - w) / 2.0).floor(),
        y: ((dh - h) / 2.0).floor(),
        w,
        h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_fit_fills_matching_aspect() {
        let rect = letterbox((640, 360), (1920, 1080), Scaling::Fit);
        assert_eq!(
            rect,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0
            }
        );
    }

    #[test]
    fn letterbox_integer_rounds_down_and_centers() {
        // 2500×1400 fits 3.89×, rounded down to 3× (1920×1080).
        let rect = letterbox((640, 360), (2500, 1400), Scaling::Integer);
        assert_eq!(
            rect,
            Rect {
                x: 290.0,
                y: 160.0,
                w: 1920.0,
                h: 1080.0
            }
        );
    }

    #[test]
    fn letterbox_pillarboxes_wide_window() {
        let rect = letterbox((640, 360), (2000, 720), Scaling::Fit);
        assert_eq!(
            rect,
            Rect {
                x: 360.0,
                y: 0.0,
                w: 1280.0,
                h: 720.0
            }
        );
    }
}
