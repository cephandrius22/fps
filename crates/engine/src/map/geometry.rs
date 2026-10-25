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

/// Textures that mark brushes for the compiler or the game, never drawn.
const TOOL_TEXTURES: [&str; 6] = ["trigger", "clip", "skip", "hint", "origin", "null"];
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
pub(super) fn surface(gfx: &mut Gfx, texture: &bsp::Texture) -> Surface {
    let name = texture.name.to_lowercase();
    if TOOL_TEXTURES.contains(&name.as_str()) {
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
    model: &bsp::Model,
    surfaces: &[Surface],
    lighting: Option<&Lighting>,
) -> Vec<Model> {
    // By texture; a BTreeMap keeps the draw order stable.
    let mut groups: BTreeMap<usize, MeshData> = BTreeMap::new();

    for (face_index, face) in model.faces.iter().enumerate() {
        let Some(&Surface::Drawn { texels, .. }) = surfaces.get(face.texture) else {
            continue;
        };
        let light_uvs = lighting.map(|l| l.uvs(index, face_index, face));

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
fn append_face(data: &mut MeshData, face: &bsp::Face, texels: Vec2, light_uvs: Option<&[Vec2]>) {
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
pub(super) fn colliders(model: &bsp::Model) -> Vec<Brush> {
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
