//! Engine core: owns the main loop and defines the API the game layer
//! is written against. Every public type here is the engine's own;
//! platform and render types stay behind this crate.
//!
//!   game ──► engine ──┬──► render ───► wgpu
//!                     └──► platform ─► winit
//!   (base: foundation utilities, usable by all)

mod app;
mod assets;
mod debug;
mod error;
mod fly;
mod gfx;
mod input;
mod sim;

pub use base::{BoxError, Button, Image, Key};
pub use error::EngineError;
pub use fly::{FlyCamera, FlySettings, MouseLook};
pub use gfx::{
    Camera, Falloff, Fog, Frame, Gfx, MeshData, MeshHandle, Mipmaps, Quantize, Scaling,
    TextureHandle, VSync, Vertex, Video,
};
pub use glam as math;
pub use input::{CursorMode, Input};
pub use sim::Tick;

/// Engine startup settings.
pub struct Config {
    /// Window title.
    pub title: String,
    /// Window width in logical pixels.
    pub width: u32,
    /// Window height in logical pixels.
    pub height: u32,
    /// Simulation rate. Rendering runs as fast as vsync allows.
    pub tick_hz: u32,
    /// Resolution and presentation.
    pub video: Video,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            title: "engine".into(),
            width: 1280,
            height: 720,
            tick_hz: 60,
            video: Video::default(),
        }
    }
}

/// The game layer. `tick` advances simulation by a fixed `dt`; `draw`
/// describes the scene, interpolating with `frame.alpha`.
pub trait Game {
    /// Called once, after the window and GPU are up. Load resources here.
    /// Any error type converts into `BoxError` with `?` or `.into()`.
    fn init(&mut self, gfx: &mut Gfx) -> Result<(), BoxError>;
    /// One fixed simulation step. Read input, update state.
    fn tick(&mut self, tick: &mut Tick);
    /// Describe the frame: set the camera, queue draws. No simulation.
    fn draw(&mut self, frame: &mut Frame);
}

/// Runs the game until it quits or the window closes.
pub fn run(config: Config, game: impl Game) -> Result<(), EngineError> {
    base::log::init();
    app::run(config, game)
}
