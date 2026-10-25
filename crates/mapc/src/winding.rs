//! Convex polygons lying on a plane ("windings", as Quake's tools call
//! them), and cutting them with other planes. A brush face starts as a
//! huge square on its plane; clipping by the brush's other planes
//! leaves its polygon:
//!
//! ```text
//!   ┌───────────────┐            ┌─────┐
//!   │        ╲      │  clip by   │      ╲
//!   │  behind ╲     │  ──────►   │       ╲
//!   │          ╲    │  one plane │        ╲
//!   └───────────────┘            └─────────┘
//! ```
//!
//! Corners stay in order through every cut, so a winding that starts
//! counter-clockwise seen from its front stays so.

use glam::DVec3;

use crate::plane::{ON_EPSILON, Plane};

/// Half the side of the starting square, map units: past any map's
/// edge (Quake's are within ±4096, BSP2's ±65536).
pub(crate) const BASE_EXTENT: f64 = 262_144.0;
/// Coordinates this close to a whole unit snap to it, undoing f64 drift
/// from cutting the huge base square.
const SNAP_EPSILON: f64 = 1e-4;

/// Where a polygon lies relative to a plane.
#[derive(Debug)]
pub(crate) enum Cut {
    /// Wholly in front.
    Front,
    /// Wholly behind.
    Back,
    /// On the plane, within [`ON_EPSILON`].
    On,
    /// Across it, in two pieces.
    Split {
        /// The part in front.
        front: Winding,
        /// The part behind.
        back: Winding,
    },
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Front,
    Back,
    On,
}

/// A convex polygon, at least three corners.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Winding(Vec<DVec3>);

impl Winding {
    /// A square on `plane`, far bigger than any map, counter-clockwise
    /// seen from the front.
    pub(crate) fn base(plane: &Plane) -> Self {
        // Any direction across the plane will do; avoid the normal's
        // own axis so the projection is never zero.
        let n = plane.normal;
        let hint = match n.z.abs() >= n.x.abs().max(n.y.abs()) {
            true => DVec3::X,
            false => DVec3::Z,
        };
        let up = (hint - n * hint.dot(n)).normalize() * BASE_EXTENT;
        let right = up.cross(n);

        // right × up = n: right, then up, turns counter-clockwise.
        let center = n * plane.dist;
        Self(vec![
            center - right - up,
            center + right - up,
            center + right + up,
            center - right + up,
        ])
    }

    /// Wraps corners; None for fewer than three.
    #[cfg(test)]
    pub(crate) fn new(points: Vec<DVec3>) -> Option<Self> {
        (points.len() >= 3).then_some(Self(points))
    }

    /// The corners, in order.
    pub(crate) fn points(&self) -> &[DVec3] {
        &self.0
    }

    /// The same polygon seen from behind.
    pub(crate) fn reversed(&self) -> Self {
        Self(self.0.iter().rev().copied().collect())
    }

    /// Area-weighted normal (Newell's method): points at the side the
    /// corners run counter-clockwise from; its length is twice the area.
    pub(crate) fn newell(&self) -> DVec3 {
        let next = self.0.iter().cycle().skip(1);
        self.0.iter().zip(next).map(|(a, b)| a.cross(*b)).sum()
    }

    /// Area, square map units.
    pub(crate) fn area(&self) -> f64 {
        self.newell().length() / 2.0
    }

    /// Axis-aligned bounds.
    pub(crate) fn bounds(&self) -> (DVec3, DVec3) {
        let start = (DVec3::INFINITY, DVec3::NEG_INFINITY);
        self.0
            .iter()
            .fold(start, |(lo, hi), &p| (lo.min(p), hi.max(p)))
    }

    /// Corners within a hair of a whole unit, moved onto it.
    pub(crate) fn snapped(self) -> Self {
        let snap = |v: f64| match (v - v.round()).abs() < SNAP_EPSILON {
            true => v.round(),
            false => v,
        };
        Self(self.0.into_iter().map(|p| p.map(snap)).collect())
    }

    /// Where the polygon lies relative to `plane`, splitting it if it
    /// crosses.
    pub(crate) fn cut(&self, plane: &Plane) -> Cut {
        let dists: Vec<f64> = self.0.iter().map(|&p| plane.distance(p)).collect();
        let sides: Vec<Side> = dists.iter().map(|&d| side(d)).collect();

        let front = sides.contains(&Side::Front);
        let back = sides.contains(&Side::Back);
        match (front, back) {
            (false, false) => return Cut::On,
            (true, false) => return Cut::Front,
            (false, true) => return Cut::Back,
            (true, true) => {}
        }

        // Walk the edges; corners on the plane go to both pieces, and
        // each edge that crosses adds its crossing point to both.
        let mut front = Vec::new();
        let mut back = Vec::new();
        let count = self.0.len();
        for i in 0..count {
            let (p, side_i) = (self.0[i], sides[i]);
            match side_i {
                Side::On => {
                    front.push(p);
                    back.push(p);
                    continue;
                }
                Side::Front => front.push(p),
                Side::Back => back.push(p),
            }

            let j = (i + 1) % count;
            if sides[j] == Side::On || sides[j] == side_i {
                continue;
            }

            let crossing = crossing(p, self.0[j], dists[i], dists[j], plane);
            front.push(crossing);
            back.push(crossing);
        }

        Cut::Split {
            front: Self(front),
            back: Self(back),
        }
    }

