//! Entity class definitions: every class a map can place, the keys it
//! reads and their defaults. One source of truth:
//!
//! ```text
//!                 ┌─► engine (map/entities.rs): door speed, lip, wait, …
//!   catalog.rs ───┼─► bake: light brightness, falloff, worldspawn settings
//!                 ├─► mapc: which classes merge into the world
//!                 └─► fgd.rs ─► trenchbroom/RetroFPS.fgd (the editor)
//! ```
//!
//! Behavior is not here: the engine (or a Lua script, via "script")
//! gives a class meaning; this crate says what a map may write.
//!
//! Modules:
//!
//! ```text
//!   lib.rs       re-exports
//!   class.rs     Class, Kind, Key, KeyType: what a definition says
//!   catalog.rs   CLASSES: every definition
//!   keys.rs      Keys: an entity's values through its class's defaults
//!   fgd.rs       fgd(): the catalog as TrenchBroom's FGD
//!   main.rs      the `fgd` command: writes it
//! ```

mod catalog;
mod class;
mod fgd;
mod keys;

pub use catalog::{BAKED_LIGHT, CLASSES, class};
pub use class::{Choice, Class, Key, KeyType, Kind};
pub use fgd::fgd;
pub use keys::Keys;
