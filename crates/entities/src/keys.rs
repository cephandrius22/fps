//! Reading an entity's keys, falling back to its class's defaults, so
//! defaults live only in the catalog.
//!
//! ```text
//!   "speed" "200" in the map         → 200
//!   no "speed", or "speed" "fast"    → func_door's default, 100
//!   no default either                → None
//! ```

use glam::DVec3;
use level::Entity;

use crate::catalog::class;
use crate::class::Class;

/// An entity's keys, read through a class's defaults.
#[derive(Clone, Copy, Debug)]
pub struct Keys<'a> {
    entity: &'a Entity,
    class: Option<&'static Class>,
}

impl<'a> Keys<'a> {
    /// Keys with the defaults of the entity's own class.
    pub fn of(entity: &'a Entity) -> Self {
        Self {
            entity,
            class: class(entity.classname()),
        }
    }

    /// Keys with the defaults of class `name`, e.g. "light" for every
    /// "light_…" class.
    pub fn as_class(entity: &'a Entity, name: &str) -> Self {
        Self {
            entity,
            class: class(name),
        }
    }

    /// The text of `key`, or its default.
    pub fn text(&self, key: &str) -> Option<&'a str> {
        self.entity.get(key).or_else(|| self.default(key))
    }

    /// `key` as a number; its default when missing or not a number.
    pub fn number(&self, key: &str) -> Option<f64> {
        let parse = |v: &str| v.trim().parse().ok();
        self.entity
            .get(key)
            .and_then(parse)
            .or_else(|| self.default(key).and_then(parse))
    }

    /// `key` as "x y z"; its default when missing or malformed.
    pub fn vector(&self, key: &str) -> Option<DVec3> {
        self.entity
            .get(key)
            .and_then(vector)
            .or_else(|| self.default(key).and_then(vector))
    }

    fn default(&self, key: &str) -> Option<&'static str> {
        self.class?.key(key)?.default
    }
}

fn vector(text: &str) -> Option<DVec3> {
    let parts: Vec<f64> = text
        .split_whitespace()
        .map(|v| v.parse().ok())
        .collect::<Option<_>>()?;
    match parts[..] {
        [x, y, z] => Some(DVec3::new(x, y, z)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(pairs: &[(&str, &str)]) -> Entity {
        Entity::new(pairs.iter().map(|&(k, v)| (k.into(), v.into())).collect())
    }

    #[test]
    fn values_win_over_defaults() {
        let door = entity(&[
            ("classname", "func_door"),
            ("speed", "200"),
            ("lip", "fast"),
        ]);
        let keys = Keys::of(&door);

        assert_eq!(keys.number("speed"), Some(200.0));
        assert_eq!(keys.number("lip"), Some(8.0), "malformed: the default");
        assert_eq!(keys.number("wait"), Some(3.0), "missing: the default");
        assert_eq!(keys.number("targetname"), None, "no default");
        assert_eq!(keys.text("classname"), Some("func_door"));
    }

    #[test]
    fn vectors_and_borrowed_classes() {
        let torch = entity(&[("classname", "light_torch"), ("origin", "1 2 3")]);

        assert_eq!(Keys::of(&torch).number("light"), None, "an unknown class");
        let as_light = Keys::as_class(&torch, "light");
        assert_eq!(as_light.number("light"), Some(300.0));
        assert_eq!(as_light.vector("origin"), Some(DVec3::new(1.0, 2.0, 3.0)));

        let bad = entity(&[("classname", "light"), ("origin", "1 2")]);
        assert_eq!(Keys::of(&bad).vector("origin"), None);
    }
}
