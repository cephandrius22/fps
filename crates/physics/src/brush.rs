//! Convex brushes: the solid volumes a level is made of. A brush is the
//! space behind all of its planes.
//!
//! ```text
//!          normal
//!            ▲
//!   outside  │  (n·p > dist)
//!   ─────────┼───────── plane
//!   inside      (n·p < dist)
//! ```
//!
//! Tracing a box against a brush is tracing the box's center against the
//! brush grown by the box: each plane is pushed out by the box's extent
//! along its normal. That is exact for faces, and for edges and corners
//! along the axes too, because every brush also carries axis-aligned
//! "bevel" planes at its bounds. Non-axial edges are slightly loose (a box
//! corner can stop up to a box-width early there), as in Quake.

use glam::{Mat3, Quat, Vec3};

use crate::DIST_EPSILON;
use crate::aabb::Aabb;

/// A vertex counts as on the brush within this distance of a plane.
const ON_PLANE: f32 = 1e-4;
/// Three planes nearer than this to parallel define no corner.
const MIN_DETERMINANT: f32 = 1e-6;
/// Normals this close to an axis count as axial (no bevel needed).
const AXIAL: f32 = 1.0 - 1e-5;
/// A tetrahedron is the smallest volume: 4 corners.
const MIN_CORNERS: usize = 4;
/// A polygon needs 3 corners; fewer is an edge or point (a bevel).
const MIN_POLYGON: usize = 3;

/// Infinite plane: points `p` with `normal · p = dist`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// Unit normal, pointing out of the brush.
    pub normal: Vec3,
    /// Distance from the origin along `normal`.
    pub dist: f32,
}

impl Plane {
    /// Plane facing `normal` (any length) through `point`.
    pub fn through(normal: Vec3, point: Vec3) -> Self {
        let normal = normal.normalize();
        Self {
            normal,
            dist: normal.dot(point),
        }
    }

    /// Signed distance from the plane; positive in front (outside).
    pub fn distance(&self, point: Vec3) -> f32 {
        self.normal.dot(point) - self.dist
    }

    /// `dist` with the plane pushed out by a box of half-size `half`.
    fn expanded_dist(&self, half: Vec3) -> f32 {
        self.dist + half.dot(self.normal.abs())
    }
}

/// How a box sweep met one brush.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Clip {
    /// Never touches it.
    Miss,
    /// Touches it at `fraction` of the way, on the face facing `normal`.
    Hit { fraction: f32, normal: Vec3 },
    /// Starts inside. `stays` when the whole sweep is inside.
    StartSolid { stays: bool },
}

/// One polygon of a brush's surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Outward unit normal.
    pub normal: Vec3,
    /// Corners, counter-clockwise seen from outside (the front-face
    /// winding the renderer expects).
    pub corners: Vec<Vec3>,
}

/// Convex solid: the intersection of the spaces behind its planes.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    planes: Vec<Plane>,
    /// Where three or more planes meet; may hold duplicates.
    corners: Vec<Vec3>,
    bounds: Aabb,
}

impl Brush {
    /// Axis-aligned box from `min` to `max`.
    pub fn cuboid(min: Vec3, max: Vec3) -> Self {
        let planes = vec![
            Plane::through(Vec3::X, max),
            Plane::through(Vec3::NEG_X, min),
            Plane::through(Vec3::Y, max),
            Plane::through(Vec3::NEG_Y, min),
            Plane::through(Vec3::Z, max),
            Plane::through(Vec3::NEG_Z, min),
        ];

        // Every combination of min/max per axis.
        let corners = (0..8)
            .map(|bits| {
                let pick = |bit: u32, axis: usize| {
                    if bits & bit == 0 {
                        min[axis]
                    } else {
                        max[axis]
                    }
                };
                Vec3::new(pick(1, 0), pick(2, 1), pick(4, 2))
            })
            .collect();

        Self {
            planes,
            corners,
            bounds: Aabb { min, max },
        }
    }

    /// Box of half-size `half` around `center`, turned by `rotation`.
    /// None only for a zero-size box.
    pub fn oriented_box(center: Vec3, half: Vec3, rotation: Quat) -> Option<Self> {
        let faces = [Vec3::X, Vec3::Y, Vec3::Z].into_iter().zip(half.to_array());
        let planes: Vec<_> = faces
            .flat_map(|(axis, extent)| {
                [axis, -axis].map(|side| {
                    let normal = rotation * side;
                    Plane::through(normal, center + normal * extent)
                })
            })
            .collect();

        Self::from_planes(&planes)
    }

