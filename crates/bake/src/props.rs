//! Props (glTF models placed by "prop_…" entities) have no lightmap:
//! the baker writes one light level per prop, the light at a point just
//! above its origin, which the engine applies to the whole model.
//!
//! ```text
//!   prop_static at (x, y, z) ─► light at (x, y, z + 16)
//!                             ─► "_light" "87"   (luxel units, 128 full)
//! ```
//!
//! Direct light and minlight only; bounced light on props waits for
//! light probes.

use entities::{BAKED_LIGHT, Keys};
use glam::DVec3;
use level::Entity;

use crate::face::{Scene, luxel};

/// Classes that get a light level.
const PROP_PREFIX: &str = "prop_";
/// Map units above the origin (a prop's base) where light is sampled.
const SAMPLE_LIFT: f64 = 16.0;

/// Sets every prop's light level; returns how many.
pub(crate) fn set_light(entities: &mut [Entity], scene: &Scene) -> usize {
    let mut count = 0;
    for entity in entities
        .iter_mut()
        .filter(|e| e.classname().starts_with(PROP_PREFIX))
    {
        let Some(origin) = Keys::of(entity).vector("origin") else {
            continue;
        };

        let level = luxel(light_at(origin + DVec3::Z * SAMPLE_LIFT, scene));
        entity.set(BAKED_LIGHT, &level.to_string());
        count += 1;
    }
    count
}

/// Unshadowed light from every direction at `point`, at least minlight.
fn light_at(point: DVec3, scene: &Scene) -> f64 {
    let lit: f64 = scene
        .lights
        .iter()
        .filter(|light| !scene.occluders.blocked(point, light.origin))
        .map(|light| light.around(point))
        .sum();
    lit.max(scene.settings.minlight)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lights::{self, Settings};
    use crate::occlusion::Occluders;
    use crate::testing::{cuboid_model, level_with, light};

    fn prop(origin: &str) -> Entity {
        Entity::new(vec![
            ("classname".into(), "prop_static".into()),
            ("origin".into(), origin.into()),
        ])
    }

    #[test]
    fn props_get_the_light_above_their_origin() {
        // A wall between the light and the second prop.
        let wall = cuboid_model(DVec3::new(50.0, -64.0, 0.0), DVec3::new(60.0, 64.0, 128.0));
        let mut level = level_with(vec![wall]);
        level
            .entities
            .push(light(DVec3::new(0.0, 0.0, 116.0), 300.0));
        level
            .entities
            .extend([prop("0 0 0"), prop("100 0 0"), prop("bad")]);

        let lights = lights::lights(&level.entities);
        let occluders = Occluders::new(&level);
        let settings = Settings {
            minlight: 10.0,
            ..lights::settings(&level.entities)
        };
        let scene = Scene {
            lights: &lights,
            occluders: &occluders,
            settings: &settings,
        };

        assert_eq!(set_light(&mut level.entities, &scene), 2);
        // 300 − 100 units = 200, stored halved.
        assert_eq!(level.entities[2].get(BAKED_LIGHT), Some("100"));
        assert_eq!(
            level.entities[3].get(BAKED_LIGHT),
            Some("5"),
            "minlight behind the wall"
        );
        assert_eq!(level.entities[4].get(BAKED_LIGHT), None);
    }
}
