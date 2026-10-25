//! Test fixtures: brushes and maps built in code.

use glam::{DVec2, DVec3};
use mapfile::{Axes, Brush, Face};

/// An axis-aligned box brush, every face `texture` with default
/// mapping. Faces in order +X, −X, +Y, −Y, +Z, −Z.
pub(crate) fn cuboid(min: DVec3, max: DVec3, texture: &str) -> Brush {
    // Per face: outward normal, then two in-plane directions a, b with
    // a × b = normal. Points [c + a, c, c + b] run clockwise seen from
    // outside, as .map files write them.
    let sides = [
        (DVec3::X, DVec3::Y, DVec3::Z),
        (DVec3::NEG_X, DVec3::Z, DVec3::Y),
        (DVec3::Y, DVec3::Z, DVec3::X),
        (DVec3::NEG_Y, DVec3::X, DVec3::Z),
        (DVec3::Z, DVec3::X, DVec3::Y),
        (DVec3::NEG_Z, DVec3::Y, DVec3::X),
    ];

    let faces = sides
        .map(|(normal, a, b)| {
            let corner = DVec3::select(normal.cmpgt(DVec3::ZERO), max, min);
            Face {
                points: [corner + a, corner, corner + b],
                texture: texture.into(),
                axes: Axes::Standard,
                offset: DVec2::ZERO,
                rotation: 0.0,
                scale: DVec2::ONE,
            }
        })
        .to_vec();

    Brush { faces, line: 0 }
}