    /// Brush behind every plane (normals need not be unit length). None
    /// when they enclose no volume. Planes must close the volume on all
    /// sides; corners are found where three planes meet.
    pub fn from_planes(planes: &[Plane]) -> Option<Self> {
        let mut planes: Vec<Plane> = planes
            .iter()
            .map(|p| {
                let len = p.normal.length();
                Plane {
                    normal: p.normal / len,
                    dist: p.dist / len,
                }
            })
            .collect();

        let corners = corners(&planes);
        if corners.len() < MIN_CORNERS {
            return None;
        }
        let bounds = Aabb::enclosing(corners.iter().copied())?;
        if bounds.half().min_element() <= ON_PLANE {
            return None;
        }

        add_bevels(&mut planes, &bounds);
        Some(Self {
            planes,
            corners,
            bounds,
        })
    }

    /// The visible surface: one polygon per face plane. Bevel planes
    /// only touch an edge or corner, so they yield no polygon.
    pub fn faces(&self) -> Vec<Face> {
        self.planes
            .iter()
            .filter_map(|plane| {
                let on_plane = self
                    .corners
                    .iter()
                    .copied()
                    .filter(|&c| plane.distance(c).abs() <= ON_PLANE);
                let corners = wind(dedup(on_plane), plane.normal);

                (corners.len() >= MIN_POLYGON).then_some(Face {
                    normal: plane.normal,
                    corners,
                })
            })
            .collect()
    }

    /// Faces plus bevel planes.
    pub fn planes(&self) -> &[Plane] {
        &self.planes
    }

    /// Axis-aligned bounds.
    pub fn bounds(&self) -> Aabb {
        self.bounds
    }

    /// True when a box of half-size `half` at `center` is inside the
    /// brush. Touching a face does not count.
    pub fn overlaps_box(&self, center: Vec3, half: Vec3) -> bool {
        self.planes
            .iter()
            .all(|p| p.normal.dot(center) < p.expanded_dist(half))
    }

    /// Sweeps a box of half-size `half` from `start` to `end`. Stops
    /// DIST_EPSILON short of the surface, so the next sweep starts
    /// outside.
    pub(crate) fn clip(&self, start: Vec3, end: Vec3, half: Vec3) -> Clip {
        // Latest plane entered and earliest left: the sweep is inside
        // the brush between the two.
        let mut enter = -1.0_f32;
        let mut leave = 1.0_f32;
        let mut normal = Vec3::ZERO;
        let mut starts_out = false;
        let mut ends_out = false;

        for plane in &self.planes {
            let dist = plane.expanded_dist(half);
            let d1 = plane.normal.dot(start) - dist;
            let d2 = plane.normal.dot(end) - dist;
            starts_out |= d1 > 0.0;
            ends_out |= d2 > 0.0;

            // Wholly in front of one face: misses the brush.
            if d1 > 0.0 && (d2 >= DIST_EPSILON || d2 >= d1) {
                return Clip::Miss;
            }
            // Wholly behind this face: it decides nothing.
            if d1 <= 0.0 && d2 <= 0.0 {
                continue;
            }

            if d1 > d2 {
                let f = ((d1 - DIST_EPSILON) / (d1 - d2)).max(0.0);
                if f > enter {
                    enter = f;
                    normal = plane.normal;
                }
            } else {
                let f = ((d1 + DIST_EPSILON) / (d1 - d2)).min(1.0);
                leave = leave.min(f);
            }
        }

        if !starts_out {
            return Clip::StartSolid { stays: !ends_out };
        }
        if enter > -1.0 && enter < leave {
            return Clip::Hit {
                fraction: enter,
                normal,
            };
        }

        Clip::Miss
    }
}

/// Points where three planes meet and that lie on or behind every plane.
fn corners(planes: &[Plane]) -> Vec<Vec3> {
    let mut corners = Vec::new();

    for (i, a) in planes.iter().enumerate() {
        for (j, b) in planes.iter().enumerate().skip(i + 1) {
            for c in planes.iter().skip(j + 1) {
                let Some(point) = intersect(a, b, c) else {
                    continue;
                };
                if planes.iter().all(|p| p.distance(point) <= ON_PLANE) {
                    corners.push(point);
                }
            }
        }
    }

    corners
}

