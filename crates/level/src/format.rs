//! The .lvl file format: the engine's own compiled level, written by
//! `mapc`. Little-endian; every list is a u32 count, then its items.
//!
//! ```text
//!   "RLVL" version:u32
//!   textures  [name:str width:u32 height:u32]
//!   models    [mins:vec3 maxs:vec3
//!              faces   [texture:u32 normal:vec3 points:[vec3] texels:[vec2]
//!                       lightmap:u8 (0 none, 1 then width:u32 height:u32
//!                                    luxels:[u8] coords:[vec2])]
//!              brushes [contents:i16 mins:vec3 maxs:vec3 planes:[normal:vec3 dist:f32]]]
//!   entities  [fields [key:str value:str]]
//!
//!   str = u32 byte length + UTF-8; vec3 / vec2 = f32s
//! ```
//!
//! Bump `VERSION` whenever the layout changes; readers reject other
//! versions rather than misread them.

use glam::{Vec2, Vec3};

use crate::error::{LevelError, Result};
use crate::model::{Brush, Contents, Entity, Face, Level, Lightmap, Model, Plane, Texture};

const MAGIC: &[u8; 4] = b"RLVL";
/// The layout this build writes and reads.
pub(crate) const VERSION: u32 = 1;
const NO_LIGHTMAP: u8 = 0;
const HAS_LIGHTMAP: u8 = 1;

/// Encodes a level as .lvl bytes.
pub fn write(level: &Level) -> Vec<u8> {
    let mut w = Writer::default();
    w.bytes(MAGIC);
    w.u32(VERSION);

    w.list(&level.textures, |w, t| {
        w.str(&t.name);
        w.u32(t.width);
        w.u32(t.height);
    });
    w.list(&level.models, write_model);
    w.list(&level.entities, |w, e| {
        let fields: Vec<_> = e.fields().collect();
        w.list(&fields, |w, (k, v)| {
            w.str(k);
            w.str(v);
        });
    });

    w.out
}

/// Decodes .lvl bytes.
pub fn read(bytes: &[u8]) -> Result<Level> {
    let mut r = Reader { bytes, pos: 0 };
    if r.take(MAGIC.len(), "header").ok() != Some(MAGIC.as_slice()) {
        return Err(LevelError::Magic);
    }
    let found = r.u32("header")?;
    if found != VERSION {
        return Err(LevelError::Version {
            found,
            supported: VERSION,
        });
    }

    let textures = r.list("textures", |r| {
        Ok(Texture {
            name: r.str("texture name")?,
            width: r.u32("texture")?,
            height: r.u32("texture")?,
        })
    })?;
    let models = r.list("models", read_model)?;
    let entities = r.list("entities", |r| {
        let fields = r.list("entity", |r| {
            Ok((r.str("entity key")?, r.str("entity value")?))
        })?;
        Ok(Entity::new(fields))
    })?;

    let rest = bytes.len() - r.pos;
    if rest > 0 {
        return Err(LevelError::Trailing(rest));
    }
    Ok(Level {
        textures,
        models,
        entities,
    })
}

fn write_model(w: &mut Writer, model: &Model) {
    w.vec3(model.mins);
    w.vec3(model.maxs);

    w.list(&model.faces, |w, face| {
        w.u32(face.texture as u32);
        w.vec3(face.normal);
        w.list(&face.points, |w, p| w.vec3(*p));
        w.list(&face.texels, |w, t| w.vec2(*t));
        match &face.lightmap {
            None => w.u8(NO_LIGHTMAP),
            Some(lightmap) => {
                w.u8(HAS_LIGHTMAP);
                w.u32(lightmap.width);
                w.u32(lightmap.height);
                w.list(&lightmap.luxels, |w, l| w.u8(*l));
                w.list(&lightmap.coords, |w, c| w.vec2(*c));
            }
        }
    });

    w.list(&model.brushes, |w, brush| {
        w.i16(brush.contents.code());
        w.vec3(brush.mins);
        w.vec3(brush.maxs);
        w.list(&brush.planes, |w, plane| {
            w.vec3(plane.normal);
            w.f32(plane.dist);
        });
    });
}

