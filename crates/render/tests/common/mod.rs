//! Shared fixtures for render integration tests.

#![allow(dead_code)]

use std::io::Write;
use std::path::PathBuf;
use std::sync::Once;

use render::{MeshData, RenderConfig, RenderError, Renderer, Shader, Vertex};

/// Set to make a missing GPU fail tests instead of skipping them.
const REQUIRE_GPU_ENV: &str = "REQUIRE_GPU";
/// Set to "true" by GitHub Actions.
const CI_ENV: &str = "GITHUB_ACTIONS";

/// One CI warning per test binary, not one per skipped test.
static ANNOUNCE_SKIP: Once = Once::new();

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

/// Headless renderer, or None (test skips) on machines without a GPU.
pub fn renderer() -> Option<Renderer> {
    let config = RenderConfig {
        internal_width: WIDTH,
        internal_height: HEIGHT,
        ..Default::default()
    };

    match Renderer::headless(config, load_shader) {
        Ok(renderer) => Some(renderer),
        Err(err) => {
            skip_or_fail(err);
            None
        }
    }
}

/// Handles a failed renderer start. Only a missing GPU may skip a test,
/// and not under $REQUIRE_GPU; anything else (e.g. a broken shipped
/// shader) is a real failure.
pub fn skip_or_fail(err: RenderError) {
    let reason = base::chain(&err).to_string();

    if !matches!(err, RenderError::NoAdapter(_)) {
        panic!("renderer failed to start: {reason}");
    }
    if std::env::var_os(REQUIRE_GPU_ENV).is_some() {
        panic!("{REQUIRE_GPU_ENV} set but no GPU: {reason}");
    }

    eprintln!("skipping GPU test: {reason}");
    announce_skip(&reason);
}

/// Turns a skip into a warning on the GitHub Actions run page. libtest
/// swallows print!/eprintln! output from passing tests, but a direct
/// stdout write reaches the CI log, where `::warning` is picked up.
fn announce_skip(reason: &str) {
    if std::env::var(CI_ENV).as_deref() != Ok("true") {
        return;
    }

    ANNOUNCE_SKIP.call_once(|| {
        let os = std::env::consts::OS;
        let _ = writeln!(
            std::io::stdout(),
            "::warning title=GPU tests skipped::No GPU adapter on {os}; headless render tests did not run. {}",
            escape_command(reason)
        );
    });
}

/// Workflow-command escaping: %, CR and LF would end or corrupt the line.
fn escape_command(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
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
