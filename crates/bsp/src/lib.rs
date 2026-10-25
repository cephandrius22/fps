//! Quake BSP map reader: BSP29 and BSP2 files from ericw-tools, plus the
//! BSPX brush list. Returns plain data in Quake's own space (units,
//! Z up); converting to the engine's meters and Y-up is the caller's job.
//!
//! ```text
//!   .bsp ─► Lumps ─┬─ faces + texinfo + edges ─► Model.faces (polygons, texels)
//!                  ├─ BSPX BRUSHLIST ─────────► Model.brushes (planes)
//!                  ├─ miptex headers ─────────► textures (name, size)
//!                  └─ entity text ────────────► entities (key/value)
//! ```
//!
//! Only what the engine draws and collides with is read. Visibility,
//! clip hulls and lightmaps are skipped for now.

mod brushlist;
mod entities;
mod error;
mod reader;

use glam::{Vec2, Vec3};

pub use entities::Entity;
pub use error::BspError;
pub use reader::Version;

use error::Result;
use reader::{Lump, Lumps, f32_at, i16_at, i32_at, u16_at, u32_at, vec3_at};

const PLANE_BYTES: usize = 20;
const VERTEX_BYTES: usize = 12;
const TEXINFO_BYTES: usize = 40;
const MODEL_BYTES: usize = 64;
const SURFEDGE_BYTES: usize = 4;
const MIPTEX_HEADER_BYTES: usize = 24;
const MIPTEX_NAME_BYTES: usize = 16;

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
    let geometry = Geometry::read(&lumps)?;

    let mut models = models(&lumps, &geometry)?;
    for (index, brushes) in brushlist::read(&lumps)? {
        let model = models
            .get_mut(index)
            .ok_or_else(|| out_of_range("brush model", index))?;
        model.brushes = brushes;
    }

    let text = String::from_utf8_lossy(lumps.get(Lump::Entities)?);
    Ok(Bsp {
        version: lumps.version,
        textures: textures(lumps.get(Lump::Textures)?)?,
        models,
        entities: entities::parse(&text)?,
    })
}

/// Shared geometry the faces index into.
struct Geometry<'a> {
    version: Version,
    planes: &'a [[u8; PLANE_BYTES]],
    vertices: &'a [[u8; VERTEX_BYTES]],
    texinfo: &'a [[u8; TEXINFO_BYTES]],
    surfedges: &'a [[u8; SURFEDGE_BYTES]],
    /// Vertex index pairs.
    edges: Vec<[u32; 2]>,
    /// Face records, still raw: their size depends on the version.
    faces: &'a [u8],
}

impl<'a> Geometry<'a> {
    fn read(lumps: &Lumps<'a>) -> Result<Self> {
        let edges = match lumps.version {
            Version::Bsp29 => lumps
                .records::<4>(Lump::Edges)?
                .iter()
                .map(|e| [u32::from(u16_at(e, 0)), u32::from(u16_at(e, 2))])
                .collect(),
            Version::Bsp2 => lumps
                .records::<8>(Lump::Edges)?
                .iter()
                .map(|e| [u32_at(e, 0), u32_at(e, 4)])
                .collect(),
        };

        Ok(Self {
            version: lumps.version,
            planes: lumps.records(Lump::Planes)?,
            vertices: lumps.records(Lump::Vertices)?,
            texinfo: lumps.records(Lump::TexInfo)?,
            surfedges: lumps.records(Lump::SurfEdges)?,
            edges,
            faces: lumps.get(Lump::Faces)?,
        })
    }

    fn face_count(&self) -> usize {
        self.faces.len() / self.face_bytes()
    }

    fn face_bytes(&self) -> usize {
        match self.version {
            Version::Bsp29 => 20,
            Version::Bsp2 => 28,
        }
    }

    /// Face `index` as a polygon, wound counter-clockwise from the front.
    fn face(&self, index: usize) -> Result<Face> {
        let size = self.face_bytes();
        let record = self
            .faces
            .get(index * size..(index + 1) * size)
            .ok_or_else(|| out_of_range("face", index))?;

        // (plane, side, first edge, edge count, texinfo)
        let (plane, side, first, count, info) = match self.version {
            Version::Bsp29 => (
                usize::from(u16_at(record, 0)),
                i16_at(record, 2) != 0,
                i32_at(record, 4) as i64,
                i16_at(record, 8) as i64,
                i16_at(record, 10) as i64,
            ),
            Version::Bsp2 => (
                u32_at(record, 0) as usize,
                i32_at(record, 4) != 0,
                i32_at(record, 8) as i64,
                i32_at(record, 12) as i64,
                i32_at(record, 16) as i64,
            ),
        };

        let plane = self
            .planes
            .get(plane)
            .ok_or_else(|| out_of_range("plane", plane))?;
        let normal = vec3_at(plane, 0);
        let normal = if side { -normal } else { normal };

        let info = usize::try_from(info).map_err(|_| bad("texinfo", info))?;
        let info = self
            .texinfo
            .get(info)
            .ok_or_else(|| out_of_range("texinfo", info))?;
        let axes = [0, 16].map(|at| (vec3_at(info, at), f32_at(info, at + 12)));
        let texture = i32_at(info, 32);
        let texture = usize::try_from(texture).map_err(|_| bad("texture", i64::from(texture)))?;

        let mut points = Vec::new();
        for i in first..first + count {
            points.push(self.surfedge_start(i)?);
        }
        // Wind counter-clockwise around the visible-side normal.
        if newell(&points).dot(normal) < 0.0 {
            points.reverse();
        }

        let texels = points
            .iter()
            .map(|&p| Vec2::new(p.dot(axes[0].0) + axes[0].1, p.dot(axes[1].0) + axes[1].1))
            .collect();

        Ok(Face {
            texture,
            normal,
            points,
            texels,
        })
    }