fn read_model(r: &mut Reader) -> Result<Model> {
    let mins = r.vec3("model")?;
    let maxs = r.vec3("model")?;

    let faces = r.list("faces", |r| {
        let texture = r.u32("face")? as usize;
        let normal = r.vec3("face")?;
        let points = r.list("face points", |r| r.vec3("face point"))?;
        let texels = r.list("face texels", |r| r.vec2("face texel"))?;
        let lightmap = match r.u8("face")? {
            NO_LIGHTMAP => None,
            HAS_LIGHTMAP => Some(Lightmap {
                width: r.u32("lightmap")?,
                height: r.u32("lightmap")?,
                luxels: r.list("lightmap luxels", |r| r.u8("luxel"))?,
                coords: r.list("lightmap coords", |r| r.vec2("lightmap coord"))?,
            }),
            _ => return Err(LevelError::Corrupt("lightmap flag")),
        };
        Ok(Face {
            texture,
            normal,
            points,
            texels,
            lightmap,
        })
    })?;

    let brushes = r.list("brushes", |r| {
        Ok(Brush {
            contents: Contents::from_code(r.i16("brush")?),
            mins: r.vec3("brush")?,
            maxs: r.vec3("brush")?,
            planes: r.list("brush planes", |r| {
                Ok(Plane {
                    normal: r.vec3("plane")?,
                    dist: r.f32("plane")?,
                })
            })?,
        })
    })?;

    Ok(Model {
        mins,
        maxs,
        faces,
        brushes,
    })
}

/// Appends little-endian values.
#[derive(Default)]
struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn bytes(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
    }

    fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    fn i16(&mut self, v: i16) {
        self.bytes(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.bytes(&v.to_le_bytes());
    }

    fn vec2(&mut self, v: Vec2) {
        self.f32(v.x);
        self.f32(v.y);
    }

    fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }

    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.bytes(s.as_bytes());
    }

    /// Count, then each item.
    fn list<T>(&mut self, items: &[T], mut each: impl FnMut(&mut Self, &T)) {
        self.u32(items.len() as u32);
        for item in items {
            each(self, item);
        }
    }
}

