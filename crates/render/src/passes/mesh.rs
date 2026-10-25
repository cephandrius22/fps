//! Opaque mesh pass into the low-res target.
//!
//! Per-draw model matrices live in one storage buffer; draw `i` reads
//! entry `i` via `instance_index`, so there is one upload per frame.
//!
//! ```text
//!   group 0: frame uniforms + model matrices (once per pass)
//!   group 1: texture + sampler (only when the texture changes)
//! ```

use crate::error::Result;
use glam::{Mat4, Vec4};

use crate::mesh::{MeshStore, Vertex};
use crate::shader::{checked, compile};
use crate::target::{COLOR_FORMAT, DEPTH_FORMAT, LowRes};
use crate::texture::TextureStore;
use crate::{Camera, Draw, Falloff, Fog};

const INITIAL_CAPACITY: usize = 256;
/// Background without fog: near-black, a little blue.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.02,
    b: 0.03,
    a: 1.0,
};

/// `Frame.fog_mode` values; must match the FOG_* constants in mesh.wgsl.
mod fog_mode {
    pub const OFF: u32 = 0;
    pub const LINEAR: u32 = 1;
    pub const EXP: u32 = 2;
    pub const EXP2: u32 = 3;
}

/// Must match `Frame` in mesh.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniforms {
    view_proj: Mat4,
    /// World-space camera position; w unused.
    eye: Vec4,
    /// Linear RGB; a unused.
    fog_color: Vec4,
    fog_mode: u32,
    fog_start: f32,
    fog_end: f32,
    fog_density: f32,
}

impl FrameUniforms {
    fn new(camera: &Camera, aspect: f32, fog: Option<Fog>) -> Self {
        let mut frame = Self {
            view_proj: camera.view_proj(aspect),
            eye: camera.eye().extend(1.0),
            fog_color: Vec4::ZERO,
            fog_mode: fog_mode::OFF,
            fog_start: 0.0,
            fog_end: 0.0,
            fog_density: 0.0,
        };
        let Some(fog) = fog else {
            return frame;
        };

        frame.fog_color = Vec4::from((glam::Vec3::from(fog.color), 1.0));
        match fog.falloff {
            Falloff::Linear { start, end } => {
                frame.fog_mode = fog_mode::LINEAR;
                frame.fog_start = start;
                frame.fog_end = end;
            }
            Falloff::Exp { density } => {
                frame.fog_mode = fog_mode::EXP;
                frame.fog_density = density;
            }
            Falloff::Exp2 { density } => {
                frame.fog_mode = fog_mode::EXP2;
                frame.fog_density = density;
            }
        }

        frame
    }
}

/// Empty background: the fog color when foggy, so geometry fading into
/// fog meets a matching sky instead of a silhouette.
fn background(fog: Option<Fog>) -> wgpu::Color {
    let Some(fog) = fog else {
        return CLEAR;
    };

    let [r, g, b] = fog.color.map(f64::from);
    wgpu::Color { r, g, b, a: 1.0 }
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
    /// This frame's background, set by `prepare`.
    clear: wgpu::Color,
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
            clear: CLEAR,
        })
    }

    pub fn reload(&mut self, device: &wgpu::Device, source: &str) -> Result<()> {
        self.pipeline = build_pipeline(device, &self.pipeline_layout, source)?;
        Ok(())
    }

    /// Uploads camera, fog and model matrices. Grows the buffer if
    /// needed.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        aspect: f32,
        fog: Option<Fog>,
        draws: &[Draw],
    ) {
        if draws.len() > self.capacity {
            self.capacity = draws.len().next_power_of_two();
            self.model_buf = create_model_buf(device, self.capacity);
            self.bind_group =
                create_bind_group(device, &self.bind_layout, &self.frame_buf, &self.model_buf);
        }

        let frame = FrameUniforms::new(camera, aspect, fog);
        queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&frame));
        self.clear = background(fog);

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
                view: &target.scene,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.clear),
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

    use glam::Vec3;

    const FOG_COLOR: [f32; 3] = [0.1, 0.2, 0.3];

    fn camera() -> Camera {
        Camera::look_at(Vec3::new(1.0, 2.0, 3.0), Vec3::ZERO, 1.0)
    }

    fn fog(falloff: Falloff) -> Option<Fog> {
        Some(Fog {
            color: FOG_COLOR,
            falloff,
        })
    }

    /// WGSL layout: mat4 (64) + two vec4 (32) + four scalars (16).
    #[test]
    fn frame_uniforms_match_wgsl_layout() {
        assert_eq!(size_of::<FrameUniforms>(), 112);
        assert_eq!(size_of::<FrameUniforms>() % 16, 0);
    }

    #[test]
    fn uniforms_carry_eye_and_no_fog_by_default() {
        let frame = FrameUniforms::new(&camera(), 1.0, None);

        assert!((frame.eye.truncate() - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-4);
        assert_eq!(frame.fog_mode, fog_mode::OFF);
    }

    #[test]
    fn each_falloff_encodes_its_mode_and_parameters() {
        let linear = FrameUniforms::new(
            &camera(),
            1.0,
            fog(Falloff::Linear {
                start: 2.0,
                end: 9.0,
            }),
        );
        assert_eq!(linear.fog_mode, fog_mode::LINEAR);
        assert_eq!((linear.fog_start, linear.fog_end), (2.0, 9.0));
        assert_eq!(linear.fog_color, Vec4::new(0.1, 0.2, 0.3, 1.0));

        let exp = FrameUniforms::new(&camera(), 1.0, fog(Falloff::Exp { density: 0.5 }));
        assert_eq!((exp.fog_mode, exp.fog_density), (fog_mode::EXP, 0.5));

        let exp2 = FrameUniforms::new(&camera(), 1.0, fog(Falloff::Exp2 { density: 0.25 }));
        assert_eq!((exp2.fog_mode, exp2.fog_density), (fog_mode::EXP2, 0.25));
    }

    #[test]
    fn background_is_fog_color_when_foggy() {
        assert_eq!(background(None), CLEAR);

        let bg = background(fog(Falloff::Exp { density: 1.0 }));
        assert_eq!([bg.r, bg.g, bg.b], FOG_COLOR.map(f64::from));
    }
}
