//! Moving a box through the world without passing into anything: slide
//! along what it hits, climb small steps, find the floor. The pieces a
//! character controller is built from (Quake's `PM_SlideMove` and
//! `PM_StepSlideMove`).
//!
//! ```text
//!   slide:  ──►│        wall: the push into it is removed,
//!              │↑       the rest continues along it
//!
//!   step:      ┌──      blocked low? retry the move from `step` higher,
//!   ──►   ─────┘        then drop back down onto the step
//! ```

use glam::{Vec2, Vec3};

use crate::world::{BodyId, Trace, World};
use crate::{DIST_EPSILON, MIN_WALK_NORMAL};

/// Hits handled per move; a box wedged into a corner touches at most
/// three faces, so four is plenty.
const MAX_BUMPS: usize = 4;
/// Faces remembered per move; more means hopelessly wedged.
const MAX_PLANES: usize = 5;
/// Velocity is clipped a hair past parallel, so the box drifts away from
/// the surface instead of grazing back into it.
const OVERCLIP: f32 = 1.001;
/// How far below the box to look for the floor.
const GROUND_PROBE: f32 = 0.01;
/// Speeds below this (m/s) count as stopped.
const MIN_SPEED: f32 = 1e-4;

/// Box position and velocity after a move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Move {
    /// Center of the box.
    pub position: Vec3,
    /// Velocity left after collisions, m/s.
    pub velocity: Vec3,
}

/// The floor under a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ground {
    /// Floor normal; its Y is at least [`MIN_WALK_NORMAL`].
    pub normal: Vec3,
    /// The body stood on.
    pub body: Option<BodyId>,
}

/// Moves the box for `dt` seconds, sliding along whatever it hits.
pub fn slide(world: &World, position: Vec3, velocity: Vec3, half: Vec3, dt: f32) -> Move {
    let original = velocity;
    let mut position = position;
    let mut velocity = velocity;
    let mut time_left = dt;
    let mut planes: Vec<Vec3> = Vec::with_capacity(MAX_PLANES);

    for _ in 0..MAX_BUMPS {
        if velocity.length() < MIN_SPEED {
            break;
        }

        let trace = world.trace(position, position + velocity * time_left, half);
        // Wedged inside something: stay put rather than tunnel out.
        if trace.all_solid {
            return Move {
                position,
                velocity: Vec3::ZERO,
            };
        }

        position = trace.end;
        let Some(normal) = trace.normal else {
            break;
        };
        if planes.len() == MAX_PLANES {
            velocity = Vec3::ZERO;
            break;
        }

        time_left -= time_left * trace.fraction;
        planes.push(normal);
        velocity = along_planes(velocity, &planes);

        // Turned back on itself: stop instead of jittering in a corner.
        if velocity.dot(original) <= 0.0 {
            velocity = Vec3::ZERO;
            break;
        }
    }

    Move { position, velocity }
}

/// As [`slide`], but a move blocked by something lower than `step` is
/// retried from `step` higher and set back down: stairs climb without a
/// jump. Use only for a box standing on the ground.
pub fn step_slide(
    world: &World,
    position: Vec3,
    velocity: Vec3,
    half: Vec3,
    dt: f32,
    step: f32,
) -> Move {
    let plain = slide(world, position, velocity, half, dt);

    // Moved the whole way: nothing to step over.
    let unblocked = position + velocity * dt;
    if plain.position.distance(unblocked) < DIST_EPSILON {
        return plain;
    }

    // Up, the same move, then back down.
    let up = world.trace(position, position + Vec3::Y * step, half);
    if up.all_solid {
        return plain;
    }
    let raised = slide(world, up.end, velocity, half, dt);
    let down = world.trace(raised.position, raised.position - Vec3::Y * step, half);

    let on_floor = down.normal.is_some_and(walkable);
    if down.all_solid || !on_floor {
        return plain;
    }

    // Keep the step only if it got further across the ground.
    let across = |p: Vec3| Vec2::new(p.x - position.x, p.z - position.z).length();
    if across(down.end) <= across(plain.position) {
        return plain;
    }

    Move {
        position: down.end,
        velocity: Vec3::new(raised.velocity.x, plain.velocity.y, raised.velocity.z),
    }
}

