//! The sandbox test level, built from brushes: everything the walker
//! should handle, side by side. Seen from above (−Z up the page):
//!
//! ```text
//!                 ┌────■─────┐ z −16   ■ inside: scripted button,
//!       secret ═► ║   room   │            3 presses open the secret
//!       panel     │  ░░░░    │   ░ trigger: reopens the door from inside
//!                 └───╡══╞───┘ z −10
//!                     door  ■ button (use: E)
//!
//!   ┌────┐◢◢◢ ramp 25°          crates      ▤▤▤▤▤┌──────┐
//!   │plat│▲▲  wall 55°                       stairs│ plat │ 1.5 m
//!   └────┘ x −12 … −6                        x 6 … │      │
//!                          ● spawn (0, 8)          └──────┘
//! ```
//!
//! Geometry and entities live in plain functions without models, so
//! tests can check them without a GPU; `build` adds meshes and
//! textures. Scripted behavior is in `assets/scripts/sandbox.lua`.

use engine::math::{Quat, Vec3};
use engine::{
    Aabb, Brush, Close, Entity, Gfx, MeshData, Model, Mover, Plane, Repeat, TextureHandle, Trigger,
    Usable, World,
};

use crate::shapes::{self, Tint};

/// Name the door answers to; the button and trigger fire it.
pub const DOOR: &str = "door";
/// Name the secret panel answers to; the scripted button fires it.
pub const SECRET: &str = "secret";
/// Lua class (in assets/scripts/sandbox.lua) of the button inside.
pub const SECRET_BUTTON_CLASS: &str = "SecretButton";
/// Where the player starts: feet on the floor, facing the room (−Z).
pub const SPAWN: Vec3 = Vec3::new(0.0, 0.0, 8.0);

/// One stone texture repeat per 2 m: 16 px tiles read as 50 cm stones.
const STONE_METERS: f32 = 2.0;
/// One wood texture repeat per meter, matching a 1 m crate.
const WOOD_METERS: f32 = 1.0;
const BUTTON_RED: [f32; 4] = [0.8, 0.1, 0.08, 1.0];

const FLOOR_HALF: f32 = 20.0;
const PLATFORM_HEIGHT: f32 = 1.5;

/// Stairs: 5 steps of 30 cm (under the 45 cm step limit), 50 cm deep.
const STEP_COUNT: usize = 5;
const STEP_RISE: f32 = 0.3;
const STEP_DEPTH: f32 = 0.5;
const STAIRS_X: f32 = 6.0;

/// Ramps rise toward −X from here.
const RAMP_FOOT_X: f32 = -6.0;
/// Gentle enough to walk (floors are up to ~45°).
const WALKABLE_SLOPE: f32 = 25.0;
/// Too steep: slides you back down.
const STEEP_SLOPE: f32 = 55.0;

/// Room interior and walls.
const ROOM_MIN: Vec3 = Vec3::new(-4.0, 0.0, -16.0);
const ROOM_MAX: Vec3 = Vec3::new(4.0, 3.0, -10.0);
const WALL: f32 = 0.3;
const DOOR_HALF_WIDTH: f32 = 1.0;
const DOOR_HEIGHT: f32 = 2.2;
/// Door slides sideways into the wall, then closes 3 s after opening.
const DOOR_TRAVEL: Vec3 = Vec3::new(-2.0, 0.0, 0.0);
const DOOR_SPEED: f32 = 2.0;
const DOOR_WAIT: f32 = 3.0;
/// Opening in the west wall the secret panel fills, along Z.
const SECRET_Z: (f32, f32) = (-14.0, -12.0);
/// The panel sinks into the floor, slowly, and stays down.
const SECRET_SPEED: f32 = 1.0;

/// Crate positions (bottom-center) and yaw, radians.
const CRATES: [(Vec3, f32); 5] = [
    (Vec3::new(-3.0, 0.0, -2.0), 0.0),
    (Vec3::new(-3.0, 1.0, -2.0), 0.4),
    (Vec3::new(-1.8, 0.0, -2.3), 0.1),
    (Vec3::new(3.0, 0.0, 1.0), 0.8),
    (Vec3::new(2.0, 0.0, -5.0), 0.3),
];
const CRATE_HALF: Vec3 = Vec3::splat(0.5);

