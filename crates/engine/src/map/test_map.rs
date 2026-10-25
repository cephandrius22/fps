//! The committed test map (maps/test.map) loaded on a headless GPU:
//! collision, doors, a walker on its stairs, and a reference image.
//! Behavior is checked on both compiles, qbsp's .bsp and mapc's .lvl;
//! the image only on the .bsp, the lit one.

use base::Key;
use platform::{Event, KeyState};

use glam::Vec3;

use super::{MapInfo, UNITS_PER_METER};
use crate::assets::Registry;
use crate::collision::Aabb;
use crate::gfx::Frame;
use crate::gfx::Gfx;
use crate::input::Input;
use crate::look::MouseLook;
use crate::look::{Look, LookSettings};
use crate::test_util::{assert_matches_reference, headless_with, repo_assets};
use crate::walk::{WalkSettings, Walker};
use crate::world::World;

/// The test map compiled by ericw-tools (lit) and by mapc (unlit).
const BSP: &str = "maps/test.bsp";
const LEVEL: &str = "maps/test.lvl";
const REFERENCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/reference/test-map.png");
/// Reference render size: small, so few pixels sit on triangle edges.
const REFERENCE_SIZE: (u32, u32) = (320, 180);
const DT: f32 = 1.0 / 60.0;
const EYE: f32 = 1.6;
const HALF: Vec3 = Vec3::new(0.3, 0.9, 0.3);

/// Runs `check` on a world holding each compile of the test map; skips
/// without a GPU.
fn with_map(check: impl Fn(&mut World, &MapInfo)) {
    for path in [BSP, LEVEL] {
        // Names the compile when a check fails.
        eprintln!("checking {path}");
        with_map_rendering(path, |world, info, _| check(world, info));
    }
}

/// As `with_map` for the map at `path`, also handing over the (320×180)
/// renderer.
fn with_map_rendering(path: &str, check: impl FnOnce(&mut World, &MapInfo, &mut render::Renderer)) {
    let vfs = repo_assets();
    let config = render::RenderConfig {
        internal_width: REFERENCE_SIZE.0,
        internal_height: REFERENCE_SIZE.1,
        ..Default::default()
    };
    let Some(mut renderer) = headless_with(&vfs, config) else {
        return;
    };
    let mut registry = Registry::with_shaders();
    let mut gfx = Gfx {
        renderer: &mut renderer,
        vfs: &vfs,
        registry: &mut registry,
    };
    let mut world = World::new();

    let info = gfx.load_map(path, &mut world).unwrap();
    check(&mut world, &info, &mut renderer);
}

fn run(world: &mut World, seconds: f32, player: Option<Aabb>) {
    for _ in 0..(seconds / DT).round() as u32 {
        world.tick(DT, player);
    }
}

/// How far (meters) a ray goes from `from` along −Z before hitting.
fn reach_north(world: &World, from: Vec3) -> f32 {
    let trace = world.trace(from, from + Vec3::NEG_Z * 30.0, Vec3::ZERO);
    from.z - trace.end.z
}

/// The view from the player start, lit by the baked lightmaps, without
/// fog or palette (those would amplify GPU differences), against the
/// committed reference image. Catches regressions anywhere from map
/// parsing to shading.
#[test]
fn view_from_the_start_matches_the_reference() {
    with_map_rendering(BSP, |world, info, renderer| {
        let start = info.start.unwrap();
        let eye = start.feet + Vec3::Y * EYE;

        let mut frame = Frame::new();
        frame.begin(0.0);
        frame.camera = Look::new(start.yaw, 0.0).camera(eye, &LookSettings::default());
        world.draw(&mut frame);
        frame.submit(renderer);

        let shot = renderer.capture().unwrap();
        assert_matches_reference(&shot, std::path::Path::new(REFERENCE));
    });
}

#[test]
fn start_and_title_come_from_the_map() {
    with_map(|_, info| {
        let start = info.start.unwrap();
        assert!((start.feet - Vec3::new(0.0, 0.0, 6.0)).length() < 1e-4);
        assert!(start.yaw.abs() < 1e-4, "faces north (−Z)");
        assert_eq!(info.message.as_deref(), Some("Engine test map"));
    });
}

#[test]
fn floor_under_the_start_is_solid_at_zero() {
    with_map(|world, info| {
        let feet = info.start.unwrap().feet;
        let trace = world.trace(feet + Vec3::Y, feet - Vec3::Y, Vec3::ZERO);

        assert!(trace.hit() && trace.end.y.abs() < 0.01, "{trace:?}");
        assert_eq!(trace.normal, Some(Vec3::Y));
    });
}

#[test]
fn button_opens_the_targeted_door() {
    with_map(|world, _| {
        let door = world.find("door1").unwrap();
        let eye = Vec3::new(0.0, EYE, 0.0);
        // Door 1 sits 8 m north (Quake y 258).
        assert!(reach_north(world, eye) < 8.1, "door 1 starts closed");

        // The button: Quake (96, 252, 64) → engine (3, 2, −7.9).
        assert!(world.use_from(Vec3::new(3.0, 2.0, -6.5), Vec3::NEG_Z));
        run(world, 2.0, None);

        assert!(world.entity(door).unwrap().position.y > 3.0, "slid up");
        assert!(reach_north(world, eye) > 19.0, "through room B to door 2");
    });
}

#[test]
fn untargeted_door_opens_on_approach() {
    with_map(|world, _| {
        let door = world.find("*door5").unwrap();
        // Quake y 608: inside door 2's field, short of the door.
        let player = Aabb::from_center(Vec3::new(0.0, 0.9, -19.0), HALF);

        run(world, 1.0, Some(player));
        assert!(world.entity(door).unwrap().position.x > 1.0, "slid east");
    });
}

#[test]
fn walker_climbs_the_stairs_to_the_ledge() {
    with_map(|world, _| {
        // Facing east (+X) toward 37.5 cm steps up to a 1.125 m ledge.
        let east = -std::f32::consts::FRAC_PI_2;
        let mut walker = Walker::new(Vec3::new(2.0, 0.0, 0.0), east, 0.0, WalkSettings::arena());
        let mut input = Input::default();
        input.apply(Event::Key {
            key: Key::W,
            state: KeyState::Pressed,
        });

        for _ in 0..120 {
            walker.update(&input, MouseLook::Disabled, world, DT);
            input.end_tick();
        }

        let feet = walker.feet();
        assert!(
            (feet.y - 36.0 / UNITS_PER_METER).abs() < 0.02,
            "on the ledge: {feet:?}"
        );
        assert!(feet.x < 8.0, "stopped at the east wall: {feet:?}");
    });
}
