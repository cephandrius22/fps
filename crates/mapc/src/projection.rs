//! Texture mapping: how a face's texture lies on it. Texel coordinates
//! are `s = p · s_axis + s_offset` (and t alike), with each axis divided
//! by its scale, as Quake's qbsp computes them.
//!
//! Quake standard faces take their axes from the world axis nearest the
//! face's normal, then rotate them:
//!
//! ```text
//!   nearest axis       s axis     t axis
//!   ±Z (floor, ceil)   +X         −Y
//!   ±X (wall)          +Y         −Z
//!   ±Y (wall)          +X         −Z
//! ```
//!
//! Valve 220 faces give their axes outright (rotation already applied),
//! so textures can lie at any angle.

use glam::{DVec2, DVec3};

use mapfile::{Axes, Face};

/// Per nearest axis: the axis, then s and t (qbsp's `baseaxis`). Ties
/// go to the first, so a 45° wall maps like a floor-facing one would.
const QUAKE_AXES: [[DVec3; 3]; 6] = [
    [DVec3::Z, DVec3::X, DVec3::NEG_Y],
    [DVec3::NEG_Z, DVec3::X, DVec3::NEG_Y],
    [DVec3::X, DVec3::Y, DVec3::NEG_Z],
    [DVec3::NEG_X, DVec3::Y, DVec3::NEG_Z],
    [DVec3::Y, DVec3::X, DVec3::NEG_Z],
    [DVec3::NEG_Y, DVec3::X, DVec3::NEG_Z],
];
/// A dot product must beat the best by this to win.
const TIE_EPSILON: f64 = 1e-4;
/// Axes and offsets this close map alike.
const MATCH_EPSILON: f64 = 1e-6;

/// One face's texture mapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Projection {
    /// Texels per map unit, across and down.
    axes: [DVec3; 2],
    /// Texels added.
    offset: DVec2,
}

impl Projection {
    /// The mapping of `face`, whose plane has `normal`.
    pub(crate) fn new(face: &Face, normal: DVec3) -> Self {
        let axes = match face.axes {
            Axes::Standard => rotate(quake_axes(normal), face.rotation),
            Axes::Valve(axes) => axes,
        };

        // A zero scale means the default, one unit per texel.
        let scale = face.scale.map(|s| if s == 0.0 { 1.0 } else { s });
        Self {
            axes: [axes[0] / scale.x, axes[1] / scale.y],
            offset: face.offset,
        }
    }

    /// Whether `other` maps every point to the same texel.
    pub(crate) fn matches(&self, other: &Projection) -> bool {
        self.axes[0].abs_diff_eq(other.axes[0], MATCH_EPSILON)
            && self.axes[1].abs_diff_eq(other.axes[1], MATCH_EPSILON)
            && self.offset.abs_diff_eq(other.offset, MATCH_EPSILON)
    }

    /// Texel coordinates of `p`.
    pub(crate) fn texel(&self, p: DVec3) -> DVec2 {
        DVec2::new(p.dot(self.axes[0]), p.dot(self.axes[1])) + self.offset
    }
}

/// Quake's s and t for the world axis nearest `normal`.
fn quake_axes(normal: DVec3) -> [DVec3; 2] {
    let mut best = (0.0, 0);
    for (i, [axis, ..]) in QUAKE_AXES.iter().enumerate() {
        let dot = normal.dot(*axis);
        if dot > best.0 + TIE_EPSILON {
            best = (dot, i);
        }
    }

    let [_, s, t] = QUAKE_AXES[best.1];
    [s, t]
}

/// Turns both axes by `degrees` in their own plane, as qbsp does: about
/// the components each axis uses, exactly for right angles.
fn rotate(axes: [DVec3; 2], degrees: f64) -> [DVec3; 2] {
    let (sin, cos) = match degrees {
        0.0 => (0.0, 1.0),
        90.0 => (1.0, 0.0),
        180.0 => (0.0, -1.0),
        270.0 => (-1.0, 0.0),
        _ => degrees.to_radians().sin_cos(),
    };

    // The component each axis runs along (its only nonzero one).
    let along = |v: DVec3| (0..3).find(|&i| v[i] != 0.0).unwrap_or(2);
    let (sv, tv) = (along(axes[0]), along(axes[1]));

    axes.map(|mut v| {
        let (s, t) = (v[sv], v[tv]);
        v[sv] = cos * s - sin * t;
        v[tv] = sin * s + cos * t;
        v
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(axes: Axes, rotation: f64, scale: DVec2) -> Face {
        Face {
            points: [DVec3::ZERO; 3],
            texture: "floor".into(),
            axes,
            offset: DVec2::new(8.0, -4.0),
            rotation,
            scale,
        }
    }

    #[test]
    fn standard_axes_follow_the_nearest_world_axis() {
        let p = DVec3::new(10.0, 20.0, 30.0);
        let cases = [
            (DVec3::Z, DVec2::new(10.0, -20.0)),
            (DVec3::NEG_Z, DVec2::new(10.0, -20.0)),
            (DVec3::X, DVec2::new(20.0, -30.0)),
            (DVec3::NEG_Y, DVec2::new(10.0, -30.0)),
            (DVec3::new(0.8, 0.0, 0.6), DVec2::new(20.0, -30.0)),
            (
                DVec3::new(1.0, 0.0, 1.0).normalize(),
                DVec2::new(10.0, -20.0),
            ),
        ];

        for (normal, texel) in cases {
            let projection = Projection::new(&face(Axes::Standard, 0.0, DVec2::ONE), normal);
            let offset = DVec2::new(8.0, -4.0);
            assert_eq!(projection.texel(p), texel + offset, "{normal}");
        }
    }

    #[test]
    fn rotation_and_scale_apply_to_standard_axes() {
        let floor =
            |rotation, scale| Projection::new(&face(Axes::Standard, rotation, scale), DVec3::Z);
        let p = DVec3::new(10.0, 20.0, 0.0);

        // 90°: s runs along +Y, t along +X.
        assert_eq!(floor(90.0, DVec2::ONE).texel(p), DVec2::new(28.0, 6.0));
        // Two units per texel across, half a unit down; zero means one.
        assert_eq!(
            floor(0.0, DVec2::new(2.0, 0.5)).texel(p),
            DVec2::new(13.0, -44.0)
        );
        assert_eq!(floor(0.0, DVec2::ZERO).texel(p), DVec2::new(18.0, -24.0));

        let turned = floor(45.0, DVec2::ONE).texel(DVec3::X);
        let half = std::f64::consts::FRAC_1_SQRT_2;
        assert!(turned.abs_diff_eq(DVec2::new(8.0 + half, -4.0 + half), 1e-12));
    }

    #[test]
    fn valve_axes_are_used_as_given() {
        let axes = Axes::Valve([DVec3::new(0.0, 0.0, 1.0), DVec3::new(1.0, 0.0, 0.0)]);
        let projection = Projection::new(&face(axes, 30.0, DVec2::new(2.0, 1.0)), DVec3::Y);

        assert_eq!(
            projection.texel(DVec3::new(10.0, 99.0, 30.0)),
            DVec2::new(15.0 + 8.0, 10.0 - 4.0)
        );
    }
}
