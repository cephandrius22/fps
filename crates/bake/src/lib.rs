//! Lightmap baker: lights a level's faces from its light entities, with
//! shadows cast by its brushes, into `level::Lightmap`s. Direct light
//! only, grayscale, as Quake's original `light` tool computed it.
//!
//! ```text
//!   Level ─┬─ entities ─► lights.rs: point lights, worldspawn settings
//!          ├─ brushes ──► occlusion.rs: what casts shadows
//!          └─ faces ────► face.rs: one sample per luxel ─► Face.lightmap
//! ```
//!
//! Lightmaps follow `level::LuxelGrid` (one luxel per 16 texels), so
//! the engine draws them exactly as it draws a .bsp's. Not yet: bounced
//! light, colored light, spotlights, sun light, and anti-aliasing
//! (several samples per luxel).
//!
//! Modules:
//!
//! ```text
//!   lib.rs         bake()
//!   lights.rs      Light, Settings: from entities
//!   face.rs        one face's lightmap: samples, texels → points
//!   occlusion.rs   Occluders: segment against brushes
//! ```

mod face;
mod lights;
mod occlusion;
#[cfg(test)]
mod testing;

use level::Level;

use face::Scene;
use occlusion::Occluders;

/// What a bake did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Baked {
    /// Light entities used.
    pub lights: usize,
    /// Faces given a lightmap.
    pub faces: usize,
    /// Luxels computed.
    pub luxels: usize,
}

/// Lights every face of `level`, replacing any lightmaps it had. Brush
/// entities are lit where the map places them.
pub fn bake(level: &mut Level) -> Baked {
    let lights = lights::lights(&level.entities);
    let settings = lights::settings(&level.entities);
    let occluders = Occluders::new(level);
    let scene = Scene {
        lights: &lights,
        occluders: &occluders,
        settings: &settings,
    };

    let mut baked = Baked {
        lights: lights.len(),
        ..Baked::default()
    };
    for face in level.models.iter_mut().flat_map(|m| &mut m.faces) {
        face.lightmap = face::light_face(face, &scene);
        if let Some(lightmap) = &face.lightmap {
            baked.faces += 1;
            baked.luxels += lightmap.luxels.len();
        }
    }

    baked
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;
    use crate::testing::{cuboid_model, floor, level_with, light};

    #[test]
    fn lights_every_face_and_reports_it() {
        let mut world = cuboid_model(
            DVec3::new(-64.0, -64.0, -16.0),
            DVec3::new(128.0, 128.0, 0.0),
        );
        world.faces = vec![floor(64.0, 0.0), floor(32.0, 0.0)];
        let mut level = level_with(vec![world]);
        level
            .entities
            .push(light(DVec3::new(32.0, 32.0, 64.0), 300.0));

        let baked = bake(&mut level);

        assert_eq!(
            baked,
            Baked {
                lights: 1,
                faces: 2,
                luxels: 25 + 9
            }
        );
        let lightmap = level.models[0].faces[0].lightmap.as_ref().unwrap();
        assert!(
            lightmap.luxels.iter().all(|&l| l > 0),
            "the floor brush casts no shadow on itself"
        );
    }

    #[test]
    fn without_lights_everything_is_minlight() {
        let mut world = cuboid_model(DVec3::splat(-1.0), DVec3::ZERO);
        world.faces = vec![floor(64.0, 0.0)];
        let mut level = level_with(vec![world]);
        level.entities[0].set("light", "30");

        assert_eq!(bake(&mut level).lights, 0);
        let lightmap = level.models[0].faces[0].lightmap.as_ref().unwrap();
        assert!(lightmap.luxels.iter().all(|&l| l == 15));
    }
}
