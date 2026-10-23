//! Mesh data and GPU storage. 32-bit indices, no vertex caps.

use base::{SlotId, Slots};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Interleaved vertex, object space. Right-handed, Y up; triangles wind
/// counter-clockwise when seen from their front side (back faces are
/// culled). `lightmap_uv` is reserved for baked lighting (L1) so the
/// format does not change when it lands.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    /// Position.
    pub pos: [f32; 3],
    /// Unit normal.
    pub normal: [f32; 3],
    /// Texture coordinate, (0,0) top-left.
    pub uv: [f32; 2],
    /// Lightmap coordinate (unused until baked lighting).
    pub lightmap_uv: [f32; 2],
    /// Linear RGBA tint.
    pub color: [f32; 4],
}

impl Vertex {
    pub(crate) const ATTRIBS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x2,
        4 => Float32x4,
    ];

    pub(crate) fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// Indexed triangle list.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    /// Vertex pool.
    pub vertices: Vec<Vertex>,
    /// Three indices per triangle into `vertices`.
    pub indices: Vec<u32>,
}

/// Handle to a mesh owned by the renderer. Detects use after destroy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshId(SlotId);

pub(crate) struct GpuMesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub index_count: u32,
}

#[derive(Default)]
pub(crate) struct MeshStore {
    meshes: Slots<GpuMesh>,
}

impl MeshStore {
    pub fn create(&mut self, device: &wgpu::Device, data: &MeshData) -> MeshId {
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh-vertices"),
            contents: bytemuck::cast_slice(&data.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh-indices"),
            contents: bytemuck::cast_slice(&data.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        MeshId(self.meshes.insert(GpuMesh {
            vertices,
            indices,
            index_count: data.indices.len() as u32,
        }))
    }

    /// Buffers drop here; wgpu frees them once in-flight frames finish.
    pub fn destroy(&mut self, id: MeshId) -> bool {
        self.meshes.remove(id.0).is_some()
    }

    pub fn get(&self, id: MeshId) -> Option<&GpuMesh> {
        self.meshes.get(id.0)
    }
}

#[cfg(test)]
mod tests {
    use std::mem::offset_of;

    use super::*;

    /// Shader locations read the vertex at these byte offsets; a field
    /// reorder must fail here, not as garbage on screen.
    #[test]
    fn attribute_offsets_match_struct() {
        let offsets = [
            offset_of!(Vertex, pos),
            offset_of!(Vertex, normal),
            offset_of!(Vertex, uv),
            offset_of!(Vertex, lightmap_uv),
            offset_of!(Vertex, color),
        ];

        for (attrib, offset) in Vertex::ATTRIBS.iter().zip(offsets) {
            assert_eq!(
                attrib.offset as usize, offset,
                "location {}",
                attrib.shader_location
            );
        }
        assert_eq!(Vertex::layout().array_stride as usize, size_of::<Vertex>());
    }
}
