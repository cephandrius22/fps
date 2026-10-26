//! Faces and models: polygons rebuilt from the edge lists, wound
//! counter-clockwise from the visible side, with texel coordinates.
//!
//! ```text
//!   model ─► faces[first..first+count]
//!   face  ─► plane (normal, flipped by `side`)
//!         ─► surfedges[first..] ─► edges ─► vertices   (corners, in order)
//!         ─► texinfo ─► texture index, s/t axes        (texels)
//!         ─► light offset + styles ─► lighting lump    (lightmap)
//! ```
//!
//! Lightmap sizes must match ericw-tools' `light` exactly, or faces read
//! each other's luxels: texels are computed in double precision, stored
//! as f32, then sized by `level::LuxelGrid`.

use glam::{Vec2, Vec3};

use crate::error::{Result, index_error};
use crate::reader::{Lump, Lumps, Version, f32_at, i16_at, i32_at, u16_at, u32_at, vec3_at};
use level::{Face, Lightmap, LuxelGrid, Model};

const PLANE_BYTES: usize = 20;
const VERTEX_BYTES: usize = 12;
const TEXINFO_BYTES: usize = 40;
const MODEL_BYTES: usize = 64;
const SURFEDGE_BYTES: usize = 4;
const BSP29_FACE_BYTES: usize = 20;
const BSP2_FACE_BYTES: usize = 28;
/// Style slot value meaning "no lightmap".
const NO_STYLE: u8 = 255;

