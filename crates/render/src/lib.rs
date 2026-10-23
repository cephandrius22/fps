//! Renderer. Draws the scene into a low-resolution target, then
//! upscales it to the window with nearest-neighbor sampling.
//!
//!   draws ─► MeshPass ─► low-res color+depth (e.g. 640×360)
//!                                │
//!                                ▼
//!                  BlitPass (nearest, letterboxed) ─► window surface
//!
//! Knows nothing about files or windowing libraries: shader source
//! comes from the caller, and any raw window handle works as a target.
//! Headless mode skips the window and renders only the low-res target.

mod camera;
mod capture;
mod error;
mod gpu;
mod mesh;
mod passes;
mod shader;
mod swapchain;
mod target;

use std::fmt::Debug;

use base::BoxError;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

pub use camera::Camera;
pub use capture::Capture;
pub use error::RenderError;
pub use mesh::{MeshData, MeshId, Vertex};

use error::Result;
use gpu::Gpu;
use mesh::MeshStore;
use passes::{BlitPass, MeshPass};
use swapchain::Swapchain;
use target::{COLOR_FORMAT, LowRes};

/// Shader programs the renderer needs. The caller decides where their
/// source lives (see [`Renderer::new`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shader {
    /// Opaque scene geometry.
    Mesh,
    /// Low-res → window upscale.
    Blit,
}

impl Shader {
    /// Every shader, for loading all of them up front.
    pub const ALL: [Shader; 2] = [Shader::Mesh, Shader::Blit];
}

/// Presentation sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VSync {
    /// Wait for the display refresh. No tearing.
    On,
    /// Present immediately. Lowest latency, may tear.
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

/// Renderer settings fixed at creation.
#[derive(Clone, Copy, Debug)]
pub struct RenderConfig {
    /// Low-res target width in pixels.
    pub internal_width: u32,
    /// Low-res target height in pixels.
    pub internal_height: u32,
    /// Upscale mode.
    pub scaling: Scaling,
    /// Presentation sync.
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
    /// Mesh to draw.
    pub mesh: MeshId,
    /// Object → world transform.
    pub model: glam::Mat4,
}

/// Everything needed to render one frame.
pub struct FrameView<'a> {
    /// Viewpoint.
    pub camera: &'a Camera,
    /// Draws in submission order.
    pub draws: &'a [Draw],
}

/// Anything with raw window handles that can be shared across threads.
pub trait WindowTarget:
    HasWindowHandle + HasDisplayHandle + Debug + Send + Sync + Clone + 'static
{
}

impl<T> WindowTarget for T where
    T: HasWindowHandle + HasDisplayHandle + Debug + Send + Sync + Clone + 'static
{
}

/// The renderer. One per window (or one headless).
pub struct Renderer {
    gpu: Gpu,
    /// None when headless.
    swapchain: Option<Swapchain>,
    config: RenderConfig,
    target: LowRes,
    meshes: MeshStore,
    mesh_pass: MeshPass,
    blit_pass: BlitPass,
}

impl Renderer {
    /// Renderer presenting to `window`, whose drawable size is `size`
    /// (physical pixels). `source` returns WGSL for each [`Shader`].
    pub fn new<E: Into<BoxError>>(
        window: impl WindowTarget,
        size: (u32, u32),
        config: RenderConfig,
        source: impl Fn(Shader) -> Result<String, E>,
    ) -> Result<Self> {
        let desc =
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone()));
        let instance = wgpu::Instance::new(desc);
        let surface = instance.create_surface(window)?;

        let gpu = Gpu::new(&instance, Some(&surface))?;
        let swapchain = Swapchain::new(surface, &gpu, size, config.vsync)?;

        Self::build(gpu, Some(swapchain), config, source)
    }

    /// Renderer without a window. `render` draws only the low-res target;
    /// read it back with `capture`. Fails when no GPU adapter exists.
    pub fn headless<E: Into<BoxError>>(
        config: RenderConfig,
        source: impl Fn(Shader) -> Result<String, E>,
    ) -> Result<Self> {
        let desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        let instance = wgpu::Instance::new(desc);
        let gpu = Gpu::new(&instance, None)?;

        Self::build(gpu, None, config, source)
    }

    fn build<E: Into<BoxError>>(
        gpu: Gpu,
        swapchain: Option<Swapchain>,
        config: RenderConfig,
        source: impl Fn(Shader) -> Result<String, E>,
    ) -> Result<Self> {
        let target = LowRes::new(&gpu.device, config.internal_width, config.internal_height);

        // Headless still builds the blit pass so its shader is validated.
        let output_format = swapchain.as_ref().map_or(COLOR_FORMAT, Swapchain::format);
        let load = |shader| {
            source(shader).map_err(|err| RenderError::ShaderSource {
                shader,
                source: err.into(),
            })
        };
        let mesh_pass = MeshPass::new(&gpu.device, &load(Shader::Mesh)?)?;
        let blit_pass = BlitPass::new(&gpu.device, output_format, &target, &load(Shader::Blit)?)?;

        Ok(Self {
            gpu,
            swapchain,
            config,
            target,
            meshes: MeshStore::default(),
            mesh_pass,
            blit_pass,
        })
    }

    /// Window drawable size changed (physical pixels). 0×0 is ignored.
    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(swapchain) = &mut self.swapchain {
            swapchain.resize(&self.gpu.device, width, height);
        }
    }

    /// Uploads a mesh. The handle stays valid until `destroy_mesh`.
    pub fn create_mesh(&mut self, data: &MeshData) -> MeshId {
        self.meshes.create(&self.gpu.device, data)
    }

    /// Frees a mesh's GPU buffers. False if the handle was already stale.
    pub fn destroy_mesh(&mut self, id: MeshId) -> bool {
        self.meshes.destroy(id)
    }

    /// Draws one frame and presents it. Skips silently when the window
    /// cannot present (minimized, occluded).
    pub fn render(&mut self, view: &FrameView) {
        let aspect = self.target.width as f32 / self.target.height as f32;
        let view_proj = view.camera.view_proj(aspect);
        self.mesh_pass
            .prepare(&self.gpu.device, &self.gpu.queue, view_proj, view.draws);

        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.mesh_pass
            .record(&mut encoder, &self.target, &self.meshes, view.draws);

        let Some(swapchain) = &mut self.swapchain else {
            self.gpu.queue.submit([encoder.finish()]);
            return;
        };
        let Some(frame) = swapchain.acquire(&self.gpu.device) else {
            self.gpu.queue.submit([encoder.finish()]);
            return;
        };

        let surface_view = frame.texture.create_view(&Default::default());
        self.blit_pass.record(
            &mut encoder,
            &surface_view,
            swapchain.size(),
            &self.target,
            self.config.scaling,
        );

        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(frame);
    }

    /// Last rendered low-res frame, as RGBA8 (sRGB). Blocks until the GPU
    /// is idle; meant for debug captures and tests, not per-frame use.
    pub fn capture(&self) -> Result<Capture> {
        capture::read(&self.gpu.device, &self.gpu.queue, &self.target)
    }

    /// Hot reload. On error the old version stays active, so a typo never
    /// kills the session.
    pub fn reload_shader(&mut self, shader: Shader, source: &str) -> Result<()> {
        match shader {
            Shader::Mesh => self.mesh_pass.reload(&self.gpu.device, source),
            Shader::Blit => self.blit_pass.reload(&self.gpu.device, source),
        }
    }
}
