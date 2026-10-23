//! Phase 0 sandbox: a spinning colored cube at 640×360, upscaled.
//! Esc quits. Edit assets/shaders/*.wgsl while running to hot reload.

mod cube;

use anyhow::Result;
use engine::math::{Mat4, Quat, Vec3};
use engine::{Camera, Config, Frame, Game, Gfx, Key, MeshId, Tick};

/// Radians per second.
const SPIN_SPEED: f32 = 1.2;
const FOV_DEG: f32 = 70.0;

#[derive(Default)]
struct Sandbox {
    cube: Option<MeshId>,
    /// Spin angle at the previous and current tick, for interpolation.
    prev_angle: f32,
    angle: f32,
}

impl Game for Sandbox {
    fn init(&mut self, gfx: &mut Gfx) -> Result<()> {
        self.cube = Some(gfx.create_mesh(&cube::mesh()));
        Ok(())
    }

    fn tick(&mut self, tick: &mut Tick) {
        if tick.input.was_pressed(Key::Escape) {
            tick.quit();
        }

        self.prev_angle = self.angle;
        self.angle += SPIN_SPEED * tick.dt;
    }

    fn draw(&mut self, frame: &mut Frame) {
        let Some(cube) = self.cube else {
            return;
        };

        let eye = Vec3::new(0.0, 1.2, 3.0);
        frame.camera = Camera::look_at(eye, Vec3::ZERO, FOV_DEG.to_radians());

        // Tilted axis so three faces show at once.
        let angle = self.prev_angle + (self.angle - self.prev_angle) * frame.alpha;
        let axis = Vec3::new(0.3, 1.0, 0.2).normalize();
        frame.draw_mesh(cube, Mat4::from_quat(Quat::from_axis_angle(axis, angle)));
    }
}

fn main() -> Result<()> {
    let config = Config {
        title: "Retro FPS — sandbox".into(),
        ..Default::default()
    };

    engine::run(config, Sandbox::default())
}
