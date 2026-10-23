//! Narrow views of engine state handed to the game in each callback.

use glam::{Mat4, Vec3};
use platform::Input;
use render::{Camera, Draw, MeshData, MeshId, Renderer};

const DEFAULT_FOV_DEG: f32 = 70.0;

/// Resource creation, available during `Game::init`.
pub struct Gfx<'a> {
    pub(crate) renderer: &'a mut Renderer,
}

impl Gfx<'_> {
    pub fn create_mesh(&mut self, data: &MeshData) -> MeshId {
        self.renderer.create_mesh(data)
    }
}

/// One simulation step.
pub struct Tick<'a> {
    pub input: &'a Input,
    /// Seconds per tick (fixed).
    pub dt: f32,
    pub(crate) quit: bool,
}

impl Tick<'_> {
    pub fn quit(&mut self) {
        self.quit = true;
    }
}

/// Scene description for one rendered frame.
pub struct Frame {
    pub camera: Camera,
    /// Progress from the previous tick to the current one, 0..1.
    pub alpha: f32,
    pub(crate) draws: Vec<Draw>,
}

impl Frame {
    pub(crate) fn new() -> Self {
        Self {
            camera: Camera::look_at(
                Vec3::new(0.0, 0.0, 5.0),
                Vec3::ZERO,
                DEFAULT_FOV_DEG.to_radians(),
            ),
            alpha: 0.0,
            draws: Vec::new(),
        }
    }

    pub(crate) fn begin(&mut self, alpha: f32) {
        self.alpha = alpha;
        self.draws.clear();
    }

    pub fn draw_mesh(&mut self, mesh: MeshId, model: Mat4) {
        self.draws.push(Draw { mesh, model });
    }
}
