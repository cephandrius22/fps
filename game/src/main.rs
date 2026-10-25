//! Sandbox: walk (or fly) around a brush-built test level with stairs,
//! ramps, crates and a room behind a sliding door, in fog, at 640×360
//! upscaled.
//!
//! Bindings owned here (walking comes from `Walker`, flying from
//! `FlyCamera`, F12 screenshots from the engine; README.md lists them
//! all):
//!
//! ```text
//! left click   mouse free: capture it for mouse look
//! Esc          mouse captured: free it; mouse free: quit
//! E            use what is in front of you, within 2 m (the button)
//! F1           fog on / off (starts on)
//! F2           palette: dithered (start) → off → nearest → dithered
//! F3           walk ↔ fly; back to walking only where the body fits
//! F4           movement profile: survival (start) ↔ arena
//! ```
//!
//! Edit assets/shaders/*.wgsl, assets/textures/*.png or
//! assets/palettes/*.png while running to see them reload.

mod level;
mod shapes;

use engine::math::{Mat4, Quat, Vec3};
use engine::{
    Aabb, BoxError, Button, Camera, Config, CursorMode, Falloff, FlyCamera, Fog, Frame, Game, Gfx,
    Key, MeshHandle, MouseLook, Quantize, Tick, WalkSettings, Walker, World,
};
use shapes::Tint;

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
const CAPTURE_BUTTON: Button = Button::Left;
const RELEASE_KEY: Key = Key::Escape;
const USE_KEY: Key = Key::E;
const FOG_KEY: Key = Key::F1;
const QUANTIZE_KEY: Key = Key::F2;
const MOVEMENT_KEY: Key = Key::F3;
const PROFILE_KEY: Key = Key::F4;

/// Ready-made walking feel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    Survival,
    Arena,
}

impl Profile {
    fn settings(self) -> WalkSettings {
        match self {
            Profile::Survival => WalkSettings::survival(),
            Profile::Arena => WalkSettings::arena(),
        }
    }

    fn other(self) -> Profile {
        match self {
            Profile::Survival => Profile::Arena,
            Profile::Arena => Profile::Survival,
        }
    }
}

/// How the player gets around.
enum Movement {
    /// With collision and gravity.
    Walk(Walker),
    /// Noclip: through walls, no gravity, sets off no triggers.
    Fly(FlyCamera),
}

impl Movement {
    fn eye(&self) -> Vec3 {
        match self {
            Movement::Walk(walker) => walker.eye(),
            Movement::Fly(fly) => fly.position(),
        }
    }

    fn forward(&self) -> Vec3 {
        match self {
            Movement::Walk(walker) => walker.forward(),
            Movement::Fly(fly) => fly.forward(),
        }
    }

    fn camera(&self, alpha: f32) -> Camera {
        match self {
            Movement::Walk(walker) => walker.camera(alpha),
            Movement::Fly(fly) => fly.camera(alpha),
        }
    }
}

/// The player: how they move, and the profile walking uses.
struct Player {
    movement: Movement,
    profile: Profile,
}

impl Player {
    fn new() -> Self {
        let profile = Profile::Survival;
        let walker = Walker::new(level::SPAWN, 0.0, 0.0, profile.settings());

        Self {
            movement: Movement::Walk(walker),
            profile,
        }
    }

    /// F3: walk ↔ fly. F4: survival ↔ arena (applies when walking, and
    /// to the next walk after flying).
    fn handle_keys(&mut self, tick: &Tick, world: &World) {
        if tick.input.was_pressed(PROFILE_KEY) {
            self.profile = self.profile.other();
            if let Movement::Walk(walker) = &mut self.movement {
                walker.settings = self.profile.settings();
            }
        }

        if !tick.input.was_pressed(MOVEMENT_KEY) {
            return;
        }
        self.movement = match &self.movement {
            Movement::Walk(walker) => {
                Movement::Fly(FlyCamera::new(walker.eye(), walker.yaw(), walker.pitch()))
            }
            Movement::Fly(fly) => match land(fly, self.profile.settings(), world) {
                Some(walker) => Movement::Walk(walker),
                None => return,
            },
        };
    }

    /// Moves one tick. Returns the body to test triggers against: None
    /// while flying.
    fn update(&mut self, tick: &Tick, world: &World) -> Option<Aabb> {
        let mouse = match tick.cursor() {
            CursorMode::Captured => MouseLook::Enabled,
            CursorMode::Free => MouseLook::Disabled,
        };

        match &mut self.movement {
            Movement::Walk(walker) => {
                walker.update(tick.input, mouse, world, tick.dt);
                Some(walker.bounds())
            }
            Movement::Fly(fly) => {
                fly.update(tick.input, mouse, tick.dt);
                None
            }
        }
    }
}

