//! Low-level reading: the header's lump directory, fixed-size records,
//! and a cursor for variable-length data. Everything is little-endian.
//!
//! ```text
//!   [version: 4][15 × (offset, length)] [lumps …] [BSPX directory …]
//! ```

use glam::Vec3;

use crate::error::{BspError, Result};

const LUMP_COUNT: usize = 15;
const HEADER_BYTES: usize = 4 + LUMP_COUNT * 8;
const BSP29: u32 = 29;
/// "BSP2" read as a little-endian u32.
const BSP2: u32 = u32::from_le_bytes(*b"BSP2");

/// File format. BSP2 widens face and edge fields for bigger maps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    /// Original Quake format.
    Bsp29,
    /// Extended format (32-bit indices).
    Bsp2,
}

/// Lumps this reader uses, by directory index.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Lump {
    Entities = 0,
    Planes = 1,
    Textures = 2,
    Vertices = 3,
    TexInfo = 6,
    Faces = 7,
    Lighting = 8,
    Edges = 12,
    SurfEdges = 13,
    Models = 14,
}

impl Lump {
    fn name(self) -> &'static str {
        match self {
            Lump::Entities => "entities",
            Lump::Planes => "planes",
            Lump::Textures => "textures",
            Lump::Vertices => "vertices",
            Lump::TexInfo => "texinfo",
            Lump::Faces => "faces",
            Lump::Lighting => "lighting",
            Lump::Edges => "edges",
            Lump::SurfEdges => "surfedges",
            Lump::Models => "models",
        }
    }
}

/// The file and its lump directory.
pub(crate) struct Lumps<'a> {
    pub version: Version,
    data: &'a [u8],
    /// (offset, length) per lump.
    directory: [(usize, usize); LUMP_COUNT],
}

impl<'a> Lumps<'a> {
    pub fn read(data: &'a [u8]) -> Result<Self> {
        if data.len() < HEADER_BYTES {
            return Err(BspError::Truncated("header"));
        }

        let version = match u32::from_le_bytes(data[..4].try_into().expect("4 bytes")) {
            BSP29 => Version::Bsp29,
            BSP2 => Version::Bsp2,
            other => return Err(BspError::Version(other)),
        };

        let mut directory = [(0, 0); LUMP_COUNT];
        for (i, entry) in directory.iter_mut().enumerate() {
            let at = 4 + i * 8;
            *entry = (u32_at(data, at) as usize, u32_at(data, at + 4) as usize);
        }

        Ok(Self {
            version,
            data,
            directory,
        })
    }

    /// The lump's bytes.
    pub fn get(&self, lump: Lump) -> Result<&'a [u8]> {
        let (offset, len) = self.directory[lump as usize];
        offset
            .checked_add(len)
            .and_then(|end| self.data.get(offset..end))
            .ok_or(BspError::LumpBounds(lump.name()))
    }

    /// The lump split into `N`-byte records.
    pub fn records<const N: usize>(&self, lump: Lump) -> Result<&'a [[u8; N]]> {
        let bytes = self.get(lump)?;
        let (records, rest) = bytes.as_chunks::<N>();
        if !rest.is_empty() {
            return Err(BspError::LumpSize {
                lump: lump.name(),
                len: bytes.len(),
                record: N,
            });
        }

        Ok(records)
    }

    /// Byte just past the last lump, where a BSPX directory may follow.
    pub fn end(&self) -> usize {
        self.directory
            .iter()
            .map(|&(offset, len)| offset.saturating_add(len))
            .max()
            .unwrap_or(HEADER_BYTES)
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }
}

// Readers for fields at known offsets inside a record. Callers pass
// offsets that fit the record size, so the slices never go out of range.

pub(crate) fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().expect("2 bytes"))
}

pub(crate) fn i16_at(bytes: &[u8], at: usize) -> i16 {
    i16::from_le_bytes(bytes[at..at + 2].try_into().expect("2 bytes"))
}

pub(crate) fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

pub(crate) fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

