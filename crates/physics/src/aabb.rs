//! Axis-aligned boxes: moving hulls, trigger volumes, broadphase bounds.

use glam::Vec3;

/// Axis-aligned box, `min` ≤ `max` on every axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    /// Lowest corner.
    pub min: Vec3,
    /// Highest corner.
    pub max: Vec3,
}

impl Aabb {
    /// Box spanning `center ± half`.
    pub fn from_center(center: Vec3, half: Vec3) -> Self {
        Self {
            min: center - half,
            max: center + half,
        }
    }

    /// Smallest box holding every point; None for no points.
    pub fn enclosing(points: impl IntoIterator<Item = Vec3>) -> Option<Self> {
        let mut points = points.into_iter();
        let first = points.next()?;

        let start = Self {
            min: first,
            max: first,
        };
        Some(points.fold(start, |b, p| Self {
            min: b.min.min(p),
            max: b.max.max(p),
        }))
    }

    /// Center point.
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Half the size on each axis.
    pub fn half(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    /// Moved by `offset`.
    pub fn shifted(&self, offset: Vec3) -> Self {
        Self {
            min: self.min + offset,
            max: self.max + offset,
        }
    }

    /// Grown by `amount` on every side.
    pub fn grown(&self, amount: Vec3) -> Self {
        Self {
            min: self.min - amount,
            max: self.max + amount,
        }
    }

    /// Union of both boxes.
    pub fn union(&self, other: &Aabb) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// True when the boxes share volume. Touching faces do not count.
    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.min.cmplt(other.max).all() && other.min.cmplt(self.max).all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit() -> Aabb {
        Aabb::from_center(Vec3::ZERO, Vec3::splat(0.5))
    }

    #[test]
    fn center_and_half_round_trip() {
        let b = Aabb::from_center(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.5, 1.0, 2.0));

        assert_eq!(b.center(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(b.half(), Vec3::new(0.5, 1.0, 2.0));
    }

    #[test]
    fn enclosing_spans_all_points() {
        let b = Aabb::enclosing([Vec3::new(1.0, -2.0, 0.0), Vec3::new(-1.0, 3.0, 5.0)]).unwrap();

        assert_eq!(b.min, Vec3::new(-1.0, -2.0, 0.0));
        assert_eq!(b.max, Vec3::new(1.0, 3.0, 5.0));
        assert_eq!(Aabb::enclosing([]), None);
    }

    #[test]
    fn overlap_excludes_touching_faces() {
        let touching = unit().shifted(Vec3::X);
        let sharing = unit().shifted(Vec3::X * 0.9);

        assert!(!unit().overlaps(&touching));
        assert!(unit().overlaps(&sharing));
    }

    #[test]
    fn grown_and_union_cover_both() {
        let b = unit().grown(Vec3::splat(0.5));
        assert_eq!(b.half(), Vec3::ONE);

        let u = unit().union(&unit().shifted(Vec3::Y * 2.0));
        assert_eq!((u.min.y, u.max.y), (-0.5, 2.5));
    }
}
