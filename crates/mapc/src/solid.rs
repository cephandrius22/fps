//! Map brushes as solids: their planes, the polygon of each face, and
//! what fills them.
//!
//! ```text
//!   brush faces ─► planes (from 3 points) ─► each plane's base square
//!               clipped by every other plane ─► one polygon per side
//! ```
//!
//! A plane whose polygon is clipped away bounds nothing (it lies outside
//! the others) and is dropped.

use glam::DVec3;
use level::Contents;

use crate::plane::Plane;
use crate::winding::{BASE_EXTENT, Winding};

/// The fewest sides that close a volume: a tetrahedron.
const MIN_SIDES: usize = 4;
/// Texture name prefixes that set a brush's contents.
const LIQUID_PREFIX: &str = "*";
const SLIME_PREFIX: &str = "*slime";
const LAVA_PREFIX: &str = "*lava";
const SKY_PREFIX: &str = "sky";
const CLIP: &str = "clip";
/// Tool textures that say nothing about contents.
const NEUTRAL: [&str; 2] = ["skip", "hint"];

/// Why a brush makes no solid.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Invalid {
    /// A face's three points are on one line.
    #[error("a face's points are collinear")]
    Collinear,
    /// The planes enclose nothing.
    #[error("its planes enclose no volume")]
    NoVolume,
}

/// A convex brush, built.
#[derive(Debug)]
pub(crate) struct Solid<'a> {
    /// What fills it.
    pub contents: Contents,
    /// One per bounding plane.
    pub sides: Vec<Side<'a>>,
    /// Bounds of its corners.
    pub mins: DVec3,
    /// Bounds of its corners.
    pub maxs: DVec3,
}

/// One bounding plane and its polygon.
#[derive(Debug)]
pub(crate) struct Side<'a> {
    /// Facing out of the brush.
    pub plane: Plane,
    /// The face's polygon, counter-clockwise seen from outside.
    pub winding: Winding,
    /// The .map face it came from (texture and mapping).
    pub face: &'a mapfile::Face,
}

impl<'a> Solid<'a> {
    /// Builds `brush`'s polygons. Duplicate planes count once.
    pub(crate) fn build(brush: &'a mapfile::Brush) -> Result<Self, Invalid> {
        let mut planes: Vec<(Plane, &mapfile::Face)> = Vec::new();
        for face in &brush.faces {
            let plane = Plane::from_points(face.points).ok_or(Invalid::Collinear)?;
            if !planes.iter().any(|(p, _)| p.same(&plane)) {
                planes.push((plane, face));
            }
        }

        let sides: Vec<Side> = planes
            .iter()
            .enumerate()
            .filter_map(|(i, &(plane, face))| {
                let winding = polygon(i, &planes)?;
                Some(Side {
                    plane,
                    winding,
                    face,
                })
            })
            .collect();
        if sides.len() < MIN_SIDES {
            return Err(Invalid::NoVolume);
        }

        // Open on some side: a polygon still reaches the base square's
        // edge.
        let (mins, maxs) = sides.iter().map(|s| s.winding.bounds()).fold(
            (DVec3::INFINITY, DVec3::NEG_INFINITY),
            |(lo, hi), (a, b)| (lo.min(a), hi.max(b)),
        );
        if (maxs - mins).max_element() >= BASE_EXTENT {
            return Err(Invalid::NoVolume);
        }
        Ok(Self {
            contents: contents(&brush.faces),
            sides,
            mins,
            maxs,
        })
    }

    /// In the level's form: contents, bounds and planes.
    pub(crate) fn to_level(&self) -> level::Brush {
        level::Brush {
            contents: self.contents,
            mins: self.mins.as_vec3(),
            maxs: self.maxs.as_vec3(),
            planes: self.sides.iter().map(|s| s.plane.to_level()).collect(),
        }
    }
}

/// Plane `index`'s base square, clipped by every other plane.
fn polygon(index: usize, planes: &[(Plane, &mapfile::Face)]) -> Option<Winding> {
    let mut winding = Winding::base(&planes[index].0);
    for (i, (plane, _)) in planes.iter().enumerate() {
        if i != index {
            winding = winding.clip(plane)?;
        }
    }
    Some(winding.snapped())
}