/// A walker whose eyes are where the flying camera is, if its body fits
/// there.
fn land(fly: &FlyCamera, settings: WalkSettings, world: &World) -> Option<Walker> {
    let feet = fly.position() - Vec3::Y * settings.eye_height;
    let walker = Walker::new(feet, fly.yaw(), fly.pitch(), settings);

    world.is_clear(walker.bounds()).then_some(walker)
}

/// Look settings the player toggles: fog and palette.
struct View {
    fog: Option<Fog>,
    quantize: Quantize,
}

impl View {
    fn new() -> Self {
        Self {
            fog: Some(MIST),
            quantize: Quantize::Dithered,
        }
    }

    /// F1: fog on/off. F2: palette off → nearest → dithered → off.
    /// Both act once per press, on the tick it lands.
    fn handle_keys(&mut self, tick: &Tick) {
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

/// The decorative spinning cube's angle at the previous and current
/// tick, for interpolation.
#[derive(Default)]
struct Spin {
    prev: f32,
    angle: f32,
}

impl Spin {
    fn advance(&mut self, dt: f32) {
        self.prev = self.angle;
        self.angle += SPIN_SPEED * dt;
    }

    fn transform(&self, alpha: f32) -> Mat4 {
        let angle = self.prev + (self.angle - self.prev) * alpha;
        let rotation = Quat::from_axis_angle(SPIN_AXIS.normalize(), angle);
        Mat4::from_rotation_translation(rotation, SPIN_AT)
    }
}

/// Resources created in `init`.
struct Scene {
    world: World,
    cube: MeshHandle,
}

struct Sandbox {
    scene: Option<Scene>,
    player: Player,
    view: View,
    spin: Spin,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            scene: None,
            player: Player::new(),
            view: View::new(),
            spin: Spin::default(),
        }
    }

    /// Click captures the mouse; Esc frees it, or quits if already free.
    fn handle_cursor(tick: &mut Tick) {
        let captured = tick.cursor() == CursorMode::Captured;

        if !captured && tick.input.button_pressed(CAPTURE_BUTTON) {
            tick.set_cursor(CursorMode::Captured);
        }

        if !tick.input.was_pressed(RELEASE_KEY) {
            return;
        }
        if captured {
            tick.set_cursor(CursorMode::Free);
        } else {
            tick.quit();
        }
    }
}

impl Game for Sandbox {
    fn init(&mut self, gfx: &mut Gfx) -> Result<(), BoxError> {
        let textures = level::Textures {
            stone: gfx.load_texture("textures/floor.png")?,
            wood: gfx.load_texture("textures/crate.png")?,
        };
        gfx.load_palette("palettes/default.png")?;

        self.scene = Some(Scene {
            world: level::build(gfx, &textures),
            cube: gfx.create_mesh(&shapes::cube(Tint::PerFace)),
        });
        Ok(())
    }

    fn tick(&mut self, tick: &mut Tick) {
        let Some(scene) = &mut self.scene else {
            return;
        };

        Self::handle_cursor(tick);
        self.view.handle_keys(tick);
        self.player.handle_keys(tick, &scene.world);

        // Player first, then the world reacts to where they ended up.
        let body = self.player.update(tick, &scene.world);
        scene.world.tick(tick.dt, body);

        if tick.input.was_pressed(USE_KEY) {
            let movement = &self.player.movement;
            scene.world.use_from(movement.eye(), movement.forward());
        }

        self.spin.advance(tick.dt);
    }

    fn draw(&mut self, frame: &mut Frame) {
        let Some(scene) = &self.scene else {
            return;
        };

        frame.camera = self.player.movement.camera(frame.alpha);
        frame.fog = self.view.fog;
        frame.quantize = self.view.quantize;

        scene.world.draw(frame, frame.alpha);
        frame.draw_mesh(scene.cube, self.spin.transform(frame.alpha));
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
    use engine::Brush;

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

    #[test]
    fn profiles_alternate_and_differ() {
        assert_eq!(Profile::Survival.other(), Profile::Arena);
        assert_eq!(Profile::Arena.other(), Profile::Survival);
        assert_ne!(Profile::Survival.settings(), Profile::Arena.settings());
    }

    #[test]
    fn landing_needs_room_for_the_body() {
        let mut world = World::new();
        world.add_brush(Brush::cuboid(
            Vec3::new(-1.0, 0.0, -1.0),
            Vec3::new(1.0, 3.0, 1.0),
        ));
        let settings = WalkSettings::survival();

        let inside = FlyCamera::new(Vec3::new(0.0, 1.6, 0.0), 0.0, 0.0);
        assert!(land(&inside, settings, &world).is_none());

        let open = FlyCamera::new(Vec3::new(5.0, 1.6, 0.0), 0.3, -0.2);
        let walker = land(&open, settings, &world).unwrap();
        assert!((walker.eye() - open.position()).length() < 0.05);
        assert_eq!((walker.yaw(), walker.pitch()), (0.3, -0.2));
    }
}
