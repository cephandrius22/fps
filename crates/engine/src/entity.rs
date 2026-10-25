//! Entities: one flat struct with optional components. A crate is a
//! model plus a collider; a door adds a mover; a button adds `Usable`.
//! Models and colliders are lists, because a brush entity from a map
//! (a door, say) is often several brushes with several textures.
//!
//! Entities talk by name, as in Quake's target / targetname: a trigger,
//! button or script fires a target name, and every entity with that
//! `name` reacts (a mover opens or closes, a script gets `on_fire`).
//!
//! ```text
//!   button ── Usable { target: "door1" } ──┐
//!   trigger ─ Trigger { target: "door1" } ─┤ fire "door1"
//!   script ── world.fire("door1") ─────────┤
//!                                          ▼
//!                     door { name: "door1", mover }
//! ```
//!
//! `script` names a Lua class (a global table in `assets/scripts/*.lua`)
//! whose hooks run on the entity's events; see the `script` crate docs.

use glam::{Quat, Vec3};

use crate::collision::{Aabb, Brush};
use crate::gfx::{MeshHandle, TextureHandle};

/// Something in the world. Every component is optional; fill in what
/// the entity needs and leave the rest to `..Default::default()`.
#[derive(Clone, Debug)]
pub struct Entity {
    /// What triggers and buttons target, e.g. "door1".
    pub name: Option<String>,
    /// Origin in the world. Movers change it.
    pub position: Vec3,
    /// Turns the model only. Colliders never rotate: give a turned
    /// object a turned brush (`Brush::oriented_box`).
    pub rotation: Quat,
    /// What to draw: one mesh per texture, all at the entity's
    /// position and rotation. Empty draws nothing.
    pub models: Vec<Model>,
    /// Solid volumes, relative to `position`; movers move them all
    /// together. Empty is not solid.
    pub colliders: Vec<Brush>,
    /// Slides between two positions when fired: doors, gates.
    pub mover: Option<Mover>,
    /// Fires a target when the player walks in.
    pub trigger: Option<Trigger>,
    /// Fires a target when the player uses it (e.g. a button).
    pub usable: Option<Usable>,
    /// Lua class running this entity's behavior, e.g. "SecretButton".
    /// Its hooks (`on_spawn`, `on_fire`, `on_use`, `on_enter`,
    /// `on_tick`) add to the components above. Use needs a collider.
    pub script: Option<String>,
}

impl Default for Entity {
    fn default() -> Self {
        Self {
            name: None,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            models: Vec::new(),
            colliders: Vec::new(),
            mover: None,
            trigger: None,
            usable: None,
            script: None,
        }
    }
}

/// A mesh to draw at the entity's position and rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Model {
    /// Mesh, in entity-local coordinates.
    pub mesh: MeshHandle,
    /// Texture; None draws vertex colors only.
    pub texture: Option<TextureHandle>,
}

/// Slides the entity from its spawn position to `spawn + travel` when
/// fired, and back. Stops rather than crush the player: an opening
/// mover waits, a closing one reopens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mover {
    /// Offset from the spawn position when fully open.
    pub travel: Vec3,
    /// Meters per second.
    pub speed: f32,
    /// How it closes.
    pub close: Close,
}

/// When an open mover closes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Close {
    /// By itself, this many seconds after opening fully.
    After(f32),
    /// Only when fired again: a toggle.
    OnFire,
}

/// A volume that fires `target` (and the entity's script `on_enter`)
/// when the player enters it.
#[derive(Clone, Debug, PartialEq)]
pub struct Trigger {
    /// Volume, relative to the entity's position.
    pub volume: Aabb,
    /// Name to fire; None when only the script reacts.
    pub target: Option<String>,
    /// Fire once ever, or on every entry.
    pub repeat: Repeat,
}

/// How often a trigger fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    /// The first entry only.
    Once,
    /// Each time the player enters (not while staying inside).
    EveryEntry,
}

/// Fires `target` when the player presses use while looking at the
/// entity's collider, within reach. (A scripted entity is usable even
/// without this: it gets `on_use`.)
#[derive(Clone, Debug, PartialEq)]
pub struct Usable {
    /// Name to fire.
    pub target: Option<String>,
}
