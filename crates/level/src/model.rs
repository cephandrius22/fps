//! The level data model. Map space throughout: Quake units, Z up.

use glam::{Vec2, Vec3};

/// A whole compiled map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Level {
    /// Textures by index, as faces refer to them.
    pub textures: Vec<Texture>,
    /// Model 0 is the world; the rest belong to brush entities.
    pub models: Vec<Model>,
    /// Every entity, worldspawn first.
    pub entities: Vec<Entity>,
}

/// Textures that mark brushes for the compiler or the game (trigger
/// volumes, invisible walls), never drawn.
const TOOL_TEXTURES: [&str; 6] = ["trigger", "clip", "skip", "hint", "origin", "null"];

/// Whether `name` is a tool texture, in any case: e.g. "trigger".
pub fn is_tool_texture(name: &str) -> bool {
    TOOL_TEXTURES
        .iter()
        .any(|tool| tool.eq_ignore_ascii_case(name))
}

/// A texture's name and size. The pixels live elsewhere (the engine
/// loads `textures/<name>.png`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Texture {
    /// e.g. "floor"; empty for a missing slot.
    pub name: String,
    /// Width in texels; 0 when unknown.
    pub width: u32,
    /// Height in texels; 0 when unknown.
    pub height: u32,
}

/// The world or one brush entity's geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// Bounds, in map units.
    pub mins: Vec3,
    /// Bounds, in map units.
    pub maxs: Vec3,
    /// Visible polygons.
    pub faces: Vec<Face>,
    /// Convex solids for collision.
    pub brushes: Vec<Brush>,
}

/// A convex visible polygon.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Index into [`Level::textures`].
    pub texture: usize,
    /// Unit normal on the visible side.
    pub normal: Vec3,
    /// Corners, counter-clockwise seen from the visible side.
    pub points: Vec<Vec3>,
    /// Texture coordinates per corner, in texels.
    pub texels: Vec<Vec2>,
    /// Baked light; None when the face is unlit (e.g. sky) or the map
    /// was not lit.
    pub lightmap: Option<Lightmap>,
}

/// A face's baked light: a small grid of luxels, one per 16 × 16 texels.
///
/// ```text
///   luxel (0,0)   (1,0)   …  (width−1, 0)
///     ·       ·       ·          corners map into this grid by `coords`;
///     ·       ·       ·          luxel centers sit at .5
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Lightmap {
    /// Luxels across.
    pub width: u32,
    /// Luxels down.
    pub height: u32,
    /// Brightness per luxel, row-major. Quake doubles it when lighting
    /// ("overbright"): 128 is full texture brightness, 255 nearly twice.
    pub luxels: Vec<u8>,
    /// Per corner (matching `Face::points`), position in luxels.
    pub coords: Vec<Vec2>,
}

/// A convex solid: the space behind every plane.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    /// What fills it.
    pub contents: Contents,
    /// Bounds, in map units.
    pub mins: Vec3,
    /// Bounds, in map units.
    pub maxs: Vec3,
    /// Outward planes, enclosing the brush on every side.
    pub planes: Vec<Plane>,
}

/// Points `p` with `normal · p = dist`; outside where greater.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// Unit normal.
    pub normal: Vec3,
    /// Distance from the origin along the normal.
    pub dist: f32,
}

/// What a brush is made of. Codes follow Quake's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contents {
    /// Nothing.
    Empty,
    /// Walls and floors.
    Solid,
    /// Swimmable.
    Water,
    /// Swimmable, hurts.
    Slime,
    /// Swimmable, hurts more.
    Lava,
    /// Sky: solid to movement.
    Sky,
    /// Invisible wall: solid, never drawn.
    Clip,
    /// A code no format here assigns a meaning.
    Other(i16),
}