/// Textures the level is drawn with.
pub struct Textures {
    /// Floors, walls, stairs, ramps.
    pub stone: TextureHandle,
    /// Crates and the door.
    pub wood: TextureHandle,
}

/// Static solids: floor, stairs, platforms, ramps, room walls.
pub fn stone() -> Vec<Brush> {
    let mut solids = vec![Brush::cuboid(
        Vec3::new(-FLOOR_HALF, -1.0, -FLOOR_HALF),
        Vec3::new(FLOOR_HALF, 0.0, FLOOR_HALF),
    )];

    solids.extend(stairs());
    solids.extend(ramps());
    solids.extend(room_walls());
    solids
}

/// Steps rising toward +X, then a platform at the top.
fn stairs() -> Vec<Brush> {
    let mut steps: Vec<Brush> = (0..STEP_COUNT)
        .map(|i| {
            let x = STAIRS_X + STEP_DEPTH * i as f32;
            let top = STEP_RISE * (i + 1) as f32;
            Brush::cuboid(Vec3::new(x, 0.0, -1.0), Vec3::new(x + STEP_DEPTH, top, 1.0))
        })
        .collect();

    let landing = STAIRS_X + STEP_DEPTH * STEP_COUNT as f32;
    steps.push(Brush::cuboid(
        Vec3::new(landing, 0.0, -3.0),
        Vec3::new(landing + 3.5, PLATFORM_HEIGHT, 3.0),
    ));
    steps
}

/// A walkable ramp and a too-steep one, side by side, up to a platform.
fn ramps() -> Vec<Brush> {
    let gentle = ramp(WALKABLE_SLOPE, 2.0, 4.0);
    let steep = ramp(STEEP_SLOPE, 5.0, 7.0);
    let platform_end = RAMP_FOOT_X - run(WALKABLE_SLOPE);

    vec![
        gentle,
        steep,
        Brush::cuboid(
            Vec3::new(platform_end - 3.0, 0.0, 2.0),
            Vec3::new(platform_end, PLATFORM_HEIGHT, 7.0),
        ),
    ]
}

/// Horizontal length of a ramp up to platform height.
fn run(degrees: f32) -> f32 {
    PLATFORM_HEIGHT / degrees.to_radians().tan()
}

/// Wedge rising toward −X from `RAMP_FOOT_X` at `degrees`, between
/// `z_min` and `z_max`.
fn ramp(degrees: f32, z_min: f32, z_max: f32) -> Brush {
    let foot = Vec3::new(RAMP_FOOT_X, 0.0, 0.0);
    let back = RAMP_FOOT_X - run(degrees);
    let slope = Vec3::new(degrees.to_radians().tan(), 1.0, 0.0);

    Brush::from_planes(&[
        Plane::through(slope, foot),
        Plane::through(Vec3::NEG_Y, foot),
        Plane::through(Vec3::NEG_X, Vec3::X * back),
        Plane::through(Vec3::Z, Vec3::Z * z_max),
        Plane::through(Vec3::NEG_Z, Vec3::Z * z_min),
    ])
    .expect("ramp planes enclose a wedge")
}

