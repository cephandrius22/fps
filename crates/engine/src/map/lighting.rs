//! A map's baked light: every face lightmap packed into one atlas
//! texture, so a whole map draws with a single lightmap.
//!
//! ```text
//!   face lightmaps ─► atlas.rs (shelf packing) ─► one lightmap texture
//!                                   │
//!   face corners' luxel coords ─────┴──► atlas UVs per vertex
//! ```
//!
//! ericw-tools leaves out lightmaps of faces no light reached; those
//! sample one reserved black luxel, as Quake draws them black.

use std::collections::HashMap;

use glam::Vec2;

use crate::atlas::{self, Block};
use crate::gfx::{Gfx, TextureHandle};

/// Luxel value for faces no light reached.
const DARK: u8 = 0;

/// The map's baked light: every face lightmap packed in one texture,
/// plus one black luxel for faces that received no light.
pub(super) struct Lighting {
    pub texture: TextureHandle,
    /// Atlas size in luxels.
    size: Vec2,
    /// Atlas corner of each lit face, by (model, face) index.
    corners: HashMap<(usize, usize), Vec2>,
    /// Atlas UV of the black luxel's center.
    dark: Vec2,
}

impl Lighting {
    /// Atlas UVs for a face's corners: its own lightmap, or the black
    /// luxel for a face light never reached.
    pub fn uvs(&self, model: usize, face_index: usize, face: &bsp::Face) -> Vec<Vec2> {
        let lit = face
            .lightmap
            .as_ref()
            .zip(self.corners.get(&(model, face_index)));
        let Some((lightmap, corner)) = lit else {
            return vec![self.dark; face.points.len()];
        };

        lightmap
            .coords
            .iter()
            .map(|&c| (*corner + c) / self.size)
            .collect()
    }
}

/// Packs every face lightmap into one atlas texture. None for an unlit
/// map, or (logged) if the atlas cannot be uploaded.
pub(super) fn lighting(gfx: &mut Gfx, map: &bsp::Bsp, path: &str) -> Option<Lighting> {
    let lit: Vec<((usize, usize), &bsp::Lightmap)> = map
        .models
        .iter()
        .enumerate()
        .flat_map(|(m, model)| {
            model
                .faces
                .iter()
                .enumerate()
                .filter_map(move |(f, face)| Some(((m, f), face.lightmap.as_ref()?)))
        })
        .collect();

    if lit.is_empty() {
        return None;
    }

    // The faces' lightmaps, then the black luxel.
    let mut blocks: Vec<Block> = lit
        .iter()
        .map(|(_, l)| Block {
            width: l.width,
            height: l.height,
            luxels: &l.luxels,
        })
        .collect();
    blocks.push(Block {
        width: 1,
        height: 1,
        luxels: &[DARK],
    });
    let packed = atlas::pack(&blocks)?;

    let size = Vec2::new(packed.image.width() as f32, packed.image.height() as f32);
    let texture = gfx
        .create_lightmap(&packed.image)
        .inspect_err(|err| log::warn!("{path}: lightmaps dropped: {}", base::chain(err)))
        .ok()?;
    let corners = lit
        .iter()
        .zip(&packed.positions)
        .map(|((key, _), at)| (*key, at.as_vec2()))
        .collect();

    let dark_corner = packed.positions.last().expect("the black luxel was packed");

    Some(Lighting {
        texture,
        size,
        corners,
        dark: (dark_corner.as_vec2() + 0.5) / size,
    })
}
