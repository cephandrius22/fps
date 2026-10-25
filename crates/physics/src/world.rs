//! The collision world: every solid body, and box traces through them.
//!
//! ```text
//!   start ●──────────────►○ end         box of half-size `half`
//!                 ┌────┐
//!   result  ●────►│body│                fraction 0.4, normal −X
//!                 └────┘
//! ```

use base::{SlotId, Slots};
use glam::Vec3;

use crate::DIST_EPSILON;
use crate::aabb::Aabb;
use crate::brush::{Brush, Clip};

/// Handle to a body in a [`World`]. Detects use after removal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyId(SlotId);

/// A brush placed in the world. Moves by `offset` only, never rotates.
struct Body {
    brush: Brush,
    offset: Vec3,
}

impl Body {
    fn bounds(&self) -> Aabb {
        self.brush.bounds().shifted(self.offset)
    }
}

/// Result of sweeping a box through the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    /// Share of the sweep completed, 0..1. 1 means nothing was hit.
    pub fraction: f32,
    /// Where the box ended: just short of whatever it hit.
    pub end: Vec3,
    /// Surface normal at the hit; None when nothing was hit.
    pub normal: Option<Vec3>,
    /// What was hit, or the body the box started inside.
    pub body: Option<BodyId>,
    /// The box started inside a body.
    pub start_solid: bool,
    /// The box never got out of a body; `fraction` is 0.
    pub all_solid: bool,
}

impl Trace {
    fn clear(end: Vec3) -> Self {
        Self {
            fraction: 1.0,
            end,
            normal: None,
            body: None,
            start_solid: false,
            all_solid: false,
        }
    }

    /// True when the sweep stopped short.
    pub fn hit(&self) -> bool {
        self.fraction < 1.0
    }
}

/// All solid bodies. Static level brushes and moving ones (doors) alike.
#[derive(Default)]
pub struct World {
    bodies: Slots<Body>,
}

impl World {
    /// Adds a solid brush at its own coordinates (offset zero).
    pub fn add(&mut self, brush: Brush) -> BodyId {
        let body = Body {
            brush,
            offset: Vec3::ZERO,
        };
        BodyId(self.bodies.insert(body))
    }

    /// Removes a body. False if it was already gone.
    pub fn remove(&mut self, id: BodyId) -> bool {
        self.bodies.remove(id.0).is_some()
    }

    /// Moves a body to `offset` from where it was added. False if gone.
    pub fn set_offset(&mut self, id: BodyId, offset: Vec3) -> bool {
        let Some(body) = self.bodies.get_mut(id.0) else {
            return false;
        };

        body.offset = offset;
        true
    }

    /// World-space bounds of a body.
    pub fn bounds(&self, id: BodyId) -> Option<Aabb> {
        self.bodies.get(id.0).map(Body::bounds)
    }

    /// True when `aabb` shares volume with the body.
    pub fn overlaps(&self, id: BodyId, aabb: &Aabb) -> bool {
        let Some(body) = self.bodies.get(id.0) else {
            return false;
        };

        body.brush
            .overlaps_box(aabb.center() - body.offset, aabb.half())
    }

    /// Sweeps a box of half-size `half` from `start` to `end` and stops at
    /// the first body in the way. A zero `half` traces a ray.
    pub fn trace(&self, start: Vec3, end: Vec3, half: Vec3) -> Trace {
        let mut trace = Trace::clear(end);

        // Broadphase: only bodies the whole sweep could reach.
        let reach = Aabb::from_center(start, half)
            .union(&Aabb::from_center(end, half))
            .grown(Vec3::splat(DIST_EPSILON));

        for (id, body) in self.bodies.iter() {
            if !reach.overlaps(&body.bounds()) {
                continue;
            }

            let clip = body
                .brush
                .clip(start - body.offset, end - body.offset, half);
            match clip {
                Clip::Miss => {}
                Clip::StartSolid { stays } => {
                    trace.start_solid = true;
                    trace.body = Some(BodyId(id));
                    if stays {
                        trace.all_solid = true;
                        trace.fraction = 0.0;
                    }
                }
                Clip::Hit { fraction, normal } if fraction < trace.fraction => {
                    trace.fraction = fraction;
                    trace.normal = Some(normal);
                    trace.body = Some(BodyId(id));
                }
                Clip::Hit { .. } => {}
            }
        }

        trace.end = start + (end - start) * trace.fraction;
        trace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAY: Vec3 = Vec3::ZERO;

    fn wall_at_x(x: f32) -> Brush {
        Brush::cuboid(Vec3::new(x, -5.0, -5.0), Vec3::new(x + 1.0, 5.0, 5.0))
    }

    #[test]
    fn empty_world_lets_everything_through() {
        let trace = World::default().trace(Vec3::ZERO, Vec3::X * 10.0, RAY);

        assert!(!trace.hit());
        assert_eq!(trace.end, Vec3::X * 10.0);
        assert_eq!(trace.normal, None);
    }

    #[test]
    fn nearest_body_wins() {
        let mut world = World::default();
        let far = world.add(wall_at_x(6.0));
        let near = world.add(wall_at_x(3.0));

        let trace = world.trace(Vec3::ZERO, Vec3::X * 10.0, RAY);

        assert_eq!(trace.body, Some(near));
        assert_ne!(trace.body, Some(far));
        assert_eq!(trace.normal, Some(Vec3::NEG_X));
        assert!((trace.end.x - 3.0).abs() < 0.01 && trace.end.x < 3.0);
    }

    #[test]
    fn offset_moves_the_body_and_removal_clears_it() {
        let mut world = World::default();
        let wall = world.add(wall_at_x(3.0));

        world.set_offset(wall, Vec3::X * 2.0);
        let trace = world.trace(Vec3::ZERO, Vec3::X * 10.0, RAY);
        assert!((trace.end.x - 5.0).abs() < 0.01);
        assert_eq!(world.bounds(wall).unwrap().min.x, 5.0);

        assert!(world.remove(wall));
        assert!(!world.trace(Vec3::ZERO, Vec3::X * 10.0, RAY).hit());
        assert!(!world.set_offset(wall, Vec3::ZERO));
    }

    #[test]
    fn starting_inside_is_flagged() {
        let mut world = World::default();
        let wall = world.add(wall_at_x(-0.5));

        let out = world.trace(Vec3::ZERO, Vec3::X * 5.0, RAY);
        assert!(out.start_solid && !out.all_solid);

        let stuck = world.trace(Vec3::ZERO, Vec3::X * 0.1, RAY);
        assert!(stuck.all_solid);
        assert_eq!((stuck.fraction, stuck.body), (0.0, Some(wall)));
        assert_eq!(stuck.end, Vec3::ZERO);
    }

    #[test]
    fn overlap_respects_offset() {
        let mut world = World::default();
        let wall = world.add(wall_at_x(3.0));
        let player = Aabb::from_center(Vec3::new(3.2, 0.0, 0.0), Vec3::splat(0.4));

        assert!(world.overlaps(wall, &player));
        world.set_offset(wall, Vec3::Y * 20.0);
        assert!(!world.overlaps(wall, &player));
    }
}
