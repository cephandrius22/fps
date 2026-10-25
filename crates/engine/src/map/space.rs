//! Quake space → engine space. Quake is right-handed with Z up, in
//! units; the engine is right-handed with Y up, in meters.
//!
//! ```text
//!   Quake      engine
//!     x  ───►    x
//!     y  ───►   −z        all ÷ UNITS_PER_METER
//!     z  ───►    y
//! ```
//!
//! The swap is a rotation, so handedness, winding and plane distances
//! (once scaled) carry over unchanged.

use glam::Vec3;

use super::UNITS_PER_METER;
use crate::collision::{Aabb, Plane};

pub(super) fn to_engine_point(q: Vec3) -> Vec3 {
    to_engine_direction(q) / UNITS_PER_METER
}

pub(super) fn to_engine_direction(q: Vec3) -> Vec3 {
    Vec3::new(q.x, q.z, -q.y)
}

/// The axis swap is a rotation, so only the distance scales.
pub(super) fn to_engine_plane(p: bsp::Plane) -> Plane {
    Plane {
        normal: to_engine_direction(p.normal),
        dist: p.dist / UNITS_PER_METER,
    }
}

/// Quake angle (degrees, 0 = +X, 90 = +Y) → engine yaw (radians,
/// 0 = −Z, which is Quake +Y).
pub(super) fn to_engine_yaw(angle: f32) -> f32 {
    (angle - 90.0).to_radians()
}

/// Engine box for Quake bounds; the swap reverses one axis.
pub(super) fn bounds(mins: Vec3, maxs: Vec3) -> Aabb {
    let (a, b) = (to_engine_point(mins), to_engine_point(maxs));
    Aabb {
        min: a.min(b),
        max: a.max(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    #[test]
    fn axes_swap_to_y_up_and_scale_to_meters() {
        // Quake north (+Y) is engine −Z; Quake up is engine +Y.
        assert_eq!(to_engine_direction(Vec3::Y), Vec3::NEG_Z);
        assert_eq!(to_engine_direction(Vec3::Z), Vec3::Y);
        assert_eq!(
            to_engine_point(Vec3::new(32.0, 64.0, 96.0)),
            Vec3::new(1.0, 3.0, -2.0)
        );

        // Still right-handed: X × Y = Z holds after the swap.
        let [x, y, z] = [Vec3::X, Vec3::Y, Vec3::Z].map(to_engine_direction);
        assert!((x.cross(y) - z).length() < EPS);
    }

    #[test]
    fn planes_keep_the_same_points() {
        let quake = bsp::Plane {
            normal: Vec3::Z,
            dist: 64.0,
        };
        let plane = to_engine_plane(quake);
        let on_plane = to_engine_point(Vec3::new(5.0, 7.0, 64.0));

        assert!((plane.normal.dot(on_plane) - plane.dist).abs() < EPS);
    }

    #[test]
    fn bounds_stay_ordered_after_the_swap() {
        let b = bounds(Vec3::new(0.0, 0.0, 0.0), Vec3::new(32.0, 64.0, 96.0));

        assert_eq!(b.min, Vec3::new(0.0, 0.0, -2.0));
        assert_eq!(b.max, Vec3::new(1.0, 3.0, 0.0));
    }
}
