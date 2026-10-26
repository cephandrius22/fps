//! Lightmap baker: lights a level's faces from its light entities, with
//! shadows cast by its brushes and light bounced between surfaces, into
//! `level::Lightmap`s. Grayscale, in the manner of Quake's `light`
//! tool, plus bounces.
//!
//! ```text
//!   Level ─┬─ entities ─► lights.rs: point lights, worldspawn settings
//!          ├─ brushes ──► occlusion.rs: what casts shadows
//!          ├─ faces ────► face.rs: one sample per luxel
//!          │                ─► direct light ─► bounce.rs: + bounced light
//!          │                ─► Face.lightmap
//!          └─ props ────► light at the origin ─► "_light" key (props.rs)
//! ```
//!
//! Lightmaps follow `level::LuxelGrid` (one luxel per 16 texels), so
//! they keep Quake's texel density. How much each surface reflects (its
//! albedo) comes from the caller, which knows the textures. Not yet:
//! colored light, spotlights, sun light, and anti-aliasing (several
//! samples per luxel).
//!
//! Modules:
//!
//! ```text
//!   lib.rs         bake()
//!   lights.rs      Light, Settings: from entities
//!   face.rs        Samples: sample points, direct light, the lightmap
//!   bounce.rs      patches re-emitting light; gathering it
//!   props.rs       one light level per prop entity
//!   occlusion.rs   Occluders: segment against brushes
//! ```

mod bounce;
mod face;
mod lights;
mod occlusion;
mod props;
#[cfg(test)]
mod testing;

use level::Level;

use bounce::Bounce;
use face::{Samples, Scene};
use occlusion::Occluders;

/// How much light a texture reflects, 0 to 1, by name.
pub type Albedo<'a> = dyn Fn(&str) -> f64 + 'a;

/// What a bake did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Baked {
    /// Light entities used.
    pub lights: usize,
    /// Faces given a lightmap.
    pub faces: usize,
    /// Luxels computed.
    pub luxels: usize,
    /// Bounce passes run.
    pub bounces: u32,
    /// Prop entities given a light level.
    pub props: usize,
}

/// Lights every face of `level`, replacing any lightmaps it had. Brush
/// entities are lit where the map places them. `albedo` says how much
/// each texture reflects.
pub fn bake(level: &mut Level, albedo: &Albedo) -> Baked {
    let lights = lights::lights(&level.entities);
    let settings = lights::settings(&level.entities);
    let occluders = Occluders::new(level);
    let scene = Scene {
        lights: &lights,
        occluders: &occluders,
        settings: &settings,
    };

    // Every face in one list, so bounces see across models.
    let faces: Vec<&level::Face> = level.models.iter().flat_map(|m| &m.faces).collect();
    let samples: Vec<Option<Samples>> = faces.iter().map(|f| Samples::new(f)).collect();
    let albedos: Vec<f64> = faces
        .iter()
        .map(|f| {
            level
                .textures
                .get(f.texture)
                .map_or(0.0, |t| albedo(&t.name))
        })
        .collect();
    let direct = samples
        .iter()
        .map(|s| {
            s.as_ref()
                .map(|s| face::direct(s, &scene))
                .unwrap_or_default()
        })
        .collect();

    let bounce = Bounce {
        occluders: &occluders,
        passes: settings.bounces,
        scale: settings.bounce_scale,
    };
    let light = bounce::add(&samples, &albedos, direct, &bounce);

    let mut baked = Baked {
        lights: lights.len(),
        bounces: settings.bounces,
        props: props::set_light(&mut level.entities, &scene),
        ..Baked::default()
    };
    let faces = level.models.iter_mut().flat_map(|m| &mut m.faces);
    for ((face, samples), light) in faces.zip(&samples).zip(&light) {
        face.lightmap = samples
            .as_ref()
            .map(|s| s.lightmap(light, settings.minlight));
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
    use crate::testing::{cuboid_model, floor, level_with, light, wall};

    /// Every texture reflects half.
    fn gray(_: &str) -> f64 {
        0.5
    }

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

        let baked = bake(&mut level, &gray);

        let expected = Baked {
            lights: 1,
            faces: 2,
            luxels: 25 + 9,
            bounces: 1,
            props: 0,
        };
        assert_eq!(baked, expected);
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

        assert_eq!(bake(&mut level, &gray).lights, 0);
        let lightmap = level.models[0].faces[0].lightmap.as_ref().unwrap();
        assert!(lightmap.luxels.iter().all(|&l| l == 15));
    }

    #[test]
    fn bounces_brighten_a_wall_beside_a_lit_floor() {
        // A light low over a floor beside a wall: the floor's bounced
        // light adds to what reaches the wall directly.
        let mut world = cuboid_model(DVec3::splat(-1.0), DVec3::ZERO);
        world.faces = vec![floor(64.0, 0.0), wall(64.0)];
        let mut level = level_with(vec![world]);
        level
            .entities
            .push(light(DVec3::new(32.0, 32.0, 8.0), 300.0));

        let wall_light = |bounces: &str| {
            let mut level = level.clone();
            level.entities[0].set("_bounce", bounces);
            bake(&mut level, &gray);
            let lightmap = level.models[0].faces[1].lightmap.clone().unwrap();
            lightmap.luxels.iter().map(|&l| u32::from(l)).sum::<u32>()
        };

        assert!(wall_light("1") > wall_light("0"));
    }
}
