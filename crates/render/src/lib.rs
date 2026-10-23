//! Renderer. Draws the scene into a low-resolution target, then
//! upscales it to the window with nearest-neighbor sampling.
//!
//!   draws ─► MeshPass ─► low-res color+depth (e.g. 640×360)
//!                                │
//!                                ▼
//!                  BlitPass (nearest, letterboxed) ─► window surface

mod camera;
mod capture;
mod gpu;
mod mesh;
mod passes;
mod shader;
mod target;

use anyhow::Result;
use base::Vfs;

pub use camera::Camera;
pub use capture::Capture;
pub use mesh::{MeshData, MeshId, Vertex};

use gpu::Gpu;
use mesh::MeshStore;
use passes::{BlitPass, MeshPass};
use target::LowRes;

const MESH_SHADER: &str = "shaders/mesh.wgsl";
const BLIT_SHADER: &str = "shaders/blit.wgsl";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VSync {
    On,
    Off,
}

/// How the low-res image fills the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scaling {
    /// Largest whole-number multiple that fits: crisp, uniform pixels.
    Integer,
    /// Fill as much as aspect allows: no wasted border, uneven pixels.
    Fit,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderConfig {
    pub internal_width: u32,
    pub internal_height: u32,
    pub scaling: Scaling,
    pub vsync: VSync,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            internal_width: 640,
            internal_height: 360,
            scaling: Scaling::Fit,
            vsync: VSync::On,
        }
    }
}

/// One mesh instance to draw this frame.
#[derive(Clone, Copy, Debug)]
pub struct Draw {
    pub mesh: MeshId,
    pub model: glam::Mat4,
}

/// Everything needed to render one frame.
pub struct FrameView<'a> {
    pub camera: &'a Camera,
    pub draws: &'a [Draw],
}

pub struct Renderer {
    gpu: Gpu,
    vfs: Vfs,
    config: RenderConfig,
    target: LowRes,
    meshes: MeshStore,
    mesh_pass: MeshPass,
    blit_pass: BlitPass,
}

impl Renderer {
    pub fn new(window: platform::Window, vfs: Vfs, config: RenderConfig) -> Result<Self> {
        let gpu = Gpu::new(window, config.vsync)?;
        let target = LowRes::new(&gpu.device, config.internal_width, config.internal_height);

        let mesh_src = vfs.read_text(MESH_SHADER)?;
        let blit_src = vfs.read_text(BLIT_SHADER)?;
        let mesh_pass = MeshPass::new(&gpu.device, &mesh_src)?;
        let blit_pass = BlitPass::new(&gpu.device, gpu.format(), &target, &blit_src)?;

        Ok(Self {
            gpu,
            vfs,
            config,
            target,
            meshes: MeshStore::default(),
            mesh_pass,
            blit_pass,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.gpu.resize(width, height);
    }

    pub fn create_mesh(&mut self, data: &MeshData) -> MeshId {
        self.meshes.create(&self.gpu.device, data)
    }

    pub fn render(&mut self, view: &FrameView) {
        let Some(frame) = self.gpu.acquire() else {
            return;
        };

        let aspect = self.target.width as f32 / self.target.height as f32;
        let view_proj = view.camera.view_proj(aspect);
        self.mesh_pass
            .prepare(&self.gpu.device, &self.gpu.queue, view_proj, view.draws);

        let surface_view = frame.texture.create_view(&Default::default());
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());

        self.mesh_pass
            .record(&mut encoder, &self.target, &self.meshes, view.draws);
        self.blit_pass.record(
            &mut encoder,
            &surface_view,
            self.gpu.size(),
            &self.target,
            self.config.scaling,
        );

        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(frame);
    }

    /// Last rendered low-res frame, as RGBA8 (sRGB). Blocks on the GPU.
    pub fn capture(&self) -> Result<Capture> {
        capture::read(&self.gpu.device, &self.gpu.queue, &self.target)
    }

    /// Hot reload. Unknown paths are ignored. On error the old version
    /// stays active, so a typo never kills the session.
    pub fn on_asset_changed(&mut self, path: &str) {
        let result = match path {
            MESH_SHADER => self
                .vfs
                .read_text(path)
                .and_then(|src| self.mesh_pass.reload(&self.gpu.device, &src)),
            BLIT_SHADER => self
                .vfs
                .read_text(path)
                .and_then(|src| self.blit_pass.reload(&self.gpu.device, &src)),
            _ => return,
        };

        match result {
            Ok(()) => log::info!("reloaded {path}"),
            Err(err) => log::error!("reload {path} failed, keeping old version: {err:#}"),
        }
    }
}