/// Four walls, a roof, a doorway in the front (+Z) wall, and an opening
/// in the west (−X) wall for the secret panel.
fn room_walls() -> Vec<Brush> {
    let (lo, hi) = (ROOM_MIN, ROOM_MAX);
    let front = hi.z;
    let west = lo.x - WALL;
    let (secret_back, secret_front) = SECRET_Z;

    vec![
        // Back and right.
        Brush::cuboid(
            Vec3::new(west, 0.0, lo.z - WALL),
            Vec3::new(hi.x + WALL, hi.y, lo.z),
        ),
        Brush::cuboid(
            Vec3::new(hi.x, 0.0, lo.z),
            Vec3::new(hi.x + WALL, hi.y, front + WALL),
        ),
        // West, either side of the secret opening, and above it.
        Brush::cuboid(
            Vec3::new(west, 0.0, lo.z),
            Vec3::new(lo.x, hi.y, secret_back),
        ),
        Brush::cuboid(
            Vec3::new(west, 0.0, secret_front),
            Vec3::new(lo.x, hi.y, front + WALL),
        ),
        Brush::cuboid(
            Vec3::new(west, DOOR_HEIGHT, secret_back),
            Vec3::new(lo.x, hi.y, secret_front),
        ),
        // Front, either side of the doorway, and above it.
        Brush::cuboid(
            Vec3::new(lo.x, 0.0, front),
            Vec3::new(-DOOR_HALF_WIDTH, hi.y, front + WALL),
        ),
        Brush::cuboid(
            Vec3::new(DOOR_HALF_WIDTH, 0.0, front),
            Vec3::new(hi.x, hi.y, front + WALL),
        ),
        Brush::cuboid(
            Vec3::new(-DOOR_HALF_WIDTH, DOOR_HEIGHT, front),
            Vec3::new(DOOR_HALF_WIDTH, hi.y, front + WALL),
        ),
        // Roof.
        Brush::cuboid(
            Vec3::new(west, hi.y, lo.z - WALL),
            Vec3::new(hi.x + WALL, hi.y + WALL, front + WALL),
        ),
    ]
}

/// Sliding door filling the doorway, slimmer than the wall it hides in.
fn door() -> Entity {
    let z = ROOM_MAX.z + WALL / 2.0;
    let brush = Brush::cuboid(
        Vec3::new(-DOOR_HALF_WIDTH, 0.0, z - 0.1),
        Vec3::new(DOOR_HALF_WIDTH, DOOR_HEIGHT, z + 0.1),
    );

    Entity {
        name: Some(DOOR.into()),
        colliders: vec![brush],
        mover: Some(Mover {
            travel: DOOR_TRAVEL,
            speed: DOOR_SPEED,
            close: Close::After(DOOR_WAIT),
        }),
        ..Default::default()
    }
}

/// Button on the front wall's outside, right of the door: opens it.
fn door_button() -> Entity {
    let face = ROOM_MAX.z + WALL;
    let brush = Brush::cuboid(
        Vec3::new(DOOR_HALF_WIDTH + 0.5, 1.1, face),
        Vec3::new(DOOR_HALF_WIDTH + 0.8, 1.4, face + 0.1),
    );

    Entity {
        colliders: vec![brush],
        usable: Some(Usable {
            target: Some(DOOR.into()),
        }),
        ..Default::default()
    }
}

/// Inside the room, just behind the door: reopens it on the way out.
fn door_trigger() -> Entity {
    let front = ROOM_MAX.z;
    let volume = Aabb {
        min: Vec3::new(-DOOR_HALF_WIDTH, 0.0, front - 1.5),
        max: Vec3::new(DOOR_HALF_WIDTH, DOOR_HEIGHT, front),
    };

    Entity {
        trigger: Some(Trigger {
            volume,
            target: Some(DOOR.into()),
            repeat: Repeat::EveryEntry,
        }),
        ..Default::default()
    }
}

/// Fills the west-wall opening; sinks into the floor when fired.
fn secret_panel() -> Entity {
    let (back, front) = SECRET_Z;
    let brush = Brush::cuboid(
        Vec3::new(ROOM_MIN.x - WALL + 0.05, 0.0, back),
        Vec3::new(ROOM_MIN.x - 0.05, DOOR_HEIGHT, front),
    );

    Entity {
        name: Some(SECRET.into()),
        colliders: vec![brush],
        mover: Some(Mover {
            travel: Vec3::NEG_Y * DOOR_HEIGHT,
            speed: SECRET_SPEED,
            close: Close::OnFire,
        }),
        ..Default::default()
    }
}

/// Button on the back wall's inside, run by Lua: counts presses and
/// fires the secret panel (see assets/scripts/sandbox.lua).
fn secret_button() -> Entity {
    let back = ROOM_MIN.z;
    let brush = Brush::cuboid(
        Vec3::new(-0.15, 1.1, back),
        Vec3::new(0.15, 1.4, back + 0.1),
    );

    Entity {
        colliders: vec![brush],
        script: Some(SECRET_BUTTON_CLASS.into()),
        ..Default::default()
    }
}

