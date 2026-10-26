//! Shadows: whether a straight path from a surface to a light passes
//! through a solid brush.
//!
//! A segment is clipped against each brush's planes in turn; it passes
//! through the brush if some part of it lies behind every plane:
//!
//! ```text
//!        light ●
//!               ╲         enter = latest entry over all planes
//!            ┌───╳───┐    exit  = earliest exit
//!            │    ╲  │    blocked when enter < exit
//!            └─────╳─┘
//!                   ╲
//!           surface ●
//! ```
//!
//! Paths that only graze a brush's surface are not blocked, so light
//! skims along walls and floors. A brush the path starts inside does
//! not block it either: those samples are on faces buried in other
//! brushes, mostly doors and buttons shut in their frames, and they
//! should look lit once the door moves, not black.

use glam::DVec3;
use level::Level;

/// How far from a plane still counts as on it, map units.
const ON_EPSILON: f64 = 0.01;
/// Brush entities with "_shadow" "1" cast shadows; the world always
/// does.
const SHADOW_KEY: &str = "_shadow";
const SHADOW_ON: &str = "1";

/// A convex solid that casts shadows.
struct Occluder {
    mins: DVec3,
    maxs: DVec3,
    /// Outward (normal, dist) pairs.
    planes: Vec<(DVec3, f64)>,
}

/// Everything that casts shadows.
pub(crate) struct Occluders(Vec<Occluder>);

impl Occluders {
    /// The world's solid brushes, and those of brush entities marked
    /// "_shadow" "1", in the position the map places them.
    pub(crate) fn new(level: &Level) -> Self {
        let mut casting = vec![0];
        let marked = level
            .entities
            .iter()
            .filter(|e| e.get(SHADOW_KEY) == Some(SHADOW_ON));
        casting.extend(marked.filter_map(|e| e.model()).filter(|&m| m != 0));

        let brushes = casting
            .into_iter()
            .filter_map(|m| level.models.get(m))
            .flat_map(|model| &model.brushes)
            .filter(|b| b.contents.is_solid())
            .map(|b| Occluder {
                mins: b.mins.as_dvec3(),
                maxs: b.maxs.as_dvec3(),
                planes: b
                    .planes
                    .iter()
                    .map(|p| (p.normal.as_dvec3(), f64::from(p.dist)))
                    .collect(),
            })
            .collect();
        Self(brushes)
    }

    /// Whether the segment `from` → `to` passes through any occluder
    /// other than those `from` is inside.
    pub(crate) fn blocked(&self, from: DVec3, to: DVec3) -> bool {
        let (lo, hi) = (from.min(to), from.max(to));
        self.0
            .iter()
            .filter(|o| !(lo.cmpgt(o.maxs) | o.mins.cmpgt(hi)).any())
            .any(|o| !o.contains(from) && o.crosses(from, to))
    }
}

impl Occluder {
    /// Whether `p` is inside, deeper than the epsilon.
    fn contains(&self, p: DVec3) -> bool {
        self.planes
            .iter()
            .all(|&(normal, dist)| normal.dot(p) - dist < -ON_EPSILON)
    }

    /// Whether part of `from` → `to` lies inside, deeper than the
    /// epsilon.
    fn crosses(&self, from: DVec3, to: DVec3) -> bool {
        let (mut enter, mut exit) = (0.0_f64, 1.0_f64);

        for &(normal, dist) in &self.planes {
            let d0 = normal.dot(from) - dist;
            let d1 = normal.dot(to) - dist;

            // Wholly in front (or along the surface): outside.
            if d0 >= -ON_EPSILON && d1 >= -ON_EPSILON {
                return false;
            }
            if d0 < -ON_EPSILON && d1 < -ON_EPSILON {
                continue;
            }

            let crossing = d0 / (d0 - d1);
            match d0 > d1 {
                true => enter = enter.max(crossing),
                false => exit = exit.min(crossing),
            }
            if enter >= exit {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cuboid_model, level_with};

    /// One world cube, 0..64 on every axis.
    fn cube() -> Occluders {
        Occluders::new(&level_with(vec![cuboid_model(
            DVec3::ZERO,
            DVec3::splat(64.0),
        )]))
    }

    #[test]
    fn a_path_through_a_brush_is_blocked() {
        let occluders = cube();

        assert!(occluders.blocked(DVec3::new(-10.0, 32.0, 32.0), DVec3::new(80.0, 32.0, 32.0)));
        assert!(occluders.blocked(DVec3::new(-10.0, -10.0, -10.0), DVec3::splat(80.0)));
        let into = (DVec3::new(32.0, 32.0, 80.0), DVec3::splat(32.0));
        assert!(occluders.blocked(into.0, into.1), "ends inside");
    }

    #[test]
    fn paths_beside_grazing_or_leaving_are_not() {
        let occluders = cube();

        assert!(!occluders.blocked(DVec3::new(-10.0, 80.0, 32.0), DVec3::new(80.0, 80.0, 32.0)));
        assert!(!occluders.blocked(DVec3::new(-10.0, 64.0, 32.0), DVec3::new(80.0, 64.0, 32.0)));
        assert!(!occluders.blocked(DVec3::new(32.0, 32.0, 64.0), DVec3::new(32.0, 32.0, 200.0)));
        assert!(!occluders.blocked(DVec3::new(-10.0, 32.0, 32.0), DVec3::new(-1.0, 32.0, 32.0)));
    }

    #[test]
    fn a_path_escapes_the_brush_it_starts_in_but_no_other() {
        // The cube, and a wall beyond it at x 100..110.
        let mut world = cuboid_model(DVec3::ZERO, DVec3::splat(64.0));
        let wall = cuboid_model(DVec3::new(100.0, 0.0, 0.0), DVec3::new(110.0, 64.0, 64.0));
        world.brushes.extend(wall.brushes);
        let occluders = Occluders::new(&level_with(vec![world]));
        let start = DVec3::splat(32.0);

        assert!(!occluders.blocked(start, DVec3::new(80.0, 32.0, 32.0)));
        assert!(occluders.blocked(start, DVec3::new(120.0, 32.0, 32.0)));
    }

    #[test]
    fn only_solid_world_and_marked_entities_cast() {
        let mut level = level_with(vec![
            cuboid_model(DVec3::splat(1000.0), DVec3::splat(1001.0)),
            cuboid_model(DVec3::ZERO, DVec3::splat(64.0)),
        ]);
        let through = (DVec3::new(-10.0, 32.0, 32.0), DVec3::new(80.0, 32.0, 32.0));
        let door = |shadow: Option<&str>| {
            let mut fields = vec![
                ("classname".into(), "func_door".into()),
                ("model".into(), "*1".into()),
            ];
            fields.extend(shadow.map(|s| (SHADOW_KEY.into(), s.into())));
            level::Entity::new(fields)
        };

        level.entities.push(door(None));
        assert!(!Occluders::new(&level).blocked(through.0, through.1));

        level.entities[1] = door(Some(SHADOW_ON));
        assert!(Occluders::new(&level).blocked(through.0, through.1));

        level.models[1].brushes[0].contents = level::Contents::Water;
        assert!(
            !Occluders::new(&level).blocked(through.0, through.1),
            "water"
        );
    }
}
