//! Renderer. Draws the scene into a low-resolution target, applies the
//! retro post-pass there, then upscales to the window with
//! nearest-neighbor sampling.
//!
//! ```text
//!   draws ─► MeshPass (lit, fogged) ─► low-res scene color+depth (640×360)
//!                                             │
//!                                             ▼
//!                   PostPass (palette, dither) ─► low-res output ─► capture
//!                                             │
//!                                             ▼
//!                   BlitPass (nearest, letterboxed) ─► window surface
//! ```
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
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod texture;

use std::fmt::Debug;

use base::{BoxError, Image};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

pub use camera::Camera;
pub use error::RenderError;
pub use mesh::{MeshData, MeshId, Vertex};
pub use texture::TextureId;

use error::Result;
use gpu::Gpu;
use mesh::MeshStore;
use passes::{BlitPass, MeshPass, PostPass};
use swapchain::Swapchain;
use target::{COLOR_FORMAT, LowRes};
use texture::{TextureKind, TextureStore};

/// Shader programs the renderer needs. The caller decides where their
/// source lives (see [`Renderer::new`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shader {
    /// Opaque scene geometry.
    Mesh,
    /// Low-res retro post-pass: palette quantization and dither.
    Post,
    /// Low-res → window upscale.
    Blit,
}

impl Shader {
    /// Every shader, for loading all of them up front.
    pub const ALL: [Shader; 3] = [Shader::Mesh, Shader::Post, Shader::Blit];
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

/// Texture mip chains. Without them, textures far away or at grazing
/// angles shimmer as the camera moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mipmaps {
    /// Base level only: maximum shimmer.
    Off,
    /// Chain with hard switches between levels, like Quake's software
    /// renderer. Texels stay sharp.
    Nearest,
    /// Chain with blends between levels: no visible switch lines, but
    /// distant surfaces turn softer.
    Linear,
}

/// How fog thickens with distance from the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Falloff {
    /// Clear until `start`, solid fog from `end` (meters).
    Linear {
        /// Distance where fog begins.
        start: f32,
        /// Distance where fog is solid. Must exceed `start`.
        end: f32,
    },
    /// Visibility e^(−density·d): thickens from the camera outward.
    Exp {
        /// Per meter. 0.05 halves visibility every ~14 m.
        density: f32,
    },
    /// Visibility e^(−(density·d)²): clear nearby, then closes in fast.
    Exp2 {
        /// Per meter.
        density: f32,
    },
}

/// Distance fog. Surfaces blend toward `color` with distance, and empty
/// background is drawn in `color`, so far geometry fades out seamlessly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    /// Linear RGB.
    pub color: [f32; 3],
    /// Thickening curve.
    pub falloff: Falloff,
}

/// Palette quantization of the final low-res image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantize {
    /// Full color.
    Off,
    /// Each pixel becomes its nearest palette color. Gradients band.
    Nearest,
    /// Ordered (Bayer 4×4) dither first, so bands become a fixed
    /// pixel pattern, as on 8-bit displays.
    Dithered,
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
    /// Texture mip chains.
    pub mipmaps: Mipmaps,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            internal_width: 640,
            internal_height: 360,
            scaling: Scaling::Fit,
            vsync: VSync::On,
            mipmaps: Mipmaps::Nearest,
        }
    }
}

/// One mesh instance to draw this frame.
#[derive(Clone, Copy, Debug)]
pub struct Draw {
    /// Mesh to draw.
    pub mesh: MeshId,
    /// Texture, multiplied with vertex color. None draws untextured.
    pub texture: Option<TextureId>,
    /// Baked light from [`Renderer::create_lightmap`], read at each
    /// vertex's `lightmap_uv`. None lights with the fixed sun term.
    pub lightmap: Option<TextureId>,
    /// Object → world transform.
    pub model: glam::Mat4,
}

