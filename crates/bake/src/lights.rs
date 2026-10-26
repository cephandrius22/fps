//! Light sources and global settings, from entities, read as Quake's
//! `light` tool reads them.
//!
//! ```text
//!   "classname" "light…"   any class starting "light" (light, light_torch_…)
//!   "origin"    "x y z"    where, map units
//!   "light"     "300"      brightness (default 300)
//!   "wait"      "1"        distance scale: 2 reaches half as far
//!   "delay"     "0"        falloff: 0 linear, 1 1/d, 2 1/d²
//!
//!   worldspawn "light" (or "_minlight")   least light anywhere
//!   worldspawn "_anglescale"              how much light depends on
//!                                         the angle it arrives at (0.5)
//!   worldspawn "_bounce"                  bounce passes (1; 0 for none)
//!   worldspawn "_bouncescale"             bounced light multiplier (1)
//! ```
//!
//! Defaults come from the `entities` catalog ("light_…" classes take
//! those of "light"). Not yet: colors, spotlights ("target", "mangle"),
//! sun light.

use entities::Keys;
use glam::DVec3;
use level::Entity;

/// Every class starting with this is a light; all read "light"'s keys.
const LIGHT: &str = "light";
const WORLDSPAWN: &str = "worldspawn";
/// A distance scale ("wait") must be positive.
const MIN_SCALE: f64 = f64::MIN_POSITIVE;
/// Distance at which 1/d and 1/d² lights equal their brightness.
const INVERSE_UNIT: f64 = 128.0;
/// "delay" values.
const LINEAR: i64 = 0;
const INVERSE: i64 = 1;
const INVERSE_SQUARE: i64 = 2;

/// How light fades with distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Falloff {
    /// Brightness less distance: reaches exactly `brightness` units.
    Linear,
    /// Brightness × 128 / distance.
    Inverse,
    /// Brightness × (128 / distance)².
    InverseSquare,
}

/// A point light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Light {
    /// Where, map units.
    pub origin: DVec3,
    /// Light at its center.
    pub brightness: f64,
    /// Multiplies distances ("wait").
    pub scale: f64,
    pub falloff: Falloff,
}

/// Map-wide settings, from worldspawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Settings {
    /// No luxel is darker than this.
    pub minlight: f64,
    /// 0: light ignores the angle it arrives at; 1: it scales by its
    /// cosine.
    pub angle_scale: f64,
    /// Bounce passes.
    pub bounces: u32,
    /// Multiplies bounced light.
    pub bounce_scale: f64,
}

impl Light {
    /// Light reaching `point` on a surface facing `normal`, before
    /// shadows. Zero behind the surface.
    pub(crate) fn at(&self, point: DVec3, normal: DVec3, settings: &Settings) -> f64 {
        let to_light = self.origin - point;
        let dist = to_light.length();
        let cos = match dist > 0.0 {
            true => normal.dot(to_light / dist),
            false => 1.0,
        };
        if cos <= 0.0 {
            return 0.0;
        }

        let angle = 1.0 - settings.angle_scale + settings.angle_scale * cos;
        self.reaching(dist) * angle
    }

    /// Light reaching `point` from any direction (no surface to face),
    /// before shadows.
    pub(crate) fn around(&self, point: DVec3) -> f64 {
        self.reaching(self.origin.distance(point))
    }

    /// Light left after `dist` units.
    fn reaching(&self, dist: f64) -> f64 {
        let dist = dist * self.scale;
        match self.falloff {
            Falloff::Linear => (self.brightness - dist).max(0.0),
            Falloff::Inverse => self.brightness * INVERSE_UNIT / dist.max(1.0),
            Falloff::InverseSquare => self.brightness * (INVERSE_UNIT / dist.max(1.0)).powi(2),
        }
    }
}

/// Every light entity with an origin.
pub(crate) fn lights(entities: &[Entity]) -> Vec<Light> {
    entities
        .iter()
        .filter(|e| e.classname().starts_with(LIGHT))
        .filter_map(light)
        .collect()
}

fn light(entity: &Entity) -> Option<Light> {
    let keys = Keys::as_class(entity, LIGHT);
    let origin = keys.vector("origin")?;
    let falloff = match keys.number("delay").map(|d| d as i64) {
        None | Some(LINEAR) => Falloff::Linear,
        Some(INVERSE) => Falloff::Inverse,
        Some(INVERSE_SQUARE) => Falloff::InverseSquare,
        Some(other) => {
            log::warn!("light at {origin}: \"delay\" {other} unsupported, using linear");
            Falloff::Linear
        }
    };

    Some(Light {
        origin,
        brightness: keys.number("light").unwrap_or_default(),
        scale: keys.number("wait").unwrap_or_default().max(MIN_SCALE),
        falloff,
    })
}