/// What fills a brush, from its first face whose texture says: "*lava"
/// is lava, "*slime" slime, any other "*" water, "sky…" sky and "clip"
/// an invisible wall. Anything else is solid.
fn contents(faces: &[mapfile::Face]) -> Contents {
    let Some(name) = faces
        .iter()
        .map(|f| f.texture.to_lowercase())
        .find(|name| !NEUTRAL.contains(&name.as_str()))
    else {
        return Contents::Solid;
    };

    if name.starts_with(SLIME_PREFIX) {
        return Contents::Slime;
    }
    if name.starts_with(LAVA_PREFIX) {
        return Contents::Lava;
    }
    if name.starts_with(LIQUID_PREFIX) {
        return Contents::Water;
    }
    if name.starts_with(SKY_PREFIX) {
        return Contents::Sky;
    }
    if name == CLIP {
        return Contents::Clip;
    }
    Contents::Solid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::cuboid;

    #[test]
    fn a_box_has_six_square_sides() {
        let brush = cuboid(DVec3::ZERO, DVec3::new(64.0, 32.0, 16.0), "floor");
        let solid = Solid::build(&brush).unwrap();

        assert_eq!(solid.sides.len(), 6);
        assert_eq!(
            (solid.mins, solid.maxs),
            (DVec3::ZERO, DVec3::new(64.0, 32.0, 16.0))
        );
        for side in &solid.sides {
            assert_eq!(side.winding.points().len(), 4);
            assert!(
                side.winding
                    .newell()
                    .normalize()
                    .abs_diff_eq(side.plane.normal, 1e-12)
            );
        }

        let top = solid
            .sides
            .iter()
            .find(|s| s.plane.normal == DVec3::Z)
            .unwrap();
        assert_eq!(top.winding.area(), 64.0 * 32.0);
        assert_eq!(solid.to_level().planes.len(), 6);
    }

    #[test]
    fn redundant_and_duplicate_planes_are_dropped() {
        let mut brush = cuboid(DVec3::ZERO, DVec3::splat(64.0), "floor");
        let outside = cuboid(DVec3::splat(-8.0), DVec3::splat(72.0), "floor").faces[0].clone();
        brush.faces.push(outside);
        brush.faces.push(brush.faces[1].clone());

        assert_eq!(Solid::build(&brush).unwrap().sides.len(), 6);
    }

    #[test]
    fn broken_brushes_are_invalid() {
        let mut flat = cuboid(DVec3::ZERO, DVec3::splat(64.0), "floor");
        flat.faces.truncate(3);
        assert_eq!(Solid::build(&flat).unwrap_err(), Invalid::NoVolume);

        // ±X and ±Y only: a square tube, open at both ends.
        let mut tube = cuboid(DVec3::ZERO, DVec3::splat(64.0), "floor");
        tube.faces.truncate(4);
        assert_eq!(Solid::build(&tube).unwrap_err(), Invalid::NoVolume);

        let mut collinear = cuboid(DVec3::ZERO, DVec3::splat(64.0), "floor");
        collinear.faces[0].points = [DVec3::ZERO, DVec3::X, DVec3::X * 2.0];
        assert_eq!(Solid::build(&collinear).unwrap_err(), Invalid::Collinear);
    }

    #[test]
    fn contents_come_from_the_first_telling_texture() {
        let cases = [
            ("*water1", Contents::Water),
            ("*SLIME0", Contents::Slime),
            ("*lava1", Contents::Lava),
            ("sky4", Contents::Sky),
            ("clip", Contents::Clip),
            ("skip", Contents::Solid),
            ("floor", Contents::Solid),
        ];

        for (texture, expected) in cases {
            let brush = cuboid(DVec3::ZERO, DVec3::ONE, texture);
            assert_eq!(contents(&brush.faces), expected, "{texture}");
        }

        let mut hinted = cuboid(DVec3::ZERO, DVec3::ONE, "*water1");
        hinted.faces[0].texture = "hint".into();
        assert_eq!(
            contents(&hinted.faces),
            Contents::Water,
            "hint says nothing"
        );
    }
}
