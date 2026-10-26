//! Bounced light: lit surfaces light others. Each face's luxels are
//! grouped into patches that re-emit what they received, dimmed by
//! their texture's albedo; every sample then gathers from every patch
//! it can see. Each bounce re-emits only the previous bounce's light.
//!
//! ```text
//!   ●  light           patch P (on the floor, lit directly)
//!    ╲                   emits  albedo × its mean light
//!     ╲   wall sample ◄── receives  P · area · cosθ_wall · cosθ_P
//!      ▼                             ─────────────────────────────
//!   ▔▔▔P▔▔▔                                    π · distance²
//! ```
//!
//! Patches are a few luxels square: coarse enough to stay fast, fine
//! enough for soft indirect light. A patch on the same plane as a
//! sample sends it nothing (both cosines meet at 90°), so where a wall
//! meets a floor the wall's lowest luxels get little of the floor's
//! light: a soft darkening into the corner.

use std::f64::consts::PI;

use glam::DVec3;

use crate::face::Samples;
use crate::occlusion::Occluders;

/// Patch side, in luxels (64 map units).
const PATCH_LUXELS: u32 = 4;
/// Patches emitting less than this are skipped: they would add nothing
/// a luxel can show.
const MIN_EMISSION: f64 = 0.5;
/// Contributions below this are not traced, light units.
const MIN_CONTRIBUTION: f64 = 0.05;

/// A small piece of a face, re-emitting light.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Patch {
    /// Mean of its samples, so just off the surface.
    center: DVec3,
    normal: DVec3,
    /// Square map units.
    area: f64,
    /// Light leaving it: albedo × mean light received × scale.
    emission: f64,
}

/// What bounces add to direct light.
pub(crate) struct Bounce<'a> {
    pub occluders: &'a Occluders,
    /// Passes; 0 disables bouncing.
    pub passes: u32,
    /// Multiplies every patch's emission.
    pub scale: f64,
}

/// `light` (per face, per sample) plus every pass of bounced light.
/// `faces` and `albedos` match `light` face for face; a face without
/// samples has no light and neither emits nor receives.
pub(crate) fn add(
    faces: &[Option<Samples>],
    albedos: &[f64],
    light: Vec<Vec<f64>>,
    bounce: &Bounce,
) -> Vec<Vec<f64>> {
    let mut total = light.clone();
    let mut last = light;

    for _ in 0..bounce.passes {
        let patches = patches(faces, albedos, &last, bounce.scale);
        if patches.is_empty() {
            break;
        }

        last = faces
            .iter()
            .map(|f| {
                f.as_ref()
                    .map(|s| gather(s, &patches, bounce.occluders))
                    .unwrap_or_default()
            })
            .collect();
        for (sum, extra) in total.iter_mut().zip(&last) {
            sum.iter_mut().zip(extra).for_each(|(t, e)| *t += e);
        }
    }

    total
}

/// Every face's patches worth emitting.
fn patches(
    faces: &[Option<Samples>],
    albedos: &[f64],
    light: &[Vec<f64>],
    scale: f64,
) -> Vec<Patch> {
    faces
        .iter()
        .zip(albedos)
        .zip(light)
        .filter_map(|((face, &albedo), light)| Some((face.as_ref()?, albedo, light)))
        .flat_map(|(face, albedo, light)| face_patches(face, light, albedo * scale))
        .filter(|p| p.emission >= MIN_EMISSION)
        .collect()
}

/// One face cut into PATCH_LUXELS-square blocks of luxels.
fn face_patches(face: &Samples, light: &[f64], reflect: f64) -> Vec<Patch> {
    let [width, height] = face.grid.size;
    let area_per_luxel = face.area / face.points.len() as f64;

    let blocks = (0..height).step_by(PATCH_LUXELS as usize).flat_map(|y| {
        (0..width)
            .step_by(PATCH_LUXELS as usize)
            .map(move |x| (x, y))
    });
    blocks
        .map(|(x0, y0)| {
            let luxels: Vec<usize> = (y0..(y0 + PATCH_LUXELS).min(height))
                .flat_map(|y| {
                    (x0..(x0 + PATCH_LUXELS).min(width)).map(move |x| (y * width + x) as usize)
                })
                .collect();
            let count = luxels.len() as f64;

            Patch {
                center: luxels.iter().map(|&i| face.points[i]).sum::<DVec3>() / count,
                normal: face.normal,
                area: area_per_luxel * count,
                emission: reflect * luxels.iter().map(|&i| light[i]).sum::<f64>() / count,
            }
        })
        .collect()
}