    /// The part behind `plane`. A polygon lying on it is kept only when
    /// it faces the same way (the plane is its own).
    pub(crate) fn clip(self, plane: &Plane) -> Option<Self> {
        match self.cut(plane) {
            Cut::Front => None,
            Cut::Back => Some(self),
            Cut::Split { back, .. } => Some(back),
            Cut::On => (self.newell().dot(plane.normal) > 0.0).then_some(self),
        }
    }
}

fn side(dist: f64) -> Side {
    if dist > ON_EPSILON {
        return Side::Front;
    }
    if dist < -ON_EPSILON {
        return Side::Back;
    }
    Side::On
}

/// Where the edge `a`→`b` meets the plane. Exact on axial planes, where
/// the crossing's coordinate along the axis is the plane's distance.
fn crossing(a: DVec3, b: DVec3, da: f64, db: f64, plane: &Plane) -> DVec3 {
    let mut p = a + (b - a) * (da / (da - db));
    for axis in 0..3 {
        if plane.normal[axis] == 1.0 {
            p[axis] = plane.dist;
        } else if plane.normal[axis] == -1.0 {
            p[axis] = -plane.dist;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(normal: DVec3, dist: f64) -> Plane {
        Plane {
            normal: normal.normalize(),
            dist,
        }
    }

    /// The 64-unit square on z = 0, counter-clockwise seen from above.
    fn square() -> Winding {
        let corners = [(0.0, 0.0), (64.0, 0.0), (64.0, 64.0), (0.0, 64.0)];
        Winding::new(corners.map(|(x, y)| DVec3::new(x, y, 0.0)).to_vec()).unwrap()
    }

    #[test]
    fn base_lies_on_its_plane_counter_clockwise() {
        for normal in [DVec3::Z, DVec3::NEG_X, DVec3::new(1.0, 2.0, -3.0)] {
            let plane = plane(normal, 100.0);
            let base = Winding::base(&plane);

            assert!(
                base.points()
                    .iter()
                    .all(|&p| plane.distance(p).abs() < 1e-6)
            );
            assert!(base.newell().normalize().abs_diff_eq(plane.normal, 1e-9));
            assert!(base.area() > 1e10, "covers any map");
        }
    }

    #[test]
    fn cut_reports_which_side() {
        let w = square();

        assert!(matches!(w.cut(&plane(DVec3::Z, -1.0)), Cut::Front));
        assert!(matches!(w.cut(&plane(DVec3::Z, 1.0)), Cut::Back));
        assert!(matches!(w.cut(&plane(DVec3::Z, 0.0)), Cut::On));
        assert!(
            matches!(w.cut(&plane(DVec3::X, 64.0)), Cut::Back),
            "touching"
        );
    }

    #[test]
    fn split_pieces_keep_winding_and_area() {
        let Cut::Split { front, back } = square().cut(&plane(DVec3::X, 16.0)) else {
            panic!("crosses");
        };

        assert_eq!(front.area(), 48.0 * 64.0);
        assert_eq!(back.area(), 16.0 * 64.0);
        assert!(front.newell().z > 0.0 && back.newell().z > 0.0);
        assert!(back.points().iter().all(|p| p.x <= 16.0));
        assert!(
            back.points().contains(&DVec3::new(16.0, 64.0, 0.0)),
            "exact"
        );
    }

    #[test]
    fn clip_keeps_the_back_and_own_plane() {
        // x + y = 64, cutting the square corner to corner.
        let diagonal = plane(DVec3::new(1.0, 1.0, 0.0), 64.0 / 2f64.sqrt());
        let clipped = square().clip(&diagonal).unwrap();

        assert!((clipped.area() - 32.0 * 64.0).abs() < 1e-9, "half");
        assert!(square().clip(&plane(DVec3::Z, 0.0)).is_some(), "own plane");
        assert!(
            square().clip(&plane(DVec3::NEG_Z, 0.0)).is_none(),
            "opposite"
        );
        assert!(square().clip(&plane(DVec3::X, -1.0)).is_none(), "in front");
    }

    #[test]
    fn reversed_faces_the_other_way_and_snapping_rounds() {
        let w = square();
        assert!(w.reversed().newell().z < 0.0);

        let drifted = Winding::new(vec![
            DVec3::new(1e-6, 0.0, 0.0),
            DVec3::new(64.0, 0.5, 0.0),
            DVec3::new(63.999_99, 64.0, 0.0),
        ]);
        let snapped = drifted.unwrap().snapped();
        assert_eq!(snapped.points()[0].x, 0.0);
        assert_eq!(snapped.points()[1].y, 0.5, "not near a whole unit");
        assert_eq!(snapped.points()[2].x, 64.0);
        assert_eq!(w.bounds(), (DVec3::ZERO, DVec3::new(64.0, 64.0, 0.0)));
    }
}
