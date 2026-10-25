//! BSPX "BRUSHLIST": the map's convex brushes, written by ericw-tools'
//! `qbsp -wrbrushes`. Standard Quake BSPs keep only clip trees for three
//! fixed hull sizes; brushes let any box collide.
//!
//! ```text
//!   BSPX directory, 4-byte aligned after the last lump:
//!     "BSPX" [count] count × (name[24], offset, length)
//!   BRUSHLIST lump, repeated per model:
//!     ver=1 modelnum brushcount planecount
//!     brushcount × (mins[3] maxs[3] contents:i16 planes:u16
//!                   planes × (normal[3] dist))
//! ```
//!
//! Axial planes are not stored: the brush's bounds stand in for them.

use glam::Vec3;

use crate::error::{BspError, Result, index_error};
use crate::reader::{Cursor, Lumps, name};
use crate::{Brush, Contents, Plane};

const MAGIC: &[u8; 4] = b"BSPX";
const LUMP_NAME_BYTES: usize = 24;
const BRUSHLIST: &str = "BRUSHLIST";
const BRUSHLIST_VERSION: i32 = 1;
/// Model header: ver, modelnum, brush count, plane count.
const MODEL_HEADER_BYTES: usize = 16;

/// Brushes per model index; models without brushes are absent. Empty
/// when the file has no BRUSHLIST (compiled without `-wrbrushes`).
pub(crate) fn read(lumps: &Lumps) -> Result<Vec<(usize, Vec<Brush>)>> {
    let Some(lump) = find(lumps)? else {
        return Ok(Vec::new());
    };

    let mut cursor = Cursor::new(lump, BRUSHLIST);
    let mut models = Vec::new();

    while cursor.remaining() >= MODEL_HEADER_BYTES {
        let version = cursor.i32()?;
        if version != BRUSHLIST_VERSION {
            return Err(BspError::BrushListVersion(version));
        }
        let model = cursor.i32()?;
        let count = cursor.i32()?;
        let _planes = cursor.i32()?;

        let model =
            usize::try_from(model).map_err(|_| index_error("brush model", i64::from(model)))?;
        let count =
            usize::try_from(count).map_err(|_| index_error("brush count", i64::from(count)))?;
        let brushes = (0..count)
            .map(|_| brush(&mut cursor))
            .collect::<Result<_>>()?;
        models.push((model, brushes));
    }

    Ok(models)
}

/// The BRUSHLIST lump's bytes, if the BSPX directory lists one.
fn find<'a>(lumps: &Lumps<'a>) -> Result<Option<&'a [u8]>> {
    let data = lumps.data();
    let start = lumps.end().next_multiple_of(4);
    if data.get(start..start + MAGIC.len()) != Some(MAGIC.as_slice()) {
        return Ok(None);
    }

    let mut cursor = Cursor::new(&data[start + MAGIC.len()..], "BSPX directory");
    let count = cursor.u32()?;
    for _ in 0..count {
        let lump_name = name(cursor.take(LUMP_NAME_BYTES)?);
        let offset = cursor.u32()? as usize;
        let len = cursor.u32()? as usize;
        if lump_name != BRUSHLIST {
            continue;
        }

        let bytes = offset
            .checked_add(len)
            .and_then(|end| data.get(offset..end))
            .ok_or(BspError::LumpBounds(BRUSHLIST))?;
        return Ok(Some(bytes));
    }

    Ok(None)
}

fn brush(cursor: &mut Cursor) -> Result<Brush> {
    let mins = cursor.vec3()?;
    let maxs = cursor.vec3()?;
    let contents = Contents::from_code(cursor.i16()?);
    let count = cursor.u16()?;

    // Bounds first: the six axial planes the file leaves out.
    let mut planes = vec![
        Plane {
            normal: Vec3::X,
            dist: maxs.x,
        },
        Plane {
            normal: Vec3::NEG_X,
            dist: -mins.x,
        },
        Plane {
            normal: Vec3::Y,
            dist: maxs.y,
        },
        Plane {
            normal: Vec3::NEG_Y,
            dist: -mins.y,
        },
        Plane {
            normal: Vec3::Z,
            dist: maxs.z,
        },
        Plane {
            normal: Vec3::NEG_Z,
            dist: -mins.z,
        },
    ];
    for _ in 0..count {
        let normal = cursor.vec3()?;
        let dist = cursor.f32()?;
        planes.push(Plane { normal, dist });
    }

    Ok(Brush {
        contents,
        mins,
        maxs,
        planes,
    })
}
