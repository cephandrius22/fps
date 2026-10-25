//! Procedural meshes for the sandbox: a unit cube. Level geometry comes
//! from brushes (`Brush::mesh`) instead.

use engine::math::Vec3;
use engine::{MeshData, Vertex};

const WHITE: [f32; 4] = [1.0; 4];
const HALF: f32 = 0.5;

/// Cube face color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    /// A distinct color per face; reads well untextured.
    PerFace,
    /// White, so a texture shows its true colors.
    White,
}

/// (normal, u axis, v axis, color). u × v = normal, so corners listed
/// (-u-v, +u-v, +u+v, -u+v) wind counter-clockwise seen from outside.
const FACES: [(Vec3, Vec3, Vec3, [f32; 4]); 6] = [
    (Vec3::X, Vec3::NEG_Z, Vec3::Y, [0.85, 0.25, 0.20, 1.0]),
    (Vec3::NEG_X, Vec3::Z, Vec3::Y, [0.20, 0.70, 0.30, 1.0]),
    (Vec3::Y, Vec3::X, Vec3::NEG_Z, [0.90, 0.80, 0.30, 1.0]),
    (Vec3::NEG_Y, Vec3::X, Vec3::Z, [0.40, 0.25, 0.60, 1.0]),
    (Vec3::Z, Vec3::X, Vec3::Y, [0.25, 0.45, 0.85, 1.0]),
    (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y, [0.90, 0.55, 0.20, 1.0]),
];

const CORNERS: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];

/// Unit cube centered on the origin. 24 vertices so each face keeps its
/// own normal, color and full 0..1 UVs.
pub fn cube(tint: Tint) -> MeshData {
    let mut data = MeshData::default();

    for (normal, u, v, face_color) in FACES {
        let base = data.vertices.len() as u32;
        let color = match tint {
            Tint::PerFace => face_color,
            Tint::White => WHITE,
        };

        for (cu, cv) in CORNERS {
            let pos = (normal + u * cu + v * cv) * HALF;

            data.vertices.push(Vertex {
                pos: pos.to_array(),
                normal: normal.to_array(),
                uv: [(cu + 1.0) * HALF, (1.0 - cv) * HALF],
                color,
                ..Default::default()
            });
        }

        data.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    data
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    /// Back-face culling hides anything wound the wrong way, so every
    /// triangle must wind counter-clockwise around its stored normal.
    fn assert_winds_with_normals(mesh: &MeshData) {
        let pos = |i: u32| Vec3::from(mesh.vertices[i as usize].pos);

        for tri in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(pos);
            let winding_normal = (b - a).cross(c - a).normalize();
            let normal = Vec3::from(mesh.vertices[tri[0] as usize].normal);

            assert!((winding_normal - normal).length() < EPS, "tri {tri:?}");
        }
    }

    #[test]
    fn cube_has_four_vertices_and_two_triangles_per_face() {
        let cube = cube(Tint::PerFace);

        assert_eq!(cube.vertices.len(), 6 * 4);
        assert_eq!(cube.indices.len(), 6 * 6);
        assert!(
            cube.indices
                .iter()
                .all(|&i| (i as usize) < cube.vertices.len())
        );
    }

    #[test]
    fn cube_is_unit_and_centered() {
        for v in cube(Tint::PerFace).vertices {
            assert!(
                v.pos.iter().all(|c| (c.abs() - HALF).abs() < EPS),
                "{:?}",
                v.pos
            );
        }
    }

    #[test]
    fn cube_faces_point_outward() {
        let cube = cube(Tint::PerFace);
        assert_winds_with_normals(&cube);

        for v in &cube.vertices {
            assert!(Vec3::from(v.pos).dot(Vec3::from(v.normal)) > 0.0);
        }
    }

    #[test]
    fn white_tint_leaves_texture_colors_alone() {
        assert!(cube(Tint::White).vertices.iter().all(|v| v.color == WHITE));
        assert!(
            cube(Tint::PerFace)
                .vertices
                .iter()
                .any(|v| v.color != WHITE)
        );
    }
}