/// Every level entity, ready to draw and collide, spawned into `world`.
pub fn build(gfx: &mut Gfx, world: &mut World, textures: &Textures) {
    let stone_texture = Some(textures.stone);
    let wood_texture = Some(textures.wood);

    for brush in stone() {
        world.spawn(with_mesh(gfx, solid(brush), STONE_METERS, stone_texture));
    }

    let crate_mesh = gfx.create_mesh(&shapes::cube(Tint::White));
    for (base, yaw) in CRATES {
        let rotation = Quat::from_rotation_y(yaw);
        world.spawn(Entity {
            position: base + Vec3::Y * CRATE_HALF.y,
            rotation,
            colliders: Brush::oriented_box(Vec3::ZERO, CRATE_HALF, rotation)
                .into_iter()
                .collect(),
            models: vec![Model {
                mesh: crate_mesh,
                texture: wood_texture,
                lightmap: None,
            }],
            ..Default::default()
        });
    }

    world.spawn(with_mesh(gfx, door(), WOOD_METERS, wood_texture));
    world.spawn(with_mesh(gfx, secret_panel(), STONE_METERS, stone_texture));
    world.spawn(red_button(gfx, door_button()));
    world.spawn(red_button(gfx, secret_button()));
    world.spawn(door_trigger());
}

/// Entity solid as `brush`, in world space.
fn solid(brush: Brush) -> Entity {
    Entity {
        colliders: vec![brush],
        ..Default::default()
    }
}

/// Adds one model per collider, drawn from the brush itself.
fn with_mesh(
    gfx: &mut Gfx,
    entity: Entity,
    meters_per_texture: f32,
    texture: Option<TextureHandle>,
) -> Entity {
    let models = entity
        .colliders
        .iter()
        .map(|brush| Model {
            mesh: gfx.create_mesh(&brush.mesh(meters_per_texture)),
            texture,
            lightmap: None,
        })
        .collect();

    Entity { models, ..entity }
}

/// Adds plain red models drawn from the entity's colliders.
fn red_button(gfx: &mut Gfx, entity: Entity) -> Entity {
    let models = entity
        .colliders
        .iter()
        .map(|brush| Model {
            mesh: gfx.create_mesh(&tinted(brush.mesh(WOOD_METERS), BUTTON_RED)),
            texture: None,
            lightmap: None,
        })
        .collect();

    Entity { models, ..entity }
}