/// The walkable floor directly under the box, if any.
pub fn ground(world: &World, position: Vec3, half: Vec3) -> Option<Ground> {
    let trace = world.trace(position, position - Vec3::Y * GROUND_PROBE, half);
    floor_hit(&trace)
}

/// Drops the box up to `max_drop` onto walkable floor below; None if
/// there is none that close. Keeps a walker on stairs and slopes going
/// down instead of launching off each edge.
pub fn snap_down(world: &World, position: Vec3, half: Vec3, max_drop: f32) -> Option<Vec3> {
    let trace = world.trace(position, position - Vec3::Y * max_drop, half);
    floor_hit(&trace).map(|_| trace.end)
}

fn floor_hit(trace: &Trace) -> Option<Ground> {
    if trace.start_solid {
        return None;
    }

    let normal = trace.normal.filter(|&n| walkable(n))?;
    Some(Ground {
        normal,
        body: trace.body,
    })
}

fn walkable(normal: Vec3) -> bool {
    normal.y >= MIN_WALK_NORMAL
}

/// `velocity` minus its push into `normal` (a little extra, see OVERCLIP).
fn clip(velocity: Vec3, normal: Vec3) -> Vec3 {
    velocity - normal * velocity.dot(normal) * OVERCLIP
}

/// A velocity that moves along every plane in `planes` without pushing
/// into any: clipped against one plane if that clears the rest, else
/// along the crease of two, else none (a corner).
fn along_planes(velocity: Vec3, planes: &[Vec3]) -> Vec3 {
    for (i, &plane) in planes.iter().enumerate() {
        let clipped = clip(velocity, plane);
        let clears_others = planes
            .iter()
            .enumerate()
            .all(|(j, &other)| j == i || clipped.dot(other) >= 0.0);

        if clears_others {
            return clipped;
        }
    }

    let [a, b] = planes else {
        return Vec3::ZERO;
    };
    let crease = a.cross(*b).normalize_or_zero();
    crease * crease.dot(velocity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Brush, Plane};

    const DT: f32 = 0.1;
    const STEP: f32 = 0.45;
    /// A 0.6 × 1.8 m person-sized box.
    const HALF: Vec3 = Vec3::new(0.3, 0.9, 0.3);

    /// Floor top at y = 0, 40 m square.
    fn floor() -> Brush {
        Brush::cuboid(Vec3::new(-20.0, -1.0, -20.0), Vec3::new(20.0, 0.0, 20.0))
    }

    fn world(brushes: impl IntoIterator<Item = Brush>) -> World {
        let mut world = World::default();
        for brush in brushes {
            world.add(brush);
        }
        world
    }

    /// Box center when standing on y = `height`.
    fn standing(x: f32, height: f32) -> Vec3 {
        Vec3::new(x, height + HALF.y + DIST_EPSILON, 0.0)
    }

    /// Block from x = 1 to 3, `height` tall.
    fn block(height: f32) -> Brush {
        Brush::cuboid(Vec3::new(1.0, 0.0, -2.0), Vec3::new(3.0, height, 2.0))
    }

    #[test]
    fn falling_box_lands_on_floor() {
        let world = world([floor()]);
        let moved = slide(&world, standing(0.0, 2.0), Vec3::NEG_Y * 50.0, HALF, DT);

        assert!((moved.position.y - standing(0.0, 0.0).y).abs() < 0.01);
        assert_eq!(moved.velocity.y, 0.0);
    }

    #[test]
    fn wall_keeps_the_sideways_part_of_the_motion() {
        let world = world([
            floor(),
            Brush::cuboid(Vec3::new(1.0, 0.0, -20.0), Vec3::new(2.0, 3.0, 20.0)),
        ]);
        let moved = slide(
            &world,
            standing(0.0, 0.0),
            Vec3::new(10.0, 0.0, 10.0),
            HALF,
            DT,
        );

        assert!(moved.position.x < 1.0 - HALF.x + 0.01);
        assert!((moved.position.z - 1.0).abs() < 0.05, "{moved:?}");
        assert!(moved.velocity.x.abs() < 0.1);
        assert!((moved.velocity.z - 10.0).abs() < 0.1);
    }

    #[test]
    fn corner_stops_the_box() {
        let walls = [
            Brush::cuboid(Vec3::new(1.0, 0.0, -20.0), Vec3::new(2.0, 3.0, 20.0)),
            Brush::cuboid(Vec3::new(-20.0, 0.0, -2.0), Vec3::new(20.0, 3.0, -1.0)),
        ];
        let world = world(walls.into_iter().chain([floor()]));
        let moved = slide(
            &world,
            standing(0.0, 0.0),
            Vec3::new(10.0, 0.0, -10.0),
            HALF,
            DT,
        );

        assert!(moved.velocity.length() < 0.1, "{moved:?}");
    }

    #[test]
    fn low_step_is_climbed_and_high_one_blocks() {
        let start = standing(0.0, 0.0);
        let push = Vec3::X * 20.0;

        let stairs = world([floor(), block(0.3)]);
        let climbed = step_slide(&stairs, start, push, HALF, DT, STEP);
        assert!(
            (climbed.position.y - standing(0.0, 0.3).y).abs() < 0.01,
            "{climbed:?}"
        );
        assert!(climbed.position.x > 1.0);

        let ledge = world([floor(), block(0.6)]);
        let blocked = step_slide(&ledge, start, push, HALF, DT, STEP);
        assert!((blocked.position.y - start.y).abs() < 0.01);
        assert!(blocked.position.x < 1.0 - HALF.x + 0.01);
    }

    #[test]
    fn step_slide_on_open_floor_is_a_plain_slide() {
        let world = world([floor()]);
        let start = standing(0.0, 0.0);
        let push = Vec3::X * 3.0;

        assert_eq!(
            step_slide(&world, start, push, HALF, DT, STEP),
            slide(&world, start, push, HALF, DT)
        );
    }

    #[test]
    fn ground_found_on_floor_and_shallow_slope_only() {
        let flat = world([floor()]);
        let on = ground(&flat, standing(0.0, 0.0), HALF).unwrap();
        assert_eq!(on.normal, Vec3::Y);
        assert!(ground(&flat, standing(0.0, 0.5), HALF).is_none());

        // A 60° slope is a wall, not a floor.
        let steep = Brush::from_planes(&[
            Plane::through(Vec3::new(3f32.sqrt(), 1.0, 0.0), Vec3::ZERO),
            Plane::through(Vec3::NEG_Y, Vec3::NEG_Y * 5.0),
            Plane::through(Vec3::NEG_X, Vec3::NEG_X * 5.0),
            Plane::through(Vec3::Z, Vec3::Z * 5.0),
            Plane::through(Vec3::NEG_Z, Vec3::NEG_Z * 5.0),
        ])
        .unwrap();
        let wall_world = world([steep]);
        let resting = slide(
            &wall_world,
            Vec3::new(-1.0, 3.0, 0.0),
            Vec3::NEG_Y * 30.0,
            HALF,
            DT,
        );
        assert!(ground(&wall_world, resting.position, HALF).is_none());
    }

    #[test]
    fn snap_down_reaches_only_nearby_floors() {
        let world = world([floor()]);

        let snapped = snap_down(&world, standing(0.0, 0.3), HALF, STEP).unwrap();
        assert!((snapped.y - standing(0.0, 0.0).y).abs() < 0.01);
        assert!(snap_down(&world, standing(0.0, 1.0), HALF, STEP).is_none());
    }

    #[test]
    fn along_planes_follows_the_crease_of_two() {
        // Floor and a wall: the crease runs along Z.
        let crease = along_planes(Vec3::new(1.0, -1.0, 1.0), &[Vec3::Y, Vec3::NEG_X]);

        assert!(crease.x.abs() < 0.02 && crease.y.abs() < 0.02, "{crease:?}");
        assert!(crease.z > 0.9);

        // Into a three-wall corner: no way along all of them.
        let corner = [Vec3::NEG_X, Vec3::NEG_Y, Vec3::NEG_Z];
        assert_eq!(along_planes(Vec3::ONE, &corner), Vec3::ZERO);
    }
}