/// Everything needed to render one frame.
pub struct FrameView<'a> {
    /// Viewpoint.
    pub camera: &'a Camera,
    /// Draws in submission order.
    pub draws: &'a [Draw],
    /// Distance fog; None for clear air.
    pub fog: Option<Fog>,
    /// Palette quantization. Needs a palette from
    /// [`Renderer::set_palette`]; until then it has no effect.
    pub quantize: Quantize,
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
    textures: TextureStore,
    mesh_pass: MeshPass,
    post_pass: PostPass,
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
        let textures = TextureStore::new(&gpu.device, &gpu.queue, config.mipmaps);
        let mesh_pass = MeshPass::new(&gpu.device, textures.layout(), &load(Shader::Mesh)?)?;
        let post_pass = PostPass::new(&gpu.device, &target, &load(Shader::Post)?)?;
        let blit_pass = BlitPass::new(&gpu.device, output_format, &target, &load(Shader::Blit)?)?;

        Ok(Self {
            gpu,
            swapchain,
            config,
            target,
            meshes: MeshStore::default(),
            textures,
            mesh_pass,
            post_pass,
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

    /// Uploads an sRGB RGBA8 image. Sampled nearest-neighbor, repeating.
    pub fn create_texture(&mut self, image: &Image) -> Result<TextureId> {
        self.textures.create(
            &self.gpu.device,
            &self.gpu.queue,
            image,
            TextureKind::Albedo,
        )
    }

    /// Uploads a lightmap: sRGB-encoded brightness, Quake-style overbright
    /// (128 = full texture brightness, 255 about twice). Alpha must be
    /// 255. Sampled bilinear, clamped, without mips.
    pub fn create_lightmap(&mut self, image: &Image) -> Result<TextureId> {
        self.textures.create(
            &self.gpu.device,
            &self.gpu.queue,
            image,
            TextureKind::Lightmap,
        )
    }

    /// Replaces a texture's pixels (any size) in place; the handle and
    /// every draw using it stay valid. Used by hot reload.
    pub fn replace_texture(&mut self, id: TextureId, image: &Image) -> Result<()> {
        self.textures
            .replace(&self.gpu.device, &self.gpu.queue, id, image)
    }

    /// Frees a texture. False if the handle was already stale.
    pub fn destroy_texture(&mut self, id: TextureId) -> bool {
        self.textures.destroy(id)
    }

    /// Sets the palette [`Quantize`] snaps to: every pixel of `colors`,
    /// row-major, alpha ignored. A 16×16 image holds the 256-color
    /// maximum. Replaces any previous palette.
    pub fn set_palette(&mut self, colors: &Image) -> Result<()> {
        self.post_pass.set_palette(colors)
    }

    /// Draws one frame and presents it. Skips silently when the window
    /// cannot present (minimized, occluded).
    pub fn render(&mut self, view: &FrameView) {
        let aspect = self.target.width as f32 / self.target.height as f32;
        self.mesh_pass.prepare(
            &self.gpu.device,
            &self.gpu.queue,
            view.camera,
            aspect,
            view.fog,
            view.draws,
        );
        self.post_pass.prepare(&self.gpu.queue, view.quantize);

        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.mesh_pass.record(
            &mut encoder,
            &self.target,
            &self.meshes,
            &self.textures,
            view.draws,
        );
        self.post_pass.record(&mut encoder, &self.target);

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

    /// Last rendered low-res frame, after the post-pass, as RGBA8 (sRGB).
    /// Blocks until the GPU is idle; meant for debug captures and tests,
    /// not per-frame use.
    pub fn capture(&self) -> Result<Image> {
        capture::read(&self.gpu.device, &self.gpu.queue, &self.target)
    }

    /// Hot reload. On error the old version stays active, so a typo never
    /// kills the session.
    pub fn reload_shader(&mut self, shader: Shader, source: &str) -> Result<()> {
        match shader {
            Shader::Mesh => self.mesh_pass.reload(&self.gpu.device, source),
            Shader::Post => self.post_pass.reload(&self.gpu.device, source),
            Shader::Blit => self.blit_pass.reload(&self.gpu.device, source),
        }
    }
}