/// Points with near-duplicates dropped (where four or more planes meet,
/// the same corner is found more than once).
fn dedup(points: impl Iterator<Item = Vec3>) -> Vec<Vec3> {
    let mut unique: Vec<Vec3> = Vec::new();

    for p in points {
        if unique.iter().all(|u| u.distance(p) > ON_PLANE) {
            unique.push(p);
        }
    }

    unique
}

/// Sorts coplanar points counter-clockwise around `normal`, seen from
/// the side it points to.
fn wind(mut points: Vec<Vec3>, normal: Vec3) -> Vec<Vec3> {
    let Some(&first) = points.first() else {
        return points;
    };
    let center = points.iter().sum::<Vec3>() / points.len() as f32;
    let u = (first - center).normalize_or_zero();
    let v = normal.cross(u);

    let angle = |p: &Vec3| {
        let d = *p - center;
        d.dot(v).atan2(d.dot(u))
    };
    points.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
    points
}

/// The single point on all three planes, if they are not parallel.
fn intersect(a: &Plane, b: &Plane, c: &Plane) -> Option<Vec3> {
    let normals = Mat3::from_cols(a.normal, b.normal, c.normal).transpose();
    if normals.determinant().abs() < MIN_DETERMINANT {
        return None;
    }

    Some(normals.inverse() * Vec3::new(a.dist, b.dist, c.dist))
}

