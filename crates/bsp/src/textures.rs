//! The texture lump: miptex headers. Only names and sizes are read; the
//! engine loads pixels from its own PNGs by name.
//!
//! ```text
//!   [count] count × offset (−1 = no header) … [name: 16][width][height][4 mip offsets]
//! ```

use crate::Texture;
use crate::error::{BspError, Result};
use crate::reader::{Cursor, name, u32_at};

const HEADER_BYTES: usize = 24;
const NAME_BYTES: usize = 16;

/// Every texture slot. A slot with offset −1 (no header at all) is an
/// empty, zero-size texture.
pub(crate) fn read(lump: &[u8]) -> Result<Vec<Texture>> {
    if lump.is_empty() {
        return Ok(Vec::new());
    }

    let mut cursor = Cursor::new(lump, "texture lump");
    let count = cursor.u32()?;
    (0..count)
        .map(|_| {
            let Ok(offset) = usize::try_from(cursor.i32()?) else {
                return Ok(Texture {
                    name: String::new(),
                    width: 0,
                    height: 0,
                });
            };

            let header = lump
                .get(offset..offset + HEADER_BYTES)
                .ok_or(BspError::Truncated("miptex header"))?;
            Ok(Texture {
                name: name(&header[..NAME_BYTES]),
                width: u32_at(header, NAME_BYTES),
                height: u32_at(header, NAME_BYTES + 4),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_slots_are_empty() {
        // One slot, offset −1.
        let mut lump = 1u32.to_le_bytes().to_vec();
        lump.extend((-1i32).to_le_bytes());

        let textures = read(&lump).unwrap();
        assert_eq!(textures[0].name, "");
        assert_eq!(textures[0].width, 0);
    }

    #[test]
    fn header_gives_name_and_size() {
        let mut lump = 1u32.to_le_bytes().to_vec();
        lump.extend(8i32.to_le_bytes());
        let name = *b"floor\0\0\0\0\0\0\0\0\0\0\0";
        lump.extend(name);
        lump.extend(64u32.to_le_bytes());
        lump.extend(32u32.to_le_bytes());

        let textures = read(&lump).unwrap();
        assert_eq!(
            textures[0],
            Texture {
                name: "floor".into(),
                width: 64,
                height: 32
            }
        );
    }

    #[test]
    fn truncated_header_is_an_error() {
        let mut lump = 1u32.to_le_bytes().to_vec();
        lump.extend(8i32.to_le_bytes());
        lump.extend(b"short");

        assert!(matches!(read(&lump), Err(BspError::Truncated(_))));
    }
}
