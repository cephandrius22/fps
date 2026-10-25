//! Game-facing collision types and their translation to the physics
//! crate. The only place engine and physics types meet, as `gfx.rs` is
//! for render.
//!
//! A level is made of convex [`Brush`]es. The same brush is both what
//! collides and, through [`Brush::mesh`], what is drawn, so the two
//! never disagree.

use glam::{Quat, Vec3};

use crate::gfx::{MeshData, Vertex};
use crate::world::EntityId;

const WHITE: [f32; 4] = [1.0; 4];

/// Infinite plane: points `p` with `normal · p = dist`. Brushes are the
/// space behind (opposite `normal`) all of their planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// Points out of the brush; any length.
    pub normal: Vec3,
    /// `normal · p` for points on the plane.
    pub dist: f32,
}

impl Plane {
    /// Plane facing `normal` through `point`.
    pub fn through(normal: Vec3, point: Vec3) -> Self {
        Self {
            normal,
            dist: normal.dot(point),
        }
    }

    fn to_physics(self) -> physics::Plane {
        physics::Plane {
            normal: self.normal,
            dist: self.dist,
        }
    }
}

/// Axis-aligned box: hulls, trigger volumes, bounds.
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
        Self::from_physics(physics::Aabb::from_center(center, half))
    }

    /// Moved by `offset`.
    pub fn shifted(&self, offset: Vec3) -> Self {
        Self::from_physics(self.to_physics().shifted(offset))
    }

    /// True when the boxes share volume. Touching faces do not count.
    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.to_physics().overlaps(&other.to_physics())
    }

    pub(crate) fn to_physics(self) -> physics::Aabb {
        physics::Aabb {
            min: self.min,
            max: self.max,
        }
    }

    pub(crate) fn from_physics(aabb: physics::Aabb) -> Self {
        Self {
            min: aabb.min,
            max: aabb.max,
        }
    }
}

/// Convex solid volume: the building block of levels, doors and props.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush(pub(crate) physics::Brush);

impl Brush {
    /// Axis-aligned box from `min` to `max`.
    pub fn cuboid(min: Vec3, max: Vec3) -> Self {
        Self(physics::Brush::cuboid(min, max))
    }

    /// Box of half-size `half` around `center`, turned by `rotation`.
    /// None for a zero-size box.
    pub fn oriented_box(center: Vec3, half: Vec3, rotation: Quat) -> Option<Self> {
        physics::Brush::oriented_box(center, half, rotation).map(Self)
    }

    /// The space behind every plane, e.g. a ramp or wedge. None when the
    /// planes enclose no volume; they must close it on all sides.
    pub fn from_planes(planes: &[Plane]) -> Option<Self> {
        let planes: Vec<_> = planes.iter().map(|p| p.to_physics()).collect();
        physics::Brush::from_planes(&planes).map(Self)
    }

    /// Axis-aligned bounds.
    pub fn bounds(&self) -> Aabb {
        Aabb::from_physics(self.0.bounds())
    }

    /// Renderable surface, white, with the texture projected along the
    /// nearest world axis and repeating every `meters_per_texture`: the
    /// way Quake textured brushes, so adjoining brushes line up.
    ///
    /// ```text
    ///   face mostly facing ±Y (floor, ceiling): u = x, v = z
    ///   face mostly facing ±X (wall):           u = z, v = −y
    ///   face mostly facing ±Z (wall):           u = x, v = −y
    /// ```
    pub fn mesh(&self, meters_per_texture: f32) -> MeshData {
        let mut data = MeshData::default();

        for face in self.0.faces() {
            let base = data.vertices.len() as u32;

            data.vertices.extend(face.corners.iter().map(|&p| Vertex {
                pos: p.to_array(),
                normal: face.normal.to_array(),
                uv: project(p, face.normal, meters_per_texture),
                color: WHITE,
                ..Default::default()
            }));

            // Convex polygon: a fan from the first corner.
            let count = face.corners.len() as u32;
            data.indices
                .extend((1..count - 1).flat_map(|i| [base, base + i, base + i + 1]));
        }

        data
    }
}

/// Texture coordinates for world point `p` on a face facing `normal`.
fn project(p: Vec3, normal: Vec3, meters_per_texture: f32) -> [f32; 2] {
    let n = normal.abs();
    let uv = if n.y >= n.x && n.y >= n.z {
        [p.x, p.z]
    } else if n.x >= n.z {
        [p.z, -p.y]
    } else {
        [p.x, -p.y]
    };

    uv.map(|c| c / meters_per_texture)
}

/// Result of sweeping a box through a [`World`](crate::World).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    /// Share of the sweep completed, 0..1. 1 means nothing was hit.
    pub fraction: f32,
    /// Where the box ended: just short of whatever it hit.
    pub end: Vec3,
    /// Surface normal at the hit; None when nothing was hit.
    pub normal: Option<Vec3>,
    /// The entity hit (or started inside). None for static level
    /// brushes and for misses.
    pub entity: Option<EntityId>,
    /// The box started inside something solid.
    pub start_solid: bool,
}

impl Trace {
    /// True when the sweep stopped short.
    pub fn hit(&self) -> bool {
        self.fraction < 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    #[test]
    fn cuboid_mesh_has_two_triangles_per_face_wound_outward() {
        let mesh = Brush::cuboid(Vec3::ZERO, Vec3::new(2.0, 1.0, 4.0)).mesh(1.0);

        assert_eq!(mesh.vertices.len(), 6 * 4);
        assert_eq!(mesh.indices.len(), 6 * 6);

        let pos = |i: u32| Vec3::from(mesh.vertices[i as usize].pos);
        for tri in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(pos);
            let winding = (b - a).cross(c - a).normalize();
            let normal = Vec3::from(mesh.vertices[tri[0] as usize].normal);
            assert!((winding - normal).length() < EPS, "{tri:?}");
        }
    }

    #[test]
    fn texture_repeats_per_meters_and_follows_the_world() {
        // Floor-facing: u = x, v = z, scaled.
        assert_eq!(project(Vec3::new(4.0, 9.0, 2.0), Vec3::Y, 2.0), [2.0, 1.0]);
        // Wall facing X: u = z, v = −y, so the texture stays upright.
        assert_eq!(project(Vec3::new(9.0, 1.0, 3.0), Vec3::X, 1.0), [3.0, -1.0]);
        // A 30° slope still counts as floor.
        let slope = Vec3::new(0.5, 0.866, 0.0);
        assert_eq!(project(Vec3::new(1.0, 5.0, 2.0), slope, 1.0), [1.0, 2.0]);
    }

    #[test]
    fn from_planes_rejects_open_sets() {
        let open = [Plane::through(Vec3::Y, Vec3::ZERO)];
        assert_eq!(Brush::from_planes(&open), None);
    }

    #[test]
    fn aabb_round_trips_through_physics() {
        let b = Aabb::from_center(Vec3::ONE, Vec3::splat(0.5));

        assert_eq!(Aabb::from_physics(b.to_physics()), b);
        assert!(b.overlaps(&b.shifted(Vec3::X * 0.5)));
        assert!(!b.overlaps(&b.shifted(Vec3::X)));
    }
}
