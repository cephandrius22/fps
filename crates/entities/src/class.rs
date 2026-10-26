//! What an entity class definition says: its kind, its keys and their
//! types and defaults.

/// How the class appears in a map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Placed at a point ("origin"), drawn in the editor as a box.
    Point {
        /// Editor box, map units: mins, maxs.
        size: ([i32; 3], [i32; 3]),
        /// Editor box color.
        color: [u8; 3],
        /// The key naming a model file the editor draws in place of
        /// the box, e.g. "model".
        model: Option<&'static str>,
    },
    /// Made of brushes, which become its model.
    Brush,
    /// Brushes that only group others: merged into the world when
    /// compiled.
    Group,
}

/// What a key holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyType {
    /// Any text.
    Text,
    /// A whole number.
    Integer,
    /// A decimal number.
    Float,
    /// "x y z".
    Vector,
    /// This entity's name, which others target.
    Name,
    /// The name of entities to fire.
    Target,
    /// One of a fixed set.
    Choices(&'static [Choice]),
}

/// One allowed value of a [`KeyType::Choices`] key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    /// As written in the map, e.g. "0".
    pub value: &'static str,
    /// For the editor, e.g. "Linear".
    pub label: &'static str,
}

/// One key a class reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// e.g. "speed".
    pub name: &'static str,
    /// What it holds.
    pub kind: KeyType,
    /// Used when a map leaves the key out, as written in a map.
    pub default: Option<&'static str>,
    /// For the editor.
    pub description: &'static str,
}

/// One entity class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Class {
    /// e.g. "func_door".
    pub name: &'static str,
    /// Point, brush or group.
    pub kind: Kind,
    /// For the editor.
    pub description: &'static str,
    /// Every key it reads.
    pub keys: &'static [Key],
}

impl Class {
    /// The key called `name`.
    pub fn key(&self, name: &str) -> Option<&'static Key> {
        self.keys.iter().find(|k| k.name == name)
    }
}