/// Every model with its faces. Brushes come from the BSPX lump later.
pub(crate) fn models(lumps: &Lumps) -> Result<Vec<Model>> {
    let geometry = Geometry::read(lumps)?;

    lumps
        .records::<MODEL_BYTES>(Lump::Models)?
        .iter()
        .map(|record| {
            let first = i64::from(i32_at(record, 56));
            let count = i64::from(i32_at(record, 60));
            if first < 0 || count < 0 || first + count > geometry.face_count() as i64 {
                return Err(index_error("model face range", first + count));
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

/// A face record's fields, the same for both versions once decoded.
struct FaceRecord {
    plane: usize,
    /// The face shows the plane's back.
    back: bool,
    first_edge: i64,
    edge_count: i64,
    texinfo: i64,
    /// Light style per lightmap; the first is the steady one.
    styles: [u8; 4],
    /// Byte offset into the lighting lump; negative for none.
    light_offset: i64,
}

/// The lumps faces index into.
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
    /// One byte per luxel; faces point into it.
    lighting: &'a [u8],
}

impl<'a> Geometry<'a> {
    fn read(lumps: &Lumps<'a>) -> Result<Self> {
        // BSP2 widens edge vertex indices from 16 to 32 bits.
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
            lighting: lumps.get(Lump::Lighting)?,
        })
    }

    fn face_bytes(&self) -> usize {
        match self.version {
            Version::Bsp29 => BSP29_FACE_BYTES,
            Version::Bsp2 => BSP2_FACE_BYTES,
        }
    }

    fn face_count(&self) -> usize {
        self.faces.len() / self.face_bytes()
    }

    /// Face `index` as a polygon, wound counter-clockwise from the front.
    fn face(&self, index: usize) -> Result<Face> {
        let face = self.record(index)?;

        let plane = self
            .planes
            .get(face.plane)
            .ok_or_else(|| index_error("plane", face.plane as i64))?;
        let normal = vec3_at(plane, 0);
        let normal = if face.back { -normal } else { normal };

        let info = usize::try_from(face.texinfo)
            .ok()
            .and_then(|i| self.texinfo.get(i))
            .ok_or_else(|| index_error("texinfo", face.texinfo))?;
        let texture = i32_at(info, 32);
        let texture =
            usize::try_from(texture).map_err(|_| index_error("texture", i64::from(texture)))?;

        let mut points = (face.first_edge..face.first_edge + face.edge_count)
            .map(|i| self.surfedge_start(i))
            .collect::<Result<Vec<_>>>()?;
        if newell(&points).dot(normal) < 0.0 {
            points.reverse();
        }

        let axes = [0, 16].map(|at| (vec3_at(info, at), f32_at(info, at + 12)));
        let texels: Vec<Vec2> = points.iter().map(|&p| texel(p, &axes)).collect();
        let lightmap = self.lightmap(&face, &texels)?;

        Ok(Face {
            texture,
            normal,
            points,
            texels,
            lightmap,
        })
    }

    /// The face's steady (style 0) lightmap, if it has one.
    fn lightmap(&self, face: &FaceRecord, texels: &[Vec2]) -> Result<Option<Lightmap>> {
        if face.styles[0] == NO_STYLE || face.light_offset < 0 {
            return Ok(None);
        }

        let grid = LuxelGrid::new(texels);
        let count = grid.count();
        let start = face.light_offset as usize;
        let luxels = self
            .lighting
            .get(start..start + count)
            .ok_or_else(|| index_error("light offset", face.light_offset))?;

        Ok(Some(Lightmap {
            width: grid.size[0],
            height: grid.size[1],
            luxels: luxels.to_vec(),
            coords: grid.coords(texels),
        }))
    }

    /// Decodes face record `index` for either version.
    fn record(&self, index: usize) -> Result<FaceRecord> {
        let size = self.face_bytes();
        let record = self
            .faces
            .get(index * size..(index + 1) * size)
            .ok_or_else(|| index_error("face", index as i64))?;

        Ok(match self.version {
            Version::Bsp29 => FaceRecord {
                plane: usize::from(u16_at(record, 0)),
                back: i16_at(record, 2) != 0,
                first_edge: i64::from(i32_at(record, 4)),
                edge_count: i64::from(i16_at(record, 8)),
                texinfo: i64::from(i16_at(record, 10)),
                styles: record[12..16].try_into().expect("4 bytes"),
                light_offset: i64::from(i32_at(record, 16)),
            },
            Version::Bsp2 => FaceRecord {
                plane: u32_at(record, 0) as usize,
                back: i32_at(record, 4) != 0,
                first_edge: i64::from(i32_at(record, 8)),
                edge_count: i64::from(i32_at(record, 12)),
                texinfo: i64::from(i32_at(record, 16)),
                styles: record[20..24].try_into().expect("4 bytes"),
                light_offset: i64::from(i32_at(record, 24)),
            },
        })
    }

    /// The vertex a surfedge starts at. Negative surfedges walk their
    /// edge backwards.
    fn surfedge_start(&self, index: i64) -> Result<Vec3> {
        let surfedge = usize::try_from(index)
            .ok()
            .and_then(|i| self.surfedges.get(i))
            .ok_or_else(|| index_error("surfedge", index))?;
        let edge = i32_at(surfedge, 0);

        let pair = self
            .edges
            .get(edge.unsigned_abs() as usize)
            .ok_or_else(|| index_error("edge", i64::from(edge)))?;
        let vertex = if edge >= 0 { pair[0] } else { pair[1] } as usize;

        let vertex = self
            .vertices
            .get(vertex)
            .ok_or_else(|| index_error("vertex", vertex as i64))?;
        Ok(vec3_at(vertex, 0))
    }
}

/// Texture coordinate of `p` along the (axis, offset) pairs, as ericw
/// computes it: in double precision, then rounded to f32.
fn texel(p: Vec3, axes: &[(Vec3, f32); 2]) -> Vec2 {
    let p = p.as_dvec3();
    let [s, t] = axes.map(|(axis, offset)| (p.dot(axis.as_dvec3()) + f64::from(offset)) as f32);
    Vec2::new(s, t)
}

/// Polygon normal by Newell's method: robust for any winding and for
/// collinear corners, which Quake faces often have. Not normalized.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texel_is_dot_plus_offset() {
        let axes = [(Vec3::X, 8.0), (Vec3::NEG_Y, 0.0)];
        assert_eq!(
            texel(Vec3::new(10.0, 4.0, 99.0), &axes),
            Vec2::new(18.0, -4.0)
        );
    }

    #[test]
    fn newell_follows_winding() {
        let square = [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y];

        assert!(newell(&square).normalize().abs_diff_eq(Vec3::Z, 1e-6));
        let mut reversed = square;
        reversed.reverse();
        assert!(newell(&reversed).normalize().abs_diff_eq(Vec3::NEG_Z, 1e-6));
    }

    #[test]
    fn newell_ignores_collinear_corners() {
        // A square with an extra corner midway along one edge.
        let square = [
            Vec3::ZERO,
            Vec3::X * 0.5,
            Vec3::X,
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::Y,
        ];
        assert!(newell(&square).normalize().abs_diff_eq(Vec3::Z, 1e-6));
    }
}
