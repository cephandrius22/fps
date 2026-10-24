//! Shared fixtures for render integration tests.

#![allow(dead_code)]

use std::path::PathBuf;

use render::test_support::GpuTest;
use render::{MeshData, RenderConfig, Shader, Vertex};

pub const WIDTH: u32 = 64;
pub const HEIGHT: u32 = 64;

pub fn shader_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/shaders")
}

pub fn shader_file(shader: Shader) -> &'static str {
    match shader {
        Shader::Mesh => "mesh.wgsl",
        Shader::Blit => "blit.wgsl",
    }
}

pub fn load_shader(shader: Shader) -> std::io::Result<String> {
    std::fs::read_to_string(shader_dir().join(shader_file(shader)))
}

/// Small headless renderer, or None (test skips) without a GPU.
pub fn renderer() -> Option<GpuTest> {
    let config = RenderConfig {
        internal_width: WIDTH,
        internal_height: HEIGHT,
        ..Default::default()
    };

    render::test_support::headless_or_skip(config, load_shader)
}

/// 2×2 quad in the XY plane at z = 0, front face toward +Z. UVs span
/// the whole texture, (0,0) at the top-left corner.
pub fn quad(color: [f32; 4]) -> MeshData {
    let corners: [[f32; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    let vertices = corners
        .iter()
        .map(|&[x, y]| Vertex {
            pos: [x, y, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [(x + 1.0) / 2.0, (1.0 - y) / 2.0],
            color,
            ..Default::default()
        })
        .collect();

    MeshData {
        vertices,
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}