/// Adds an axis-aligned plane at the bounds for every axis direction no
/// face already covers, so box sweeps stop correctly at corners.
fn add_bevels(planes: &mut Vec<Plane>, bounds: &Aabb) {
    let axes = [
        (Vec3::X, bounds.max),
        (Vec3::NEG_X, bounds.min),
        (Vec3::Y, bounds.max),
        (Vec3::NEG_Y, bounds.min),
        (Vec3::Z, bounds.max),
        (Vec3::NEG_Z, bounds.min),
    ];

    for (axis, point) in axes {
        if planes.iter().any(|p| p.normal.dot(axis) > AXIAL) {
            continue;
        }
        planes.push(Plane::through(axis, point));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    fn unit_cube() -> Brush {
        Brush::cuboid(Vec3::splat(-0.5), Vec3::splat(0.5))
    }

    /// Ramp rising toward -X: 2 m long, 1 m high, 1 m wide.
    ///
    /// ```text
    ///   y            slope
    ///   1 ┤ ┌──╲
    ///     │ │    ╲
    ///   0 ┼─┴──────╲── x
    ///    -2         0
    /// ```
    fn ramp() -> Brush {
        Brush::from_planes(&[
            Plane::through(Vec3::new(1.0, 2.0, 0.0), Vec3::ZERO),
            Plane::through(Vec3::NEG_Y, Vec3::ZERO),
            Plane::through(Vec3::NEG_X, Vec3::new(-2.0, 0.0, 0.0)),
            Plane::through(Vec3::Z, Vec3::new(0.0, 0.0, 0.5)),
            Plane::through(Vec3::NEG_Z, Vec3::new(0.0, 0.0, -0.5)),
        ])
        .unwrap()
    }

    #[test]
    fn cuboid_has_six_faces_and_exact_bounds() {
        let cube = unit_cube();

        assert_eq!(cube.planes().len(), 6);
        assert_eq!(cube.bounds().min, Vec3::splat(-0.5));
    }

    #[test]
    fn from_planes_finds_bounds_and_adds_missing_bevels() {
        let ramp = ramp();
        let bounds = ramp.bounds();

        assert!((bounds.min - Vec3::new(-2.0, 0.0, -0.5)).length() < EPS);
        assert!((bounds.max - Vec3::new(0.0, 1.0, 0.5)).length() < EPS);
        // Five faces, plus bevels for +X and +Y, which the slope covers.
        assert_eq!(ramp.planes().len(), 7);
    }

    #[test]
    fn from_planes_rejects_open_or_empty_sets() {
        let slab = [
            Plane::through(Vec3::Y, Vec3::Y),
            Plane::through(Vec3::NEG_Y, Vec3::ZERO),
        ];
        assert_eq!(Brush::from_planes(&slab), None);

        // Two faces facing each other with no space between.
        let mut flat = unit_cube().planes().to_vec();
        flat[2] = Plane::through(Vec3::Y, Vec3::splat(-0.5));
        assert_eq!(Brush::from_planes(&flat), None);
    }

    #[test]
    fn oriented_box_turns_its_bounds() {
        let quarter = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
        let turned = Brush::oriented_box(Vec3::ZERO, Vec3::splat(0.5), quarter).unwrap();

        // A unit square turned 45° reaches √½ along X and Z.
        let reach = std::f32::consts::FRAC_1_SQRT_2;
        assert!((turned.bounds().max.x - reach).abs() < EPS);
        assert!((turned.bounds().max.y - 0.5).abs() < EPS);
    }

    #[test]
    fn faces_cover_the_surface_and_wind_outward() {
        for brush in [unit_cube(), ramp()] {
            let faces = brush.faces();

            for face in &faces {
                let [a, b, c, ..] = face.corners[..] else {
                    panic!("degenerate face {face:?}");
                };
                let winding = (b - a).cross(c - a).normalize();
                assert!((winding - face.normal).length() < EPS, "{face:?}");
            }
        }

        assert_eq!(unit_cube().faces().len(), 6);
        // Bevels add planes but no polygons: two triangles, three quads.
        let ramp_faces = ramp().faces();
        assert_eq!(ramp_faces.len(), 5);
        assert_eq!(
            ramp_faces.iter().filter(|f| f.corners.len() == 3).count(),
            2
        );
    }

    #[test]
    fn ray_hits_face_just_short_of_it() {
        let clip = unit_cube().clip(
            Vec3::new(0.0, 0.0, 2.5),
            Vec3::new(0.0, 0.0, -2.5),
            Vec3::ZERO,
        );

        let Clip::Hit { fraction, normal } = clip else {
            panic!("{clip:?}");
        };
        assert_eq!(normal, Vec3::Z);
        // Face at z = 0.5 is 2 m into a 5 m sweep.
        assert!((fraction - 0.4).abs() < 0.001);
        assert!(fraction < 0.4);
    }

    #[test]
    fn box_hits_earlier_by_its_half_size() {
        let half = Vec3::splat(0.25);
        let clip = unit_cube().clip(Vec3::new(0.0, 0.0, 2.5), Vec3::new(0.0, 0.0, -2.5), half);

        let Clip::Hit { fraction, .. } = clip else {
            panic!("{clip:?}");
        };
        assert!((fraction - 0.35).abs() < 0.001);
    }

    #[test]
    fn sweep_beside_the_brush_misses() {
        let start = Vec3::new(0.8, 0.0, 2.0);
        let end = Vec3::new(0.8, 0.0, -2.0);

        assert_eq!(unit_cube().clip(start, end, Vec3::ZERO), Clip::Miss);
        // A box that wide clips the edge.
        let wide = Vec3::splat(0.4);
        assert!(matches!(
            unit_cube().clip(start, end, wide),
            Clip::Hit { .. }
        ));
    }

    #[test]
    fn starting_inside_is_reported() {
        let cube = unit_cube();

        let leaving = cube.clip(Vec3::ZERO, Vec3::X * 2.0, Vec3::ZERO);
        assert_eq!(leaving, Clip::StartSolid { stays: false });

        let staying = cube.clip(Vec3::ZERO, Vec3::X * 0.1, Vec3::ZERO);
        assert_eq!(staying, Clip::StartSolid { stays: true });
    }

    #[test]
    fn ray_lands_on_the_slope() {
        let start = Vec3::new(-1.0, 3.0, 0.0);
        let clip = ramp().clip(start, Vec3::new(-1.0, -1.0, 0.0), Vec3::ZERO);

        let Clip::Hit { fraction, normal } = clip else {
            panic!("{clip:?}");
        };
        // The slope is 0.5 m high at x = -1: 2.5 m down a 4 m sweep.
        assert!((fraction - 0.625).abs() < 0.001);
        assert!((normal - Vec3::new(1.0, 2.0, 0.0).normalize()).length() < EPS);
    }

    #[test]
    fn overlap_needs_shared_volume() {
        let cube = unit_cube();
        let half = Vec3::splat(0.5);

        assert!(cube.overlaps_box(Vec3::new(0.9, 0.0, 0.0), half));
        assert!(!cube.overlaps_box(Vec3::new(1.0, 0.0, 0.0), half));
    }
}
