//! Map geometry: textures by name, one mesh per texture per model, and
//! the solid brushes of each model.
//!
//! Textures are `textures/<name>.png`, sized by the map's miptex headers
//! (which qbsp filled from the same PNGs). Tool textures (trigger, clip,
//! skip, …) mark brushes for the compiler or the game and are never
//! drawn.

use std::collections::BTreeMap;

use glam::Vec2;

use super::lighting::Lighting;
use super::space::{to_engine_direction, to_engine_plane, to_engine_point};
use crate::collision::Brush;
use crate::entity::Model;
use crate::gfx::{Gfx, MeshData, TextureHandle, Vertex};

/// Texture size assumed when neither the map nor a PNG gives one.
const DEFAULT_TEXELS: f32 = 64.0;
const WHITE: [f32; 4] = [1.0; 4];

/// How faces with one texture are drawn.
#[derive(Clone, Copy, Debug)]
pub(super) enum Surface {
    /// A tool texture: not drawn.
    Hidden,
    /// Drawn with `texture` (None: untextured), sized `texels`.
    Drawn {
        texture: Option<TextureHandle>,
        texels: Vec2,
    },
}

/// How a texture's faces are drawn: loads `textures/<name>.png`.
pub(super) fn surface(gfx: &mut Gfx, texture: &level::Texture) -> Surface {
    let name = texture.name.to_lowercase();
    if level::is_tool_texture(&name) {
        return Surface::Hidden;
    }

    let size = Vec2::new(texture.width as f32, texture.height as f32);
    let texels = if size.min_element() > 0.0 {
        size
    } else {
        Vec2::splat(DEFAULT_TEXELS)
    };
    let loaded = gfx
        .load_texture(&format!("textures/{name}.png"))
        .inspect_err(|err| {
            log::warn!(
                "map texture {name:?} drawn untextured: {}",
                base::chain(err)
            )
        })
        .ok();

    Surface::Drawn {
        texture: loaded,
        texels,
    }
}

/// One mesh per texture for the model's visible faces; lit by the atlas
/// when the map has baked light.
pub(super) fn meshes(
    gfx: &mut Gfx,
    index: usize,
    model: &level::Model,
    surfaces: &[Surface],
    lighting: Option<&Lighting>,
) -> Vec<Model> {
    // By texture; a BTreeMap keeps the draw order stable.
    let mut groups: BTreeMap<usize, MeshData> = BTreeMap::new();

    for (face_index, face) in model.faces.iter().enumerate() {
        let Some(&Surface::Drawn { texels, .. }) = surfaces.get(face.texture) else {
            continue;
        };
        let light_uvs = lighting.map(|l| l.layout.uvs(index, face_index, face));

        let group = groups.entry(face.texture).or_default();
        append_face(group, face, texels, light_uvs.as_deref());
    }

    groups
        .into_iter()
        .map(|(texture_index, data)| {
            let texture = match surfaces[texture_index] {
                Surface::Drawn { texture, .. } => texture,
                Surface::Hidden => None,
            };

            Model {
                mesh: gfx.create_mesh(&data),
                texture,
                lightmap: lighting.map(|l| l.texture),
            }
        })
        .collect()
}

/// Adds a convex face as a triangle fan. Collinear corners (common in
/// Quake faces) only add zero-area triangles. `light_uvs`, if any, are
/// the corners' lightmap atlas coordinates.
fn append_face(data: &mut MeshData, face: &level::Face, texels: Vec2, light_uvs: Option<&[Vec2]>) {
    let base = data.vertices.len() as u32;
    let normal = to_engine_direction(face.normal).to_array();
    let light_uv = |i: usize| light_uvs.map_or(Vec2::ZERO, |uvs| uvs[i]);

    data.vertices.extend(
        face.points
            .iter()
            .zip(&face.texels)
            .enumerate()
            .map(|(i, (&p, &t))| Vertex {
                pos: to_engine_point(p).to_array(),
                normal,
                uv: (t / texels).to_array(),
                lightmap_uv: light_uv(i).to_array(),
                color: WHITE,
            }),
    );

    let count = face.points.len() as u32;
    data.indices
        .extend((1..count - 1).flat_map(|i| [base, base + i, base + i + 1]));
}

