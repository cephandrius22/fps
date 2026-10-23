//! Engine core: owns the main loop and exposes the API the game layer
//! is written against. The game never touches platform or GPU types.
//!
//!   game ──► engine ──► render ──► wgpu
//!               └─────► platform ──► winit

mod app;
mod context;

use anyhow::Result;

pub use context::{Frame, Gfx, Tick};
pub use glam as math;
pub use platform::{Button, Input, Key};
pub use render::{Camera, MeshData, MeshId, RenderConfig, Scaling, VSync, Vertex};

pub struct Config {
    pub title: String,
    /// Window size in logical pixels.
    pub width: u32,
    pub height: u32,
    /// Simulation rate. Rendering runs as fast as vsync allows.
    pub tick_hz: u32,
    pub render: RenderConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            title: "engine".into(),
            width: 1280,
            height: 720,
            tick_hz: 60,
            render: RenderConfig::default(),
        }
    }
}

/// The game layer. `tick` advances simulation by a fixed `dt`; `draw`
/// describes the scene, interpolating with `frame.alpha`.
pub trait Game {
    fn init(&mut self, gfx: &mut Gfx) -> Result<()>;
    fn tick(&mut self, tick: &mut Tick);
    fn draw(&mut self, frame: &mut Frame);
}

/// Runs the game until it quits or the window closes.
pub fn run(config: Config, game: impl Game) -> Result<()> {
    base::log::init();
    app::run(config, game)
}
