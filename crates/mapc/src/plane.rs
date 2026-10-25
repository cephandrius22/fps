//! Planes in double precision. Brushes are built and cut in f64 so
//! corners land on whole units even where a huge base polygon is
//! clipped down to a small face.

use glam::DVec3;

/// How far from a plane a point still counts as on it, map units.
pub(crate) const ON_EPSILON: f64 = 0.01;
/// Normals this close to parallel (1 − cos) are one direction.
const PARALLEL_EPSILON: f64 = 1e-9;

/// Points `p` with `normal · p = dist`; the front (outside a brush) is
/// where greater.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plane {
    /// Unit normal, pointing out of the brush.
    pub normal: DVec3,
    /// Distance from the origin along the normal.
    pub dist: f64,
}

impl Plane {
    /// The plane through three points that run clockwise seen from its
    /// front (the .map convention). None when they are collinear.
    ///
    /// ```text
    ///   p0 ◄──── p1      seen from the front;
    ///            │       normal = (p0 − p1) × (p2 − p1)
    ///            ▼       points at the viewer
    ///            p2
    /// ```
    pub(crate) fn from_points([p0, p1, p2]: [DVec3; 3]) -> Option<Self> {
        let normal = (p0 - p1).cross(p2 - p1).try_normalize()?;
        Some(Self {
            normal,
            dist: normal.dot(p1),
        })
    }

    /// Signed distance of `p`: positive in front.
    pub(crate) fn distance(&self, p: DVec3) -> f64 {
        self.normal.dot(p) - self.dist
    }

    /// Whether `other` is this plane, facing the same way.
    pub(crate) fn same(&self, other: &Plane) -> bool {
        self.normal.dot(other.normal) > 1.0 - PARALLEL_EPSILON
            && (self.dist - other.dist).abs() < ON_EPSILON
    }

    /// In the level's single precision.
    pub(crate) fn to_level(self) -> level::Plane {
        level::Plane {
            normal: self.normal.as_vec3(),
            dist: self.dist as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clockwise_points_face_the_viewer() {
        // Quake's convention, from a real .map: a wall at x = 272 seen
        // from +X.
        let points = [
            DVec3::new(272.0, -208.0, -16.0),
            DVec3::new(272.0, -272.0, -16.0),
            DVec3::new(272.0, -272.0, 48.0),
        ];
        let plane = Plane::from_points(points).unwrap();

        assert_eq!(plane.normal, DVec3::X);
        assert_eq!(plane.dist, 272.0);
        assert_eq!(plane.distance(DVec3::new(300.0, 0.0, 0.0)), 28.0);
    }

    #[test]
    fn collinear_points_make_no_plane() {
        let points = [DVec3::ZERO, DVec3::X, DVec3::X * 2.0];

        assert!(Plane::from_points(points).is_none());
    }

    #[test]
    fn same_means_same_facing() {
        let plane = Plane {
            normal: DVec3::Z,
            dist: 64.0,
        };
        let nudged = Plane {
            dist: 64.001,
            ..plane
        };

        let opposite = Plane {
            normal: DVec3::NEG_Z,
            dist: -64.0,
        };

        assert!(plane.same(&nudged));
        assert!(!plane.same(&opposite));
    }
}