fn tinted(mut mesh: MeshData, color: [f32; 4]) -> MeshData {
    for v in &mut mesh.vertices {
        v.color = color;
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A person-sized box, as the walker uses.
    const HALF: Vec3 = Vec3::new(0.3, 0.9, 0.3);

    /// Level without models: geometry, door, buttons, panel, scripts.
    fn collision_only() -> World {
        let mut world = World::new();
        let script = std::fs::read_to_string(SCRIPT_FILE).unwrap();
        world.load_script(SCRIPT_FILE, &script).unwrap();

        for brush in stone() {
            world.add_brush(brush);
        }
        for entity in [
            door(),
            door_button(),
            door_trigger(),
            secret_panel(),
            secret_button(),
        ] {
            world.spawn(entity);
        }
        world
    }

    const SCRIPT_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/scripts/sandbox.lua");
    const DT: f32 = 1.0 / 60.0;

    /// Ticks `seconds`; `player` is the body triggers and scripts see.
    fn run(world: &mut World, seconds: f32, player: Option<Aabb>) {
        for _ in 0..(seconds / DT).round() as u32 {
            world.tick(DT, player);
        }
    }

    /// A player body standing at `(x, z)`.
    fn player_at(x: f32, z: f32) -> Option<Aabb> {
        Some(Aabb::from_center(Vec3::new(x, HALF.y + 0.01, z), HALF))
    }

    /// Eye in front of the secret button, looking at it.
    const BUTTON_EYE: Vec3 = Vec3::new(0.0, 1.25, ROOM_MIN.z + 1.0);

    /// True when a box can pass through the secret opening.
    fn secret_open(world: &World) -> bool {
        let z = (SECRET_Z.0 + SECRET_Z.1) / 2.0;
        let inside = Vec3::new(ROOM_MIN.x + 1.0, 1.0, z);
        let outside = Vec3::new(ROOM_MIN.x - 2.0, 1.0, z);
        !world.trace(inside, outside, HALF * 0.5).hit()
    }

    /// Normal of whatever is straight below `(x, z)`.
    fn floor_normal(world: &World, x: f32, z: f32) -> Vec3 {
        let trace = world.trace(Vec3::new(x, 10.0, z), Vec3::new(x, -1.0, z), Vec3::ZERO);
        trace.normal.unwrap()
    }

    #[test]
    fn spawn_is_clear_and_on_the_floor() {
        let world = collision_only();
        let standing = Aabb::from_center(SPAWN + Vec3::Y * (HALF.y + 0.01), HALF);

        assert!(world.is_clear(standing));
        assert_eq!(floor_normal(&world, SPAWN.x, SPAWN.z), Vec3::Y);
    }

    #[test]
    fn ramps_have_their_slopes() {
        let world = collision_only();
        let x = RAMP_FOOT_X - 0.5;

        let gentle = floor_normal(&world, x, 3.0);
        assert!((gentle.y - WALKABLE_SLOPE.to_radians().cos()).abs() < 1e-3);

        let steep = floor_normal(&world, x, 6.0);
        assert!((steep.y - STEEP_SLOPE.to_radians().cos()).abs() < 1e-3);
    }

    #[test]
    fn stairs_climb_one_rise_at_a_time() {
        let world = collision_only();

        for i in 0..STEP_COUNT {
            let x = STAIRS_X + STEP_DEPTH * (i as f32 + 0.5);
            let trace = world.trace(Vec3::new(x, 10.0, 0.0), Vec3::new(x, -1.0, 0.0), Vec3::ZERO);
            let height = trace.end.y;
            assert!(
                (height - STEP_RISE * (i + 1) as f32).abs() < 0.01,
                "step {i}: {height}"
            );
        }
    }

    #[test]
    fn closed_door_seals_the_doorway() {
        let world = collision_only();
        let outside = Vec3::new(0.0, 1.0, ROOM_MAX.z + 2.0);
        let inside = Vec3::new(0.0, 1.0, ROOM_MAX.z - 2.0);

        assert!(world.trace(outside, inside, HALF * 0.5).hit());
        // The trigger sits inside the room, behind the door.
        let trigger = door_trigger().trigger.unwrap();
        assert!(trigger.volume.max.z <= ROOM_MAX.z);
    }

    #[test]
    fn secret_button_opens_the_panel_on_the_third_press() {
        let mut world = collision_only();

        for _ in 0..2 {
            assert!(world.use_from(BUTTON_EYE, Vec3::NEG_Z));
            run(&mut world, 3.0, None);
        }
        assert!(!secret_open(&world), "opened early");

        assert!(world.use_from(BUTTON_EYE, Vec3::NEG_Z));
        run(&mut world, 3.0, None);
        assert!(secret_open(&world));
    }

    #[test]
    fn secret_panel_shuts_behind_a_wandering_player_and_reopens() {
        let mut world = collision_only();
        for _ in 0..3 {
            world.use_from(BUTTON_EYE, Vec3::NEG_Z);
            run(&mut world, 3.0, None);
        }

        // Standing in the room by the panel: it stays open.
        let near = player_at(ROOM_MIN.x + 1.0, SECRET_Z.0 + 1.0);
        run(&mut world, 1.0, near);
        assert!(secret_open(&world), "closed on a nearby player");

        // Back at the spawn, 20 m away: it shuts.
        run(&mut world, 3.0, player_at(SPAWN.x, SPAWN.z));
        assert!(!secret_open(&world), "still open behind the player");

        // Found once, one press reopens it.
        assert!(world.use_from(BUTTON_EYE, Vec3::NEG_Z));
        run(&mut world, 3.0, None);
        assert!(secret_open(&world));
    }

    #[test]
    fn open_door_hides_inside_the_wall() {
        let mover = door().mover.unwrap();
        let open = door().colliders[0].bounds().min + mover.travel;
        // Fully open, the door's near edge clears the doorway.
        assert!(open.x + 2.0 * DOOR_HALF_WIDTH <= -DOOR_HALF_WIDTH + 1e-4);
        assert!(open.x >= ROOM_MIN.x);
    }
}
