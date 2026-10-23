//! Unit cube, one color per face. 24 vertices so each face keeps its own
//! normal and color.

use engine::math::Vec3;
use engine::{MeshData, Vertex};

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
const HALF: f32 = 0.5;

pub fn mesh() -> MeshData {
    let mut data = MeshData::default();

    for (normal, u, v, color) in FACES {
        let base = data.vertices.len() as u32;

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
