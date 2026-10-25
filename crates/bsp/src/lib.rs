//! Quake BSP map reader: BSP29 and BSP2 files from ericw-tools, plus the
//! BSPX brush list. Returns plain data in Quake's own space (units,
//! Z up); converting to the engine's meters and Y-up is the caller's job.
//!
//! ```text
//!   .bsp ─► Lumps ─┬─ faces + texinfo + edges ─► Model.faces (polygons, texels)
//!                  ├─ lighting ───────────────► Face.lightmap (luxels, coords)
//!                  ├─ BSPX BRUSHLIST ─────────► Model.brushes (planes)
//!                  ├─ miptex headers ─────────► textures (name, size)
//!                  └─ entity text ────────────► entities (key/value)
//! ```
//!
//! Only what the engine draws and collides with is read. Visibility and
//! clip hulls are skipped; of the lightmaps, only style 0 (steady light)
//! in grayscale, at Quake's 16 texels per luxel.

mod brushlist;
mod entities;
mod error;
mod geometry;
mod reader;
mod textures;

use glam::{Vec2, Vec3};

pub use entities::Entity;
pub use error::BspError;
pub use reader::Version;

use error::{Result, index_error};
use reader::{Lump, Lumps};

/// A whole map.
#[derive(Clone, Debug)]
pub struct Bsp {
    /// File format.
    pub version: Version,
    /// Textures by index, as faces refer to them.
    pub textures: Vec<Texture>,
    /// Model 0 is the world; the rest belong to brush entities.
    pub models: Vec<Model>,
    /// Every entity, worldspawn first.
    pub entities: Vec<Entity>,
}

/// A texture's name and size. The pixels live elsewhere (the engine
/// loads `textures/<name>.png`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Texture {
    /// e.g. "floor"; empty for a missing slot.
    pub name: String,
    /// Width in texels; 0 when unknown.
    pub width: u32,
    /// Height in texels; 0 when unknown.
    pub height: u32,
}

/// The world or one brush entity's geometry.
#[derive(Clone, Debug)]
pub struct Model {
    /// Bounds, in Quake units.
    pub mins: Vec3,
    /// Bounds, in Quake units.
    pub maxs: Vec3,
    /// Visible polygons.
    pub faces: Vec<Face>,
    /// Convex solids for collision; empty without a BRUSHLIST.
    pub brushes: Vec<Brush>,
}

/// A convex visible polygon.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Index into [`Bsp::textures`].
    pub texture: usize,
    /// Unit normal on the visible side.
    pub normal: Vec3,
    /// Corners, counter-clockwise seen from the visible side.
    pub points: Vec<Vec3>,
    /// Texture coordinates per corner, in texels.
    pub texels: Vec<Vec2>,
    /// Baked light; None when the face is unlit (e.g. sky) or the map
    /// was not lit.
    pub lightmap: Option<Lightmap>,
}

/// A face's baked light: a small grid of luxels, one per 16 × 16 texels.
///
/// ```text
///   luxel (0,0)   (1,0)   …  (width−1, 0)
///     ·       ·       ·          corners map into this grid by `coords`;
///     ·       ·       ·          luxel centers sit at .5
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Lightmap {
    /// Luxels across.
    pub width: u32,
    /// Luxels down.
    pub height: u32,
    /// Brightness per luxel, row-major. Quake doubles it when lighting
    /// ("overbright"): 128 is full texture brightness, 255 nearly twice.
    pub luxels: Vec<u8>,
    /// Per corner (matching `Face::points`), position in luxels.
    pub coords: Vec<Vec2>,
}

/// A convex solid: the space behind every plane.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    /// What fills it.
    pub contents: Contents,
    /// Bounds, in Quake units.
    pub mins: Vec3,
    /// Bounds, in Quake units.
    pub maxs: Vec3,
    /// Outward planes: the six axial ones from the bounds, then the rest.
    pub planes: Vec<Plane>,
}

/// Points `p` with `normal · p = dist`; outside where greater.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// Unit normal.
    pub normal: Vec3,
    /// Distance from the origin along the normal.
    pub dist: f32,
}

/// What a brush is made of (Quake contents codes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contents {
    /// Nothing.
    Empty,
    /// Walls and floors.
    Solid,
    /// Swimmable.
    Water,
    /// Swimmable, hurts.
    Slime,
    /// Swimmable, hurts more.
    Lava,
    /// Sky: solid to movement.
    Sky,
    /// Invisible wall: solid, never drawn.
    Clip,
    /// A code this reader does not know.
    Other(i16),
}

impl Contents {
    fn from_code(code: i16) -> Self {
        match code {
            -1 => Contents::Empty,
            -2 => Contents::Solid,
            -3 => Contents::Water,
            -4 => Contents::Slime,
            -5 => Contents::Lava,
            -6 => Contents::Sky,
            -8 => Contents::Clip,
            other => Contents::Other(other),
        }
    }

    /// True for contents that block movement: solid, sky and clip.
    pub fn is_solid(self) -> bool {
        matches!(self, Contents::Solid | Contents::Sky | Contents::Clip)
    }
}

/// Reads a whole .bsp file.
pub fn parse(bytes: &[u8]) -> Result<Bsp> {
    let lumps = Lumps::read(bytes)?;

    let mut models = geometry::models(&lumps)?;
    for (index, brushes) in brushlist::read(&lumps)? {
        let model = models
            .get_mut(index)
            .ok_or_else(|| index_error("brush model", index as i64))?;
        model.brushes = brushes;
    }

    let text = String::from_utf8_lossy(lumps.get(Lump::Entities)?);
    Ok(Bsp {
        version: lumps.version,
        textures: textures::read(lumps.get(Lump::Textures)?)?,
        models,
        entities: entities::parse(&text)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contents_codes_map_and_solidity() {
        assert_eq!(Contents::from_code(-2), Contents::Solid);
        assert_eq!(Contents::from_code(-8), Contents::Clip);
        assert_eq!(Contents::from_code(-42), Contents::Other(-42));
        assert!(Contents::Sky.is_solid() && Contents::Clip.is_solid());
        assert!(!Contents::Water.is_solid());
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(parse(b"nope").is_err());
        assert!(parse(&[0; 200]).is_err());
    }
}
