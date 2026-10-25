//! Quake BSP map reader: BSP29 and BSP2 files from ericw-tools, plus the
//! BSPX brush list, read into the shared `level` data model (map space:
//! Quake units, Z up). Converting to the engine's meters and Y-up is the
//! engine's job, the same for every map format.
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
//!
//! Modules (start with `parse` below, then `geometry.rs`):
//!
//! ```text
//!   lib.rs         parse() → level::Level, version()
//!   reader.rs      lump directory, little-endian records, Cursor
//!   geometry.rs    faces (polygons, texels, lightmaps) and models
//!   brushlist.rs   BSPX BRUSHLIST → brushes
//!   textures.rs    miptex headers → names and sizes
//!   entities.rs    entity text → level::Entity
//!   error.rs       BspError
//! ```

mod brushlist;
mod entities;
mod error;
mod geometry;
mod reader;
mod textures;

use level::Level;

pub use error::BspError;
pub use reader::Version;

use error::{Result, index_error};
use reader::{Lump, Lumps};

/// The file's format version.
pub fn version(bytes: &[u8]) -> Result<Version> {
    Ok(Lumps::read(bytes)?.version)
}

/// Reads a whole .bsp file into level data (map space, as the file has
/// it).
pub fn parse(bytes: &[u8]) -> Result<Level> {
    let lumps = Lumps::read(bytes)?;

    let mut models = geometry::models(&lumps)?;
    for (index, brushes) in brushlist::read(&lumps)? {
        let model = models
            .get_mut(index)
            .ok_or_else(|| index_error("brush model", index as i64))?;
        model.brushes = brushes;
    }

    let text = String::from_utf8_lossy(lumps.get(Lump::Entities)?);
    Ok(Level {
        textures: textures::read(lumps.get(Lump::Textures)?)?,
        models,
        entities: entities::parse(&text)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(parse(b"nope").is_err());
        assert!(parse(&[0; 200]).is_err());
    }
}