/// Light reaching each of `face`'s samples from `patches`.
fn gather(face: &Samples, patches: &[Patch], occluders: &Occluders) -> Vec<f64> {
    face.points
        .iter()
        .map(|&point| {
            patches
                .iter()
                .map(|patch| (patch, received(point, face.normal, patch)))
                .filter(|&(patch, amount)| {
                    amount >= MIN_CONTRIBUTION && !occluders.blocked(point, patch.center)
                })
                .map(|(_, amount)| amount)
                .sum()
        })
        .collect()
}

/// Light from `patch` arriving at `point` facing `normal`, before
/// shadows. Distances below the patch's own size are clamped, so a
/// patch beside a sample cannot flood it.
fn received(point: DVec3, normal: DVec3, patch: &Patch) -> f64 {
    let to_patch = patch.center - point;
    let dist_sq = to_patch.length_squared();
    let dir = to_patch / dist_sq.sqrt();

    let cos_here = normal.dot(dir);
    let cos_there = -patch.normal.dot(dir);
    if cos_here <= 0.0 || cos_there <= 0.0 {
        return 0.0;
    }

    patch.emission * patch.area * cos_here * cos_there / (PI * dist_sq.max(patch.area))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{floor, level_with, wall};

    const EPS: f64 = 1e-9;

    fn no_occluders() -> Occluders {
        Occluders::new(&level_with(Vec::new()))
    }

    #[test]
    fn patches_cover_faces_in_blocks() {
        let face = Samples::new(&floor(128.0, 0.0)).unwrap();
        let light = vec![10.0; face.points.len()];
        let patches = face_patches(&face, &light, 0.5);

        // 9 × 9 luxels in blocks of 4: 3 × 3 patches.
        assert_eq!(patches.len(), 9);
        let area: f64 = patches.iter().map(|p| p.area).sum();
        assert!((area - 128.0 * 128.0).abs() < EPS);
        assert!(patches.iter().all(|p| (p.emission - 5.0).abs() < EPS));
    }

    #[test]
    fn received_light_follows_both_cosines_and_distance() {
        let patch = Patch {
            center: DVec3::new(0.0, 0.0, 100.0),
            normal: DVec3::NEG_Z,
            area: 400.0,
            emission: 10.0,
        };
        let below = received(DVec3::ZERO, DVec3::Z, &patch);
        assert!((below - 10.0 * 400.0 / (PI * 100.0 * 100.0)).abs() < EPS);

        let slanted = received(DVec3::new(100.0, 0.0, 0.0), DVec3::Z, &patch);
        assert!(slanted < below / 2.0, "both cosines and the longer path");
        assert_eq!(
            received(DVec3::ZERO, DVec3::NEG_Z, &patch),
            0.0,
            "facing away"
        );

        let near = received(DVec3::new(0.0, 0.0, 99.0), DVec3::Z, &patch);
        assert!(
            (near - 10.0 / PI).abs() < EPS,
            "clamped at the patch's size"
        );
    }

    #[test]
    fn a_lit_floor_lights_a_dark_wall() {
        let faces = vec![Samples::new(&floor(64.0, 0.0)), Samples::new(&wall(64.0))];
        let lit_floor = vec![200.0; 25];
        let dark_wall = vec![0.0; 25];
        let occluders = no_occluders();
        let bounce = |passes| Bounce {
            occluders: &occluders,
            passes,
            scale: 1.0,
        };

        let light = add(
            &faces,
            &[0.5, 0.5],
            vec![lit_floor.clone(), dark_wall.clone()],
            &bounce(1),
        );
        // The last row is level with the floor; every row above glows.
        assert!(light[1][..20].iter().all(|&l| l > 0.0), "the wall glows");
        assert_eq!(light[0], lit_floor, "the dark wall gives nothing back yet");

        let two = add(
            &faces,
            &[0.5, 0.5],
            vec![lit_floor.clone(), dark_wall.clone()],
            &bounce(2),
        );
        let sum = |light: &[f64]| light.iter().sum::<f64>();
        assert!(sum(&two[0]) > sum(&lit_floor), "second bounce returns");

        let none = add(
            &faces,
            &[0.5, 0.5],
            vec![lit_floor, dark_wall.clone()],
            &bounce(0),
        );
        assert_eq!(none[1], dark_wall);
    }

    #[test]
    fn black_surfaces_reflect_nothing() {
        let faces = vec![Samples::new(&floor(64.0, 0.0)), Samples::new(&wall(64.0))];
        let occluders = no_occluders();
        let bounce = Bounce {
            occluders: &occluders,
            passes: 1,
            scale: 1.0,
        };
        let light = add(
            &faces,
            &[0.0, 0.0],
            vec![vec![200.0; 25], vec![0.0; 25]],
            &bounce,
        );

        assert!(light[1].iter().all(|&l| l == 0.0));
    }
}
