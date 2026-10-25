//! What a .map file says, before compiling. Map space: Quake units,
//! Z up.

use glam::{DVec2, DVec3};

/// A whole .map file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map {
    /// In file order; worldspawn is normally first.
    pub entities: Vec<Entity>,
}

/// One entity: its keys, and its brushes if it is a brush entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entity {
    /// Key/value pairs, in file order.
    pub fields: level::Entity,
    /// Solids; empty for point entities (lights, the player start).
    pub brushes: Vec<Brush>,
    /// 1-based line of its opening brace.
    pub line: usize,
}

/// A convex solid: the space behind every face's plane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Brush {
    /// One per bounding plane.
    pub faces: Vec<Face>,
    /// 1-based line of its opening brace.
    pub line: usize,
}

/// One bounding plane of a brush, and how its texture lies on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Three points on the plane, clockwise seen from outside the brush.
    pub points: [DVec3; 3],
    /// Texture name, e.g. "floor" or "*water1".
    pub texture: String,
    /// Where the texture axes come from.
    pub axes: Axes,
    /// Texels added along each axis.
    pub offset: DVec2,
    /// Degrees, counter-clockwise. Valve 220 axes already include it.
    pub rotation: f64,
    /// Map units per texel along each axis.
    pub scale: DVec2,
}

/// Texture axes: s runs across the texture, t down it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Axes {
    /// Quake standard: the world axes nearest the face's plane.
    Standard,
    /// Valve 220: given in the file, `[s, t]`.
    Valve([DVec3; 2]),
}
