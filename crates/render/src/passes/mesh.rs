//! Opaque mesh pass into the low-res target.
//!
//! Per-draw model matrices live in one storage buffer; draw `i` reads
//! entry `i` via `instance_index`, so there is one upload per frame.
//!
//!   group 0: frame uniforms + model matrices (once per pass)
//!   group 1: texture + sampler (only when the texture changes)

use crate::error::Result;
use glam::Mat4;

use crate::Draw;
use crate::mesh::{MeshStore, Vertex};
use crate::shader::{checked, compile};
use crate::target::{COLOR_FORMAT, DEPTH_FORMAT, LowRes};
use crate::texture::TextureStore;

const INITIAL_CAPACITY: usize = 256;
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.02,
    b: 0.03,
    a: 1.0,
};

/// Must match `Frame` in mesh.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniforms {
    view_proj: Mat4,
}

pub(crate) struct MeshPass {
    bind_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
    frame_buf: wgpu::Buffer,
    model_buf: wgpu::Buffer,
    capacity: usize,
    bind_group: wgpu::BindGroup,
    /// Reused each frame to avoid a per-frame allocation.
    models: Vec<Mat4>,
}

impl MeshPass {
    /// `texture_layout` is group 1; see [`TextureStore::layout`].
    pub fn new(
        device: &wgpu::Device,
        texture_layout: &wgpu::BindGroupLayout,
        source: &str,
    ) -> Result<Self> {
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh-bind-layout"),
            entries: &[
                layout_entry(0, wgpu::BufferBindingType::Uniform),
                layout_entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_layout), Some(texture_layout)],
            immediate_size: 0,
        });
        let pipeline = build_pipeline(device, &pipeline_layout, source)?;

        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mesh-frame"),
            size: size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let model_buf = create_model_buf(device, INITIAL_CAPACITY);
        let bind_group = create_bind_group(device, &bind_layout, &frame_buf, &model_buf);

        Ok(Self {
            bind_layout,
            pipeline_layout,
            pipeline,
            frame_buf,
            model_buf,
            capacity: INITIAL_CAPACITY,
            bind_group,
            models: Vec::with_capacity(INITIAL_CAPACITY),
        })
    }

    pub fn reload(&mut self, device: &wgpu::Device, source: &str) -> Result<()> {
        self.pipeline = build_pipeline(device, &self.pipeline_layout, source)?;
        Ok(())
    }

    /// Uploads camera and model matrices. Grows the buffer if needed.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_proj: Mat4,
        draws: &[Draw],
    ) {
        if draws.len() > self.capacity {
            self.capacity = draws.len().next_power_of_two();
            self.model_buf = create_model_buf(device, self.capacity);
            self.bind_group =
                create_bind_group(device, &self.bind_layout, &self.frame_buf, &self.model_buf);
        }

        let frame = FrameUniforms { view_proj };
        queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&frame));

        self.models.clear();
        self.models.extend(draws.iter().map(|draw| draw.model));
        queue.write_buffer(&self.model_buf, 0, bytemuck::cast_slice(&self.models));
    }

    pub fn record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &LowRes,
        meshes: &MeshStore,
        textures: &TextureStore,
        draws: &[Draw],
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mesh-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);

        // None = nothing bound yet; Some(None) = the white fallback.
        let mut bound_texture = None;

        for (i, draw) in draws.iter().enumerate() {
            // A destroyed mesh is a caller bug: flag it, skip the draw.
            let mesh = meshes.get(draw.mesh);
            base::verify!(mesh.is_some(), "draw uses destroyed mesh {:?}", draw.mesh);
            let Some(mesh) = mesh else {
                continue;
            };

            if bound_texture != Some(draw.texture) {
                pass.set_bind_group(1, textures.bind_group(draw.texture), &[]);
                bound_texture = Some(draw.texture);
            }

            let instance = i as u32;
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, instance..instance + 1);
        }
    }
}

fn build_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    source: &str,
) -> Result<wgpu::RenderPipeline> {
    let module = compile(device, "mesh.wgsl", source)?;

    checked(device, "mesh pipeline", || {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh-pipeline"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(Vertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(COLOR_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        })
    })
}

fn layout_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn create_model_buf(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mesh-models"),
        size: (capacity * size_of::<Mat4>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    frame_buf: &wgpu::Buffer,
    model_buf: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mesh-bind-group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: model_buf.as_entire_binding(),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WGSL `mat4x4<f32>` is 64 bytes; uniform structs must match exactly.
    #[test]
    fn frame_uniforms_match_wgsl_layout() {
        assert_eq!(size_of::<FrameUniforms>(), 64);
        assert_eq!(size_of::<Mat4>(), 64);
    }
}