/// Settings from the worldspawn entity, through its class's defaults.
/// ericw-tools' names ("_minlight", "_anglesense") win when present.
pub(crate) fn settings(entities: &[Entity]) -> Settings {
    let none = Entity::default();
    let world = entities
        .iter()
        .find(|e| e.classname() == WORLDSPAWN)
        .unwrap_or(&none);
    let keys = Keys::as_class(world, WORLDSPAWN);
    let get = |key| keys.number(key).unwrap_or_default();

    Settings {
        minlight: keys.number("_minlight").unwrap_or_else(|| get("light")),
        angle_scale: keys
            .number("_anglesense")
            .unwrap_or_else(|| get("_anglescale"))
            .clamp(0.0, 1.0),
        bounces: get("_bounce").max(0.0) as u32,
        bounce_scale: get("_bouncescale").max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    fn entity(pairs: &[(&str, &str)]) -> Entity {
        Entity::new(pairs.iter().map(|&(k, v)| (k.into(), v.into())).collect())
    }

    fn settings(angle_scale: f64) -> Settings {
        Settings {
            minlight: 0.0,
            angle_scale,
            // The catalog's defaults.
            bounces: 1,
            bounce_scale: 1.0,
        }
    }

    #[test]
    fn reads_light_entities_with_defaults() {
        let entities = [
            entity(&[("classname", "worldspawn")]),
            entity(&[("classname", "light"), ("origin", "0 0 160")]),
            entity(&[
                ("classname", "light_torch_small_walltorch"),
                ("origin", "1 2 3"),
                ("light", "200"),
                ("wait", "2"),
                ("delay", "2"),
            ]),
            entity(&[("classname", "light"), ("origin", "bad")]),
            entity(&[("classname", "info_player_start"), ("origin", "0 0 0")]),
        ];
        let lights = lights(&entities);

        assert_eq!(lights.len(), 2);
        assert_eq!(lights[0].origin, DVec3::new(0.0, 0.0, 160.0));
        assert_eq!((lights[0].brightness, lights[0].scale), (300.0, 1.0));
        assert_eq!(lights[0].falloff, Falloff::Linear);
        assert_eq!((lights[1].brightness, lights[1].scale), (200.0, 2.0));
        assert_eq!(lights[1].falloff, Falloff::InverseSquare);
    }

    #[test]
    fn worldspawn_sets_minlight_angle_scale_and_bounces() {
        let world = entity(&[
            ("classname", "worldspawn"),
            ("light", "20"),
            ("_anglescale", "2"),
            ("_bounce", "3"),
            ("_bouncescale", "0.5"),
        ]);
        let read = super::settings(&[world]);

        assert_eq!(read.minlight, 20.0);
        assert_eq!(read.angle_scale, 1.0, "clamped");
        assert_eq!((read.bounces, read.bounce_scale), (3, 0.5));
        assert_eq!(super::settings(&[]), self::settings(0.5));
    }

    #[test]
    fn falloffs_fade_with_scaled_distance() {
        let light = |falloff, scale| Light {
            origin: DVec3::new(0.0, 0.0, 100.0),
            brightness: 300.0,
            scale,
            falloff,
        };
        let below = |l: Light| l.at(DVec3::ZERO, DVec3::Z, &settings(0.5));

        assert!((below(light(Falloff::Linear, 1.0)) - 200.0).abs() < EPS);
        assert!(
            (below(light(Falloff::Linear, 4.0))).abs() < EPS,
            "out of reach"
        );
        assert!((below(light(Falloff::Inverse, 1.0)) - 384.0).abs() < EPS);
        assert!((below(light(Falloff::InverseSquare, 1.0)) - 491.52).abs() < EPS);
    }

    #[test]
    fn angle_dims_slanted_light_and_none_reaches_behind() {
        let light = Light {
            origin: DVec3::new(100.0, 0.0, 100.0),
            brightness: 300.0,
            scale: 1.0,
            falloff: Falloff::Linear,
        };
        let reach = 300.0 - 100.0 * 2f64.sqrt();
        let cos = std::f64::consts::FRAC_1_SQRT_2;

        let at = |scale| light.at(DVec3::ZERO, DVec3::Z, &settings(scale));
        assert!((at(0.0) - reach).abs() < EPS, "angle ignored");
        assert!((at(1.0) - reach * cos).abs() < EPS, "cosine");
        assert!((at(0.5) - reach * (0.5 + 0.5 * cos)).abs() < EPS);
        assert_eq!(light.at(DVec3::ZERO, DVec3::NEG_Z, &settings(0.5)), 0.0);
    }
}