    /// The vertex a surfedge starts at. Negative surfedges walk their
    /// edge backwards.
    fn surfedge_start(&self, index: i64) -> Result<Vec3> {
        let surfedge = usize::try_from(index)
            .ok()
            .and_then(|i| self.surfedges.get(i))
            .ok_or_else(|| bad("surfedge", index))?;
        let edge = i32_at(surfedge, 0);

        let pair = self
            .edges
            .get(edge.unsigned_abs() as usize)
            .ok_or_else(|| bad("edge", i64::from(edge)))?;
        let vertex = if edge >= 0 { pair[0] } else { pair[1] } as usize;

        let vertex = self
            .vertices
            .get(vertex)
            .ok_or_else(|| out_of_range("vertex", vertex))?;
        Ok(vec3_at(vertex, 0))
    }
}

/// Every model with its faces (brushes are filled in later).
fn models(lumps: &Lumps, geometry: &Geometry) -> Result<Vec<Model>> {
    lumps
        .records::<MODEL_BYTES>(Lump::Models)?
        .iter()
        .map(|record| {
            let first = i32_at(record, 56) as i64;
            let count = i32_at(record, 60) as i64;
            if first < 0 || count < 0 || (first + count) as usize > geometry.face_count() {
                return Err(bad("model face range", first + count));
            }

            let faces = (first..first + count)
                .map(|i| geometry.face(i as usize))
                .collect::<Result<_>>()?;
            Ok(Model {
                mins: vec3_at(record, 0),
                maxs: vec3_at(record, 12),
                faces,
                brushes: Vec::new(),
            })
        })
        .collect()
}

/// Names and sizes from the miptex headers. A slot with offset -1 (no
/// header at all) becomes an empty, zero-size texture.
fn textures(lump: &[u8]) -> Result<Vec<Texture>> {
    if lump.is_empty() {
        return Ok(Vec::new());
    }

    let mut cursor = reader::Cursor::new(lump, "texture lump");
    let count = cursor.u32()?;
    (0..count)
        .map(|_| {
            let offset = cursor.i32()?;
            let Ok(offset) = usize::try_from(offset) else {
                return Ok(Texture {
                    name: String::new(),
                    width: 0,
                    height: 0,
                });
            };

            let header = lump
                .get(offset..offset + MIPTEX_HEADER_BYTES)
                .ok_or(BspError::Truncated("miptex header"))?;
            Ok(Texture {
                name: reader::name(&header[..MIPTEX_NAME_BYTES]),
                width: u32_at(header, MIPTEX_NAME_BYTES),
                height: u32_at(header, MIPTEX_NAME_BYTES + 4),
            })
        })
        .collect()
}

/// Polygon normal by Newell's method: robust for any winding and for
/// nearly collinear corners. Not normalized.
fn newell(points: &[Vec3]) -> Vec3 {
    let mut normal = Vec3::ZERO;
    for (i, &a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        normal += Vec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    normal
}

fn out_of_range(what: &'static str, index: usize) -> BspError {
    bad(what, index as i64)
}

fn bad(what: &'static str, index: i64) -> BspError {
    BspError::Index { what, index }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newell_follows_winding() {
        let square = [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y];

        assert!(newell(&square).normalize().abs_diff_eq(Vec3::Z, 1e-6));
        let mut reversed = square;
        reversed.reverse();
        assert!(newell(&reversed).normalize().abs_diff_eq(Vec3::NEG_Z, 1e-6));
    }

    #[test]
    fn contents_codes_map_and_solidity() {
        assert_eq!(Contents::from_code(-2), Contents::Solid);
        assert_eq!(Contents::from_code(-8), Contents::Clip);
        assert_eq!(Contents::from_code(-42), Contents::Other(-42));
        assert!(Contents::Sky.is_solid() && Contents::Clip.is_solid());
        assert!(!Contents::Water.is_solid());
    }

    #[test]
    fn missing_texture_slots_are_empty() {
        // One slot, offset -1.
        let mut lump = 1u32.to_le_bytes().to_vec();
        lump.extend((-1i32).to_le_bytes());

        let textures = textures(&lump).unwrap();
        assert_eq!(textures[0].name, "");
        assert_eq!(textures[0].width, 0);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(parse(b"nope").is_err());
        assert!(parse(&[0; 200]).is_err());
    }
}
