//! Game-facing graphics types and their translation to the renderer.
//! This module is the only place engine and render types meet.

use base::{BoxError, Image, Vfs};
use glam::camera::rh::view;
use glam::{Mat4, Vec3};
use render::{Draw, Renderer};

use crate::assets::{self, Asset, Registry};
use crate::error::EngineError;

/// Near clip plane, meters. Small enough for a held weapon model.
pub(crate) const DEFAULT_NEAR: f32 = 0.05;
/// Far clip plane, meters.
pub(crate) const DEFAULT_FAR: f32 = 1000.0;
const DEFAULT_FOV_DEG: f32 = 70.0;
const DEFAULT_EYE: Vec3 = Vec3::new(0.0, 0.0, 5.0);

/// How the low-res image fills the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scaling {
    /// Largest whole-number multiple that fits: crisp, uniform pixels.
    Integer,
    /// Fill as much as aspect allows: no wasted border, uneven pixels.
    Fit,
}

/// Presentation sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VSync {
    /// Wait for the display refresh. No tearing.
    On,
    /// Present immediately. Lowest latency, may tear.
    Off,
}

/// Resolution and presentation settings.
#[derive(Clone, Copy, Debug)]
pub struct Video {
    /// Internal render width; the retro look comes from keeping it low.
    pub internal_width: u32,
    /// Internal render height.
    pub internal_height: u32,
    /// Upscale mode.
    pub scaling: Scaling,
    /// Presentation sync.
    pub vsync: VSync,
}

impl Default for Video {
    fn default() -> Self {
        Self {
            internal_width: 640,
            internal_height: 360,
            scaling: Scaling::Fit,
            vsync: VSync::On,
        }
    }
}

impl Video {
    pub(crate) fn to_render(self) -> render::RenderConfig {
        render::RenderConfig {
            internal_width: self.internal_width,
            internal_height: self.internal_height,
            scaling: match self.scaling {
                Scaling::Integer => render::Scaling::Integer,
                Scaling::Fit => render::Scaling::Fit,
            },
            vsync: match self.vsync {
                VSync::On => render::VSync::On,
                VSync::Off => render::VSync::Off,
            },
        }
    }
}

/// Mesh vertex, object space. Right-handed, Y up; triangles wind
/// counter-clockwise seen from their front (back faces are culled).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
    fn to_render(self) -> render::Vertex {
        render::Vertex {
            pos: self.pos,
            normal: self.normal,
            uv: self.uv,
            lightmap_uv: self.lightmap_uv,
            color: self.color,
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

impl MeshData {
    fn to_render(&self) -> render::MeshData {
        render::MeshData {
            vertices: self.vertices.iter().map(|v| v.to_render()).collect(),
            indices: self.indices.clone(),
        }
    }
}

/// Handle to a loaded mesh. Copy freely; valid until destroyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshHandle(render::MeshId);

/// Handle to a loaded texture. Copy freely; valid until destroyed.
/// Hot reload swaps the pixels behind it without changing the handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureHandle(render::TextureId);

/// Viewpoint and lens. World is right-handed, Y up.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// World → view transform (camera looks down its -Z).
    pub view: Mat4,
    /// Vertical field of view, radians.
    pub fov_y: f32,
    /// Near clip distance.
    pub near: f32,
    /// Far clip distance.
    pub far: f32,
}

impl Camera {
    /// Camera at `eye` facing `target`, world Y up.
    pub fn look_at(eye: Vec3, target: Vec3, fov_y: f32) -> Self {
        Self {
            view: view::look_at_mat4(eye, target, Vec3::Y),
            fov_y,
            near: DEFAULT_NEAR,
            far: DEFAULT_FAR,
        }
    }

    fn to_render(self) -> render::Camera {
        render::Camera {
            view: self.view,
            fov_y: self.fov_y,
            near: self.near,
            far: self.far,
        }
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::look_at(DEFAULT_EYE, Vec3::ZERO, DEFAULT_FOV_DEG.to_radians())
    }
}

/// Resource creation, available during `Game::init`.
pub struct Gfx<'a> {
    pub(crate) renderer: &'a mut Renderer,
    pub(crate) vfs: &'a Vfs,
    pub(crate) registry: &'a mut Registry,
}

impl Gfx<'_> {
    /// Loads a PNG from the asset directory, e.g. "textures/crate.png",
    /// and watches it: saving the file updates the texture live.
    pub fn load_texture(&mut self, path: &str) -> Result<TextureHandle, EngineError> {
        let load_err = |source: BoxError| EngineError::Load {
            path: path.to_owned(),
            source,
        };

        let image = assets::load_image(self.vfs, path).map_err(load_err)?;
        let id = self
            .renderer
            .create_texture(&image)
            .map_err(|err| load_err(err.into()))?;
        self.registry.register(path, Asset::Texture(id));

        Ok(TextureHandle(id))
    }

    /// Uploads an in-memory image, e.g. a generated pattern. Not watched.
    pub fn create_texture(&mut self, image: &Image) -> Result<TextureHandle, EngineError> {
        Ok(TextureHandle(self.renderer.create_texture(image)?))
    }

    /// Frees a texture and stops watching its file. Destroying twice is a
    /// bug (flagged by `verify!`).
    pub fn destroy_texture(&mut self, texture: TextureHandle) {
        let destroyed = self.renderer.destroy_texture(texture.0);
        base::verify!(destroyed, "texture {texture:?} destroyed twice");

        self.registry.forget(Asset::Texture(texture.0));
    }

    /// Uploads a mesh to the GPU.
    pub fn create_mesh(&mut self, data: &MeshData) -> MeshHandle {
        MeshHandle(self.renderer.create_mesh(&data.to_render()))
    }

    /// Frees a mesh. Destroying twice is a bug (flagged by `verify!`).
    pub fn destroy_mesh(&mut self, mesh: MeshHandle) {
        let destroyed = self.renderer.destroy_mesh(mesh.0);
        base::verify!(destroyed, "mesh {mesh:?} destroyed twice");
    }
}

