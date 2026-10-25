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
use base::Image;

use crate::gfx::{Gfx, TextureHandle};

/// Luxel value for faces no light reached.
const DARK: u8 = 0;

/// The map's baked light: the uploaded atlas and where each face is in it.
pub(super) struct Lighting {
    pub texture: TextureHandle,
    pub layout: Layout,
}

/// Where each face's lightmap sits in the atlas. Pure data, so it can be
/// built and checked without a GPU.
#[derive(Debug, PartialEq)]
pub(super) struct Layout {
    /// Atlas size in luxels.
    size: Vec2,
    /// Atlas corner of each lit face, by (model, face) index.
    corners: HashMap<(usize, usize), Vec2>,
    /// Atlas UV of the black luxel's center.
    dark: Vec2,
}

impl Layout {
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

/// Packs and uploads the map's lightmaps. None for an unlit map, or
/// (logged) if the atlas cannot be uploaded.
pub(super) fn lighting(gfx: &mut Gfx, map: &bsp::Bsp, path: &str) -> Option<Lighting> {
    let (image, layout) = pack(map)?;
    let texture = gfx
        .create_lightmap(&image)
        .inspect_err(|err| log::warn!("{path}: lightmaps dropped: {}", base::chain(err)))
        .ok()?;

    Some(Lighting { texture, layout })
}

/// Every face lightmap, then one black luxel, packed into one atlas
/// image. None when no face has a lightmap (the map was never lit).
fn pack(map: &bsp::Bsp) -> Option<(Image, Layout)> {
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
    let corners = lit
        .iter()
        .zip(&packed.positions)
        .map(|((key, _), at)| (*key, at.as_vec2()))
        .collect();
    let dark_corner = packed.positions.last().expect("the black luxel was packed");

    let layout = Layout {
        size,
        corners,
        dark: (dark_corner.as_vec2() + 0.5) / size,
    };
    Some((packed.image, layout))
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    const EPS: f32 = 1e-5;

    /// A triangle face, lit by a `width × height` lightmap of `value`
    /// (None: unlit).
    fn face(lightmap: Option<(u32, u32, u8)>) -> bsp::Face {
        let points = vec![Vec3::ZERO, Vec3::X, Vec3::Y];
        let lightmap = lightmap.map(|(width, height, value)| bsp::Lightmap {
            width,
            height,
            luxels: vec![value; (width * height) as usize],
            // Corners at the first, last-across and last-down luxel centers.
            coords: vec![
                Vec2::splat(0.5),
                Vec2::new(width as f32 - 0.5, 0.5),
                Vec2::new(0.5, height as f32 - 0.5),
            ],
        });

        bsp::Face {
            texture: 0,
            normal: Vec3::Z,
            texels: vec![Vec2::ZERO; points.len()],
            points,
            lightmap,
        }
    }

    fn map(faces: Vec<bsp::Face>) -> bsp::Bsp {
        bsp::Bsp {
            version: bsp::Version::Bsp29,
            textures: Vec::new(),
            models: vec![bsp::Model {
                mins: Vec3::ZERO,
                maxs: Vec3::ONE,
                faces,
                brushes: Vec::new(),
            }],
            entities: Vec::new(),
        }
    }

    #[test]
    fn unlit_map_has_no_lighting() {
        assert!(pack(&map(vec![face(None), face(None)])).is_none());
    }

    #[test]
    fn lit_face_uvs_land_on_its_own_luxels() {
        let lit = map(vec![face(Some((4, 2, 200))), face(Some((3, 3, 50)))]);
        let (image, layout) = pack(&lit).unwrap();
        let size = Vec2::new(image.width() as f32, image.height() as f32);

        for (index, face) in lit.models[0].faces.iter().enumerate() {
            let value = face.lightmap.as_ref().unwrap().luxels[0];
            for uv in layout.uvs(0, index, face) {
                assert!(
                    uv.cmpge(Vec2::ZERO).all() && uv.cmple(Vec2::ONE).all(),
                    "{uv}"
                );

                // The luxel under each corner holds this face's light.
                let texel = (uv * size).floor().as_uvec2();
                assert_eq!(image.pixel(texel.x, texel.y).unwrap()[0], value);
            }
        }
    }

    #[test]
    fn faces_without_light_sample_the_black_luxel() {
        let mixed = map(vec![face(Some((2, 2, 255))), face(None)]);
        let (image, layout) = pack(&mixed).unwrap();
        let size = Vec2::new(image.width() as f32, image.height() as f32);

        let dark = layout.uvs(0, 1, &mixed.models[0].faces[1]);
        assert_eq!(dark.len(), 3);
        assert!(dark.iter().all(|&uv| (uv - dark[0]).length() < EPS));

        let texel = (dark[0] * size).floor().as_uvec2();
        assert_eq!(
            image.pixel(texel.x, texel.y).unwrap(),
            [DARK, DARK, DARK, 255]
        );
    }
}