/// Solid brushes of a model, in engine space. Liquids are not solid;
/// a degenerate brush is logged and skipped.
pub(super) fn colliders(model: &level::Model) -> Vec<Brush> {
    model
        .brushes
        .iter()
        .filter(|b| b.contents.is_solid())
        .filter_map(|b| {
            let planes: Vec<_> = b.planes.iter().map(|p| to_engine_plane(*p)).collect();
            let brush = Brush::from_planes(&planes);
            if brush.is_none() {
                log::warn!("skipped a degenerate map brush at {:?}", b.mins);
            }
            brush
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    const EPS: f32 = 1e-5;

    /// A 64-unit square on the Quake floor (z = 0), facing up, wound
    /// counter-clockwise seen from above; texels equal Quake x, y.
    fn floor_face() -> level::Face {
        let points = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(64.0, 0.0, 0.0),
            Vec3::new(64.0, 64.0, 0.0),
            Vec3::new(0.0, 64.0, 0.0),
        ];
        level::Face {
            texture: 0,
            normal: Vec3::Z,
            texels: points.iter().map(|p| Vec2::new(p.x, p.y)).collect(),
            points,
            lightmap: None,
        }
    }

    fn quake_brush(contents: level::Contents, mins: Vec3, maxs: Vec3) -> level::Brush {
        let axial = [
            (Vec3::X, maxs.x),
            (Vec3::NEG_X, -mins.x),
            (Vec3::Y, maxs.y),
            (Vec3::NEG_Y, -mins.y),
            (Vec3::Z, maxs.z),
            (Vec3::NEG_Z, -mins.z),
        ];
        level::Brush {
            contents,
            mins,
            maxs,
            planes: axial
                .map(|(normal, dist)| level::Plane { normal, dist })
                .to_vec(),
        }
    }

    #[test]
    fn face_becomes_a_fan_in_engine_space() {
        let mut data = MeshData::default();
        append_face(&mut data, &floor_face(), Vec2::splat(64.0), None);

        assert_eq!(data.vertices.len(), 4);
        assert_eq!(data.indices, [0, 1, 2, 0, 2, 3]);
        // Quake (64, 64, 0) is engine (2, 0, −2); Quake up is engine +Y.
        assert_eq!(data.vertices[2].pos, [2.0, 0.0, -2.0]);
        assert_eq!(data.vertices[0].normal, [0.0, 1.0, 0.0]);

        // Still counter-clockwise around the normal after the swap.
        let pos = |i: u32| Vec3::from(data.vertices[i as usize].pos);
        let winding = (pos(1) - pos(0)).cross(pos(2) - pos(0));
        assert!(winding.dot(Vec3::Y) > 0.0);
    }

    #[test]
    fn texels_scale_by_texture_size_and_light_uvs_pass_through() {
        let mut data = MeshData::default();
        let light = [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y].map(|v| v * 0.5);
        append_face(&mut data, &floor_face(), Vec2::splat(32.0), Some(&light));

        // 64 texels on a 32-texel texture: the texture repeats twice.
        assert_eq!(data.vertices[2].uv, [2.0, 2.0]);
        assert_eq!(data.vertices[2].lightmap_uv, [0.5, 0.5]);

        let mut unlit = MeshData::default();
        append_face(&mut unlit, &floor_face(), Vec2::splat(32.0), None);
        assert!(unlit.vertices.iter().all(|v| v.lightmap_uv == [0.0, 0.0]));
    }

    #[test]
    fn only_solid_brushes_collide_and_they_move_to_engine_space() {
        let model = level::Model {
            mins: Vec3::ZERO,
            maxs: Vec3::splat(64.0),
            faces: Vec::new(),
            brushes: vec![
                quake_brush(
                    level::Contents::Solid,
                    Vec3::ZERO,
                    Vec3::new(32.0, 32.0, 64.0),
                ),
                quake_brush(level::Contents::Water, Vec3::ZERO, Vec3::splat(64.0)),
                quake_brush(level::Contents::Clip, Vec3::ZERO, Vec3::splat(32.0)),
            ],
        };

        let solid = colliders(&model);
        assert_eq!(solid.len(), 2, "solid and clip, not water");

        // Quake x 0..32, y 0..32, z 0..64 → engine x 0..1, y 0..2, z −1..0.
        let bounds = solid[0].bounds();
        assert!((bounds.min - Vec3::new(0.0, 0.0, -1.0)).length() < EPS);
        assert!((bounds.max - Vec3::new(1.0, 2.0, 0.0)).length() < EPS);
    }
}