/// Reads little-endian values; running out of bytes is a
/// `Truncated(what)` error.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize, what: &'static str) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or(LevelError::Truncated(what))?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(LevelError::Truncated(what))?;
        self.pos = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self, what: &'static str) -> Result<[u8; N]> {
        Ok(self.take(N, what)?.try_into().expect("took N bytes"))
    }

    fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.array::<1>(what)?[0])
    }

    fn i16(&mut self, what: &'static str) -> Result<i16> {
        Ok(i16::from_le_bytes(self.array(what)?))
    }

    fn u32(&mut self, what: &'static str) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array(what)?))
    }

    fn f32(&mut self, what: &'static str) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array(what)?))
    }

    fn vec2(&mut self, what: &'static str) -> Result<Vec2> {
        Ok(Vec2::new(self.f32(what)?, self.f32(what)?))
    }

    fn vec3(&mut self, what: &'static str) -> Result<Vec3> {
        Ok(Vec3::new(self.f32(what)?, self.f32(what)?, self.f32(what)?))
    }

    fn str(&mut self, what: &'static str) -> Result<String> {
        let len = self.u32(what)? as usize;
        let bytes = self.take(len, what)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| LevelError::Utf8(what))
    }

    /// Count, then each item. Items are read one by one rather than
    /// preallocated, so a corrupt count runs out of bytes instead of
    /// allocating gigabytes.
    fn list<T>(
        &mut self,
        what: &'static str,
        mut each: impl FnMut(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        let count = self.u32(what)?;
        (0..count).map(|_| each(self)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A level using every part of the format.
    fn sample() -> Level {
        let face = Face {
            texture: 1,
            normal: Vec3::Z,
            points: vec![Vec3::ZERO, Vec3::X * 64.0, Vec3::Y * 64.0],
            texels: vec![Vec2::ZERO, Vec2::X * 64.0, Vec2::Y * 64.0],
            lightmap: Some(Lightmap {
                width: 2,
                height: 1,
                luxels: vec![10, 200],
                coords: vec![Vec2::splat(0.5); 3],
            }),
        };
        let unlit = Face {
            lightmap: None,
            ..face.clone()
        };
        let brush = Brush {
            contents: Contents::Clip,
            mins: Vec3::ZERO,
            maxs: Vec3::ONE,
            planes: vec![Plane {
                normal: Vec3::X,
                dist: 1.0,
            }],
        };

        Level {
            textures: vec![
                Texture {
                    name: "floor".into(),
                    width: 64,
                    height: 64,
                },
                Texture {
                    name: "crate".into(),
                    width: 0,
                    height: 0,
                },
            ],
            models: vec![Model {
                mins: Vec3::ZERO,
                maxs: Vec3::splat(64.0),
                faces: vec![face, unlit],
                brushes: vec![brush],
            }],
            entities: vec![Entity::new(vec![
                ("classname".into(), "worldspawn".into()),
                ("message".into(), "Tëst".into()),
            ])],
        }
    }

    #[test]
    fn levels_round_trip_exactly() {
        let level = sample();
        assert_eq!(read(&write(&level)).unwrap(), level);
        assert_eq!(read(&write(&Level::default())).unwrap(), Level::default());
    }

    #[test]
    fn foreign_files_and_versions_are_rejected() {
        assert!(matches!(read(b"BSP2...."), Err(LevelError::Magic)));
        assert!(matches!(read(b"RL"), Err(LevelError::Magic)));

        let mut future = write(&sample());
        future[4..8].copy_from_slice(&(VERSION + 1).to_le_bytes());
        assert!(matches!(read(&future), Err(LevelError::Version { .. })));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let bytes = write(&sample());
        for len in 8..bytes.len() {
            let err = read(&bytes[..len]).unwrap_err();
            assert!(matches!(err, LevelError::Truncated(_)), "at {len}: {err:?}");
        }
    }

    #[test]
    fn trailing_bytes_and_bad_text_are_errors() {
        let mut long = write(&sample());
        long.push(0);
        assert!(matches!(read(&long), Err(LevelError::Trailing(1))));

        let mut level = Level::default();
        level.textures.push(Texture {
            name: "ab".into(),
            width: 0,
            height: 0,
        });
        let mut bytes = write(&level);
        // The name's bytes sit after magic, version, count and length.
        bytes[16] = 0xff;
        assert!(matches!(read(&bytes), Err(LevelError::Utf8(_))));
    }

    #[test]
    fn an_unknown_lightmap_flag_is_corrupt() {
        // One model with one unlit face, nothing else: the file ends
        // with the face's flag, the brush count and the entity count.
        let mut model = sample().models.remove(0);
        model.faces.truncate(1);
        model.faces[0].lightmap = None;
        model.brushes.clear();
        let level = Level {
            models: vec![model],
            ..Level::default()
        };
        let mut bytes = write(&level);
        let flag = bytes.len() - 1 - 4 - 4;
        assert_eq!(bytes[flag], NO_LIGHTMAP);
        bytes[flag] = 7;

        assert!(matches!(read(&bytes), Err(LevelError::Corrupt(_))));
    }

    #[test]
    fn a_huge_count_runs_out_of_bytes_instead_of_allocating() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(VERSION.to_le_bytes());
        bytes.extend(u32::MAX.to_le_bytes());

        assert!(matches!(read(&bytes), Err(LevelError::Truncated(_))));
    }
}
