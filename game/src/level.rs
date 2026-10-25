//! The sandbox test level, built from brushes: everything the walker
//! should handle, side by side. Seen from above (−Z up the page):
//!
//! ```text
//!                 ┌──────────┐ z −16
//!                 │   room   │
//!                 │  ░░░░    │   ░ trigger: reopens the door from inside
//!                 └───╡══╞───┘ z −10
//!                     door  ■ button (use: E)
//!
//!   ┌────┐◢◢◢ ramp 25°          crates      ▤▤▤▤▤┌──────┐
//!   │plat│▲▲  wall 55°                       stairs│ plat │ 1.5 m
//!   └────┘ x −12 … −6                        x 6 … │      │
//!                          ● spawn (0, 8)          └──────┘
//! ```
//!
//! Geometry lives in plain functions returning brushes, so tests can
//! check it without a GPU; `build` adds meshes and textures.

use engine::math::{Quat, Vec3};
use engine::{
    Aabb, Brush, Close, Entity, Gfx, MeshData, MeshHandle, Model, Mover, Plane, Repeat,
    TextureHandle, Trigger, Usable, World,
};

use crate::shapes::{self, Tint};

/// Name the door answers to; the button and trigger fire it.
pub const DOOR: &str = "door";
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

/// Four walls, a roof, and a doorway in the front (+Z) wall.
fn room_walls() -> Vec<Brush> {
    let (lo, hi) = (ROOM_MIN, ROOM_MAX);
    let front = hi.z;

    vec![
        // Back, left, right.
        Brush::cuboid(
            Vec3::new(lo.x - WALL, 0.0, lo.z - WALL),
            Vec3::new(hi.x + WALL, hi.y, lo.z),
        ),
        Brush::cuboid(
            Vec3::new(lo.x - WALL, 0.0, lo.z),
            Vec3::new(lo.x, hi.y, front + WALL),
        ),
        Brush::cuboid(
            Vec3::new(hi.x, 0.0, lo.z),
            Vec3::new(hi.x + WALL, hi.y, front + WALL),
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
            Vec3::new(lo.x - WALL, hi.y, lo.z - WALL),
            Vec3::new(hi.x + WALL, hi.y + WALL, front + WALL),
        ),
    ]
}

/// Sliding door filling the doorway, slimmer than the wall it hides in.
fn door_brush() -> Brush {
    let z = ROOM_MAX.z + WALL / 2.0;
    Brush::cuboid(
        Vec3::new(-DOOR_HALF_WIDTH, 0.0, z - 0.1),
        Vec3::new(DOOR_HALF_WIDTH, DOOR_HEIGHT, z + 0.1),
    )
}

/// Small block on the front wall's outside, right of the door.
fn button_brush() -> Brush {
    let face = ROOM_MAX.z + WALL;
    Brush::cuboid(
        Vec3::new(DOOR_HALF_WIDTH + 0.5, 1.1, face),
        Vec3::new(DOOR_HALF_WIDTH + 0.8, 1.4, face + 0.1),
    )
}

/// Inside the room, just behind the door.
fn trigger_volume() -> Aabb {
    let front = ROOM_MAX.z;
    Aabb {
        min: Vec3::new(-DOOR_HALF_WIDTH, 0.0, front - 1.5),
        max: Vec3::new(DOOR_HALF_WIDTH, DOOR_HEIGHT, front),
    }
}

/// Every level entity, ready to draw and collide.
pub fn build(gfx: &mut Gfx, textures: &Textures) -> World {
    let mut world = World::new();
    let crate_mesh = gfx.create_mesh(&shapes::cube(Tint::White));

    for brush in stone() {
        let mesh = gfx.create_mesh(&brush.mesh(STONE_METERS));
        world.spawn(solid(brush, mesh, Some(textures.stone)));
    }

    for (base, yaw) in CRATES {
        let rotation = Quat::from_rotation_y(yaw);
        let collider = Brush::oriented_box(Vec3::ZERO, CRATE_HALF, rotation);
        world.spawn(Entity {
            position: base + Vec3::Y * CRATE_HALF.y,
            rotation,
            collider,
            ..solid_model(crate_mesh, Some(textures.wood))
        });
    }

    let door = door_brush();
    world.spawn(Entity {
        name: Some(DOOR.into()),
        mover: Some(Mover {
            travel: DOOR_TRAVEL,
            speed: DOOR_SPEED,
            close: Close::After(DOOR_WAIT),
        }),
        ..solid(
            door.clone(),
            gfx.create_mesh(&door.mesh(WOOD_METERS)),
            Some(textures.wood),
        )
    });

    let button = button_brush();
    let red = tinted(button.mesh(WOOD_METERS), BUTTON_RED);
    world.spawn(Entity {
        usable: Some(Usable {
            target: DOOR.into(),
        }),
        ..solid(button, gfx.create_mesh(&red), None)
    });

    world.spawn(Entity {
        trigger: Some(Trigger {
            volume: trigger_volume(),
            target: DOOR.into(),
            repeat: Repeat::EveryEntry,
        }),
        ..Default::default()
    });

    world
}

/// Entity drawn with `mesh` and solid as `brush`, both in world space.
fn solid(brush: Brush, mesh: MeshHandle, texture: Option<TextureHandle>) -> Entity {
    Entity {
        collider: Some(brush),
        ..solid_model(mesh, texture)
    }
}

fn solid_model(mesh: MeshHandle, texture: Option<TextureHandle>) -> Entity {
    Entity {
        model: Some(Model { mesh, texture }),
        ..Default::default()
    }
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

    fn collision_only() -> World {
        let mut world = World::new();
        for brush in stone() {
            world.add_brush(brush);
        }
        world.add_brush(door_brush());
        world
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
        assert!(trigger_volume().max.z <= ROOM_MAX.z);
    }

    #[test]
    fn open_door_hides_inside_the_wall() {
        let open = door_brush().bounds().min + DOOR_TRAVEL;
        // Fully open, the door's near edge clears the doorway.
        assert!(open.x + 2.0 * DOOR_HALF_WIDTH <= -DOOR_HALF_WIDTH + 1e-4);
        assert!(open.x >= ROOM_MIN.x);
    }
}