/// Quake contents codes, as .lvl files store them.
const EMPTY: i16 = -1;
const SOLID: i16 = -2;
const WATER: i16 = -3;
const SLIME: i16 = -4;
const LAVA: i16 = -5;
const SKY: i16 = -6;
/// ericw-tools' code for clip brushes, kept for familiarity.
const CLIP: i16 = -8;

impl Contents {
    /// From a Quake contents code.
    pub fn from_code(code: i16) -> Self {
        match code {
            EMPTY => Contents::Empty,
            SOLID => Contents::Solid,
            WATER => Contents::Water,
            SLIME => Contents::Slime,
            LAVA => Contents::Lava,
            SKY => Contents::Sky,
            CLIP => Contents::Clip,
            other => Contents::Other(other),
        }
    }

    /// The Quake contents code.
    pub fn code(self) -> i16 {
        match self {
            Contents::Empty => EMPTY,
            Contents::Solid => SOLID,
            Contents::Water => WATER,
            Contents::Slime => SLIME,
            Contents::Lava => LAVA,
            Contents::Sky => SKY,
            Contents::Clip => CLIP,
            Contents::Other(code) => code,
        }
    }

    /// True for contents that block movement: solid, sky and clip.
    pub fn is_solid(self) -> bool {
        matches!(self, Contents::Solid | Contents::Sky | Contents::Clip)
    }
}

/// One entity's key/value pairs, in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entity {
    fields: Vec<(String, String)>,
}

impl Entity {
    /// Entity with these pairs, in order.
    pub fn new(fields: Vec<(String, String)>) -> Self {
        Self { fields }
    }

    /// Value of `key`; the last one wins if a key repeats.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Sets `key`, replacing every earlier value.
    pub fn set(&mut self, key: &str, value: &str) {
        self.fields.retain(|(k, _)| k != key);
        self.fields.push((key.to_owned(), value.to_owned()));
    }

    /// The `classname`, or "" if missing.
    pub fn classname(&self) -> &str {
        self.get("classname").unwrap_or_default()
    }

    /// Brush model index from `"model" "*N"`. The world is model 0
    /// without the key.
    pub fn model(&self) -> Option<usize> {
        if self.classname() == "worldspawn" {
            return Some(0);
        }
        self.get("model")?.strip_prefix('*')?.parse().ok()
    }

    /// Every pair, in file order.
    pub fn fields(&self) -> impl Iterator<Item = (&str, &str)> {
        self.fields.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(pairs: &[(&str, &str)]) -> Entity {
        Entity::new(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn tool_textures_match_in_any_case() {
        assert!(is_tool_texture("trigger") && is_tool_texture("CLIP"));
        assert!(!is_tool_texture("floor") && !is_tool_texture("triggers"));
    }

    #[test]
    fn contents_codes_round_trip() {
        for code in [-1, -2, -3, -4, -5, -6, -8, -42] {
            assert_eq!(Contents::from_code(code).code(), code);
        }
        assert_eq!(Contents::from_code(-42), Contents::Other(-42));
    }

    #[test]
    fn solid_contents_block_movement() {
        assert!(
            Contents::Solid.is_solid() && Contents::Sky.is_solid() && Contents::Clip.is_solid()
        );
        assert!(!Contents::Water.is_solid() && !Contents::Empty.is_solid());
    }

    #[test]
    fn last_value_wins_and_set_replaces() {
        let mut e = entity(&[("wait", "1"), ("wait", "2")]);
        assert_eq!(e.get("wait"), Some("2"));

        e.set("wait", "3");
        assert_eq!(e.get("wait"), Some("3"));
        assert_eq!(e.fields().filter(|(k, _)| *k == "wait").count(), 1);
    }

    #[test]
    fn model_index_comes_from_star_n_or_worldspawn() {
        assert_eq!(entity(&[("classname", "worldspawn")]).model(), Some(0));
        assert_eq!(
            entity(&[("classname", "func_door"), ("model", "*3")]).model(),
            Some(3)
        );
        assert_eq!(entity(&[("classname", "light")]).model(), None);
    }
}