pub(crate) fn f32_at(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

pub(crate) fn vec3_at(bytes: &[u8], at: usize) -> Vec3 {
    Vec3::new(
        f32_at(bytes, at),
        f32_at(bytes, at + 4),
        f32_at(bytes, at + 8),
    )
}

/// Sequential reader for variable-length data; running out of bytes is
/// a `Truncated(what)` error.
pub(crate) struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
    what: &'static str,
}

impl<'a> Cursor<'a> {
    pub fn new(bytes: &'a [u8], what: &'static str) -> Self {
        Self {
            bytes,
            pos: 0,
            what,
        }
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    pub fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.pos + len;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(BspError::Truncated(self.what))?;
        self.pos = end;
        Ok(slice)
    }

    pub fn i16(&mut self) -> Result<i16> {
        self.take(2).map(|b| i16_at(b, 0))
    }

    pub fn u16(&mut self) -> Result<u16> {
        self.take(2).map(|b| u16_at(b, 0))
    }

    pub fn i32(&mut self) -> Result<i32> {
        self.take(4).map(|b| i32_at(b, 0))
    }

    pub fn u32(&mut self) -> Result<u32> {
        self.take(4).map(|b| u32_at(b, 0))
    }

    pub fn f32(&mut self) -> Result<f32> {
        self.take(4).map(|b| f32_at(b, 0))
    }

    pub fn vec3(&mut self) -> Result<Vec3> {
        self.take(12).map(|b| vec3_at(b, 0))
    }
}

/// NUL-padded fixed-size name → string.
pub(crate) fn name(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Header with the given version and every lump empty at `at`.
    fn header(version: u32, at: u32) -> Vec<u8> {
        let mut data = version.to_le_bytes().to_vec();
        for _ in 0..LUMP_COUNT {
            data.extend(at.to_le_bytes());
            data.extend(0u32.to_le_bytes());
        }
        data
    }

    #[test]
    fn reads_both_versions_and_rejects_others() {
        let end = HEADER_BYTES as u32;
        assert_eq!(
            Lumps::read(&header(29, end)).unwrap().version,
            Version::Bsp29
        );
        assert_eq!(
            Lumps::read(&header(BSP2, end)).unwrap().version,
            Version::Bsp2
        );
        assert!(matches!(
            Lumps::read(&header(30, end)),
            Err(BspError::Version(30))
        ));
        assert!(matches!(Lumps::read(&[0; 8]), Err(BspError::Truncated(_))));
    }

    #[test]
    fn lump_outside_the_file_is_an_error() {
        let data = header(29, 9999);
        let lumps = Lumps::read(&data).unwrap();

        assert!(matches!(
            lumps.get(Lump::Faces),
            Err(BspError::LumpBounds("faces"))
        ));
    }

    #[test]
    fn records_must_divide_evenly() {
        let mut data = header(29, HEADER_BYTES as u32);
        // Planes lump: 5 bytes at the end of the header.
        data[4 + 8 + 4..4 + 8 + 8].copy_from_slice(&5u32.to_le_bytes());
        data.extend([0; 5]);
        let lumps = Lumps::read(&data).unwrap();

        let err = lumps.records::<20>(Lump::Planes).unwrap_err();
        assert!(matches!(
            err,
            BspError::LumpSize {
                len: 5,
                record: 20,
                ..
            }
        ));
    }

    #[test]
    fn cursor_reads_in_order_and_reports_truncation() {
        let bytes = [1, 0, 0xff, 0xff, 0, 0, 0x80, 0x3f];
        let mut cursor = Cursor::new(&bytes, "test");

        assert_eq!(cursor.u16().unwrap(), 1);
        assert_eq!(cursor.i16().unwrap(), -1);
        assert_eq!(cursor.f32().unwrap(), 1.0);
        assert!(matches!(cursor.u32(), Err(BspError::Truncated("test"))));
    }

    #[test]
    fn names_stop_at_the_first_nul() {
        assert_eq!(name(b"floor\0\0junk"), "floor");
        assert_eq!(name(b"full"), "full");
    }
}
