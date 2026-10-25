//! Sandbox: fly around a tiled floor with textured crates and a spinning
//! colored cube, in fog, at 640×360 upscaled.
//!
//! Click to capture the mouse; Esc releases it, Esc again quits. F1
//! toggles fog, F2 cycles palette quantization. Edit assets/shaders/*.wgsl,
//! assets/textures/*.png or assets/palettes/*.png while running to see
//! them reload.

mod shapes;

use engine::math::{Mat4, Quat, Vec3};
use engine::{
    BoxError, Button, Config, CursorMode, Falloff, FlyCamera, Fog, Frame, Game, Gfx, Key,
    MeshHandle, MouseLook, Quantize, TextureHandle, Tick,
};
use shapes::Tint;

const FLOOR_SIZE: f32 = 40.0;
/// One floor texture repeat per 2 m: 16 px tiles read as 50 cm stones.
const FLOOR_TILES: f32 = FLOOR_SIZE / 2.0;

/// Start above and behind the scene, tilted down a little.
const START: Vec3 = Vec3::new(0.0, 2.0, 8.0);
const START_PITCH: f32 = -0.15;

/// Radians per second.
const SPIN_SPEED: f32 = 1.2;
const SPIN_AT: Vec3 = Vec3::new(0.0, 2.5, 0.0);
/// Tilted so three faces show at once. Normalized at use.
const SPIN_AXIS: Vec3 = Vec3::new(0.3, 1.0, 0.2);

/// Gray-blue mist: crates clear nearby, the floor's edge (20 m) gone.
const MIST: Fog = Fog {
    color: [0.10, 0.11, 0.13],
    falloff: Falloff::Exp2 { density: 0.08 },
};
const FOG_KEY: Key = Key::F1;
const QUANTIZE_KEY: Key = Key::F2;

/// Crate positions (bottom-center on the floor) and yaw, radians.
const CRATES: [(Vec3, f32); 5] = [
    (Vec3::new(-3.0, 0.0, -2.0), 0.0),
    (Vec3::new(-3.0, 1.0, -2.0), 0.4),
    (Vec3::new(-1.8, 0.0, -2.3), 0.1),
    (Vec3::new(3.0, 0.0, 1.0), 0.8),
    (Vec3::new(2.0, 0.0, -5.0), 0.3),
];

/// Resources created in `init`.
struct Assets {
    cube: MeshHandle,
    crate_mesh: MeshHandle,
    floor: MeshHandle,
    crate_texture: TextureHandle,
    floor_texture: TextureHandle,
}

struct Sandbox {
    assets: Option<Assets>,
    camera: FlyCamera,
    fog: Option<Fog>,
    quantize: Quantize,
    /// Spin angle at the previous and current tick, for interpolation.
    prev_angle: f32,
    angle: f32,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            assets: None,
            camera: FlyCamera::new(START, 0.0, START_PITCH),
            fog: Some(MIST),
            quantize: Quantize::Dithered,
            prev_angle: 0.0,
            angle: 0.0,
        }
    }

    /// Click captures the mouse; Esc frees it, or quits if already free.
    fn handle_cursor(tick: &mut Tick) {
        let captured = tick.cursor() == CursorMode::Captured;

        if !captured && tick.input.button_pressed(Button::Left) {
            tick.set_cursor(CursorMode::Captured);
        }

        if !tick.input.was_pressed(Key::Escape) {
            return;
        }
        if captured {
            tick.set_cursor(CursorMode::Free);
        } else {
            tick.quit();
        }
    }

    /// F1: fog on/off. F2: palette off → nearest → dithered → off.
    fn handle_look_keys(&mut self, tick: &Tick) {
        if tick.input.was_pressed(FOG_KEY) {
            self.fog = match self.fog {
                Some(_) => None,
                None => Some(MIST),
            };
        }

        if tick.input.was_pressed(QUANTIZE_KEY) {
            self.quantize = next_quantize(self.quantize);
        }
    }
}

fn next_quantize(quantize: Quantize) -> Quantize {
    match quantize {
        Quantize::Off => Quantize::Nearest,
        Quantize::Nearest => Quantize::Dithered,
        Quantize::Dithered => Quantize::Off,
    }
}

impl Game for Sandbox {
    fn init(&mut self, gfx: &mut Gfx) -> Result<(), BoxError> {
        self.assets = Some(Assets {
            cube: gfx.create_mesh(&shapes::cube(Tint::PerFace)),
            crate_mesh: gfx.create_mesh(&shapes::cube(Tint::White)),
            floor: gfx.create_mesh(&shapes::floor(FLOOR_SIZE, FLOOR_TILES)),
            crate_texture: gfx.load_texture("textures/crate.png")?,
            floor_texture: gfx.load_texture("textures/floor.png")?,
        });
        gfx.load_palette("palettes/default.png")?;
        Ok(())
    }

    fn tick(&mut self, tick: &mut Tick) {
        Self::handle_cursor(tick);
        self.handle_look_keys(tick);

        let look = match tick.cursor() {
            CursorMode::Captured => MouseLook::Enabled,
            CursorMode::Free => MouseLook::Disabled,
        };
        self.camera.update(tick.input, look, tick.dt);

        self.prev_angle = self.angle;
        self.angle += SPIN_SPEED * tick.dt;
    }

    fn draw(&mut self, frame: &mut Frame) {
        let Some(assets) = &self.assets else {
            return;
        };

        frame.camera = self.camera.camera(frame.alpha);
        frame.fog = self.fog;
        frame.quantize = self.quantize;
        frame.draw_textured(assets.floor, assets.floor_texture, Mat4::IDENTITY);

        for (base, yaw) in CRATES {
            // Cube is centered; lift by half its height to sit on `base`.
            let center = base + Vec3::Y * 0.5;
            let model = Mat4::from_rotation_translation(Quat::from_rotation_y(yaw), center);
            frame.draw_textured(assets.crate_mesh, assets.crate_texture, model);
        }

        let angle = self.prev_angle + (self.angle - self.prev_angle) * frame.alpha;
        let spin = Quat::from_axis_angle(SPIN_AXIS.normalize(), angle);
        frame.draw_mesh(assets.cube, Mat4::from_rotation_translation(spin, SPIN_AT));
    }
}

// anyhow at the top: the binary only reports errors, it never matches them.
fn main() -> anyhow::Result<()> {
    let config = Config {
        title: "Retro FPS — sandbox".into(),
        ..Default::default()
    };

    engine::run(config, Sandbox::new())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantize_cycle_visits_every_mode_and_wraps() {
        let mut mode = Quantize::Off;
        let mut seen = Vec::new();

        for _ in 0..3 {
            mode = next_quantize(mode);
            seen.push(mode);
        }

        assert_eq!(seen, [Quantize::Nearest, Quantize::Dithered, Quantize::Off]);
    }
}
