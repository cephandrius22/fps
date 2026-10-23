//! Shared fixtures for render integration tests.

#![allow(dead_code)]

use std::path::PathBuf;

use anyhow::Result;
use render::{MeshData, RenderConfig, Renderer, Shader, Vertex};

/// Set to make a missing GPU fail tests instead of skipping them.
const REQUIRE_GPU_ENV: &str = "REQUIRE_GPU";

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

pub fn load_shader(shader: Shader) -> Result<String> {
    Ok(std::fs::read_to_string(
        shader_dir().join(shader_file(shader)),
    )?)
}

/// Headless renderer, or None (test skips) on machines without a GPU.
pub fn renderer() -> Option<Renderer> {
    let config = RenderConfig {
        internal_width: WIDTH,
        internal_height: HEIGHT,
        ..Default::default()
    };

    match Renderer::headless(config, load_shader) {
        Ok(renderer) => Some(renderer),
        Err(err) if std::env::var_os(REQUIRE_GPU_ENV).is_none() => {
            eprintln!("skipping GPU test: {err:#}");
            None
        }
        Err(err) => panic!("{REQUIRE_GPU_ENV} set but no GPU: {err:#}"),
    }
}

/// 2×2 quad in the XY plane at z = 0, front face toward +Z.
pub fn quad(color: [f32; 4]) -> MeshData {
    let corners = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    let vertices = corners
        .iter()
        .map(|&[x, y]| Vertex {
            pos: [x, y, 0.0],
            normal: [0.0, 0.0, 1.0],
            color,
            ..Default::default()
        })
        .collect();

    MeshData {
        vertices,
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}
