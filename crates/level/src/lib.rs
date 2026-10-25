//! Level data: what a compiled map holds, whatever file it came from.
//! The engine loads this model; the `bsp` crate reads it from Quake
//! .bsp files, and `mapc` builds it from TrenchBroom .map files and
//! stores it in the engine's own .lvl format.
//!
//! ```text
//!   .bsp ─► bsp::parse ──┐
//!                        ├─► Level ─► engine (map loading)
//!   .lvl ─► level::read ─┘
//!   .map ─► mapc ─► Level ─► level::write ─► .lvl
//! ```
//!
//! Everything is in map space, as authored: Quake units (32 per meter
//! in the engine), right-handed, Z up. Converting to engine space is
//! the engine's job, so all formats share one conversion.
//!
//! Modules:
//!
//! ```text
//!   model.rs   Level, Model, Face, Lightmap, Brush, Plane, Contents,
//!              Texture, Entity: the data model
//!   format.rs  write() / read(): the .lvl binary format
//!   error.rs   LevelError
//! ```

mod error;
mod format;
mod model;

pub use error::LevelError;
pub use format::{read, write};
pub use model::{
    Brush, Contents, Entity, Face, Level, Lightmap, Model, Plane, Texture, is_tool_texture,
};