/// Scene description for one rendered frame.
pub struct Frame {
    /// Viewpoint for this frame.
    pub camera: Camera,
    /// Progress from the previous tick to the current one, 0..1.
    pub alpha: f32,
    draws: Vec<Draw>,
}

impl Frame {
    pub(crate) fn new() -> Self {
        Self {
            camera: Camera::default(),
            alpha: 0.0,
            draws: Vec::new(),
        }
    }

    /// Starts a frame: sets `alpha`, clears last frame's draws.
    pub(crate) fn begin(&mut self, alpha: f32) {
        self.alpha = alpha;
        self.draws.clear();
    }

    /// Queues `mesh` with object → world transform `model`.
    pub fn draw_mesh(&mut self, mesh: MeshHandle, model: Mat4) {
        self.draws.push(Draw {
            mesh: mesh.0,
            texture: None,
            model,
        });
    }

    /// Queues `mesh` with `texture` (multiplied by vertex color).
    pub fn draw_textured(&mut self, mesh: MeshHandle, texture: TextureHandle, model: Mat4) {
        self.draws.push(Draw {
            mesh: mesh.0,
            texture: Some(texture.0),
            model,
        });
    }

    /// Hands the frame to the renderer.
    pub(crate) fn submit(&self, renderer: &mut Renderer) {
        renderer.render(&render::FrameView {
            camera: &self.camera.to_render(),
            draws: &self.draws,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_converts_field_for_field() {
        let v = Vertex {
            pos: [1.0, 2.0, 3.0],
            normal: [0.0, 1.0, 0.0],
            uv: [0.25, 0.5],
            lightmap_uv: [0.75, 1.0],
            color: [0.1, 0.2, 0.3, 0.4],
        };
        let r = v.to_render();

        assert_eq!(r.pos, v.pos);
        assert_eq!(r.normal, v.normal);
        assert_eq!(r.uv, v.uv);
        assert_eq!(r.lightmap_uv, v.lightmap_uv);
        assert_eq!(r.color, v.color);
    }

    #[test]
    fn mesh_data_keeps_counts_and_indices() {
        let data = MeshData {
            vertices: vec![Vertex::default(); 3],
            indices: vec![0, 1, 2],
        };
        let r = data.to_render();

        assert_eq!(r.vertices.len(), 3);
        assert_eq!(r.indices, data.indices);
    }

    #[test]
    fn video_maps_every_enum_variant() {
        let video = Video {
            scaling: Scaling::Integer,
            vsync: VSync::Off,
            ..Default::default()
        };
        let r = video.to_render();

        assert_eq!(r.scaling, render::Scaling::Integer);
        assert_eq!(r.vsync, render::VSync::Off);
        assert_eq!((r.internal_width, r.internal_height), (640, 360));
        assert_eq!(Video::default().to_render().scaling, render::Scaling::Fit);
        assert_eq!(Video::default().to_render().vsync, render::VSync::On);
    }

    #[test]
    fn camera_look_at_puts_target_in_front() {
        let cam = Camera::look_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, 1.0);
        let target_in_view = cam.view.transform_point3(Vec3::ZERO);

        // View space looks down -Z: the target is 5 m ahead.
        assert!((target_in_view.z + 5.0).abs() < 1e-5);
        assert_eq!(cam.to_render().near, DEFAULT_NEAR);
    }

    #[test]
    fn frame_begin_sets_alpha() {
        // Clearing queued draws needs a real MeshId, i.e. a GPU; not
        // covered by unit tests.
        let mut frame = Frame::new();
        frame.begin(0.25);

        assert!(frame.draws.is_empty());
        assert_eq!(frame.alpha, 0.25);
    }

    fn repo_assets() -> Vfs {
        Vfs::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")).unwrap()
    }

    fn headless(vfs: &Vfs) -> Option<Renderer> {
        let source = |shader| vfs.read_text(crate::assets::shader_path(shader));
        render::test_support::headless_or_skip(Default::default(), source)
    }

    #[test]
    fn load_texture_watches_path_until_destroyed() {
        let vfs = repo_assets();
        let Some(mut renderer) = headless(&vfs) else {
            return;
        };
        let mut registry = Registry::with_shaders();
        let mut gfx = Gfx {
            renderer: &mut renderer,
            vfs: &vfs,
            registry: &mut registry,
        };

        let texture = gfx.load_texture("textures/crate.png").unwrap();
        assert_eq!(
            gfx.registry.lookup("textures/crate.png"),
            Some(Asset::Texture(texture.0))
        );

        gfx.destroy_texture(texture);
        assert_eq!(gfx.registry.lookup("textures/crate.png"), None);
    }

    #[test]
    fn load_texture_error_names_the_path() {
        let vfs = repo_assets();
        let Some(mut renderer) = headless(&vfs) else {
            return;
        };
        let mut registry = Registry::with_shaders();
        let mut gfx = Gfx {
            renderer: &mut renderer,
            vfs: &vfs,
            registry: &mut registry,
        };

        let err = gfx.load_texture("textures/nope.png").unwrap_err();
        assert!(matches!(&err, EngineError::Load { path, .. } if path == "textures/nope.png"));
        assert_eq!(gfx.registry.lookup("textures/nope.png"), None);
    }
}
