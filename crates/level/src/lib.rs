//! Level data: what a compiled map holds, whatever file it came from.
//! The engine loads this model; the `bsp` crate reads it from Quake
//! .bsp files, `mapc` builds it from TrenchBroom .map files, `bake`
//! lights it, and `mapc` stores it in the engine's own .lvl format.
//!
//! ```text
//!   .bsp ─► bsp::parse ──┐
//!                        ├─► Level ─► engine (map loading)
//!   .lvl ─► level::read ─┘
//!   .map ─► mapc ─► Level ─► bake ─► level::write ─► .lvl
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
//!   luxels.rs  LuxelGrid: how lightmaps lie over texels
//!   format.rs  write() / read(): the .lvl binary format
//!   error.rs   LevelError
//! ```

mod error;
mod format;
mod luxels;
mod model;

pub use error::LevelError;
pub use format::{read, write};
pub use luxels::{LuxelGrid, TEXELS_PER_LUXEL};
pub use model::{
    Brush, Contents, Entity, Face, Level, Lightmap, Model, Plane, Texture, is_tool_texture,
};
