//! TrenchBroom .map source reader: the text a level designer saves,
//! read as written. Compiling it into a level (polygons, CSG, contents)
//! is `mapc`'s job.
//!
//! ```text
//!   // comment
//!   {                                      entity
//!   "classname" "worldspawn"               key/value pairs
//!   {                                      brush: the space behind
//!   ( 0 0 0 ) ( 0 64 0 ) ( 64 0 0 ) floor 0 0 0 1 1      every face plane
//!   ( 0 0 0 ) ( 0 64 0 ) ( 64 0 0 ) floor [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1
//!   …                                      ▲ Valve 220: explicit axes
//!   }
//!   }
//! ```
//!
//! A face is three points on its plane, clockwise seen from outside the
//! brush, then a texture name and its mapping. Both Quake 1 mappings are
//! read: standard (offset, rotation, scale) and Valve 220 (axes too),
//! face by face. Map space: Quake units, Z up. Errors name the line.
//!
//! Modules:
//!
//! ```text
//!   lib.rs     parse()
//!   model.rs   Map, Entity, Brush, Face, Axes: what the file says
//!   parse.rs   the grammar: entities → brushes → faces
//!   lexer.rs   characters → numbers, words, quoted strings; comments
//!   error.rs   MapError
//! ```

mod error;
mod lexer;
mod model;
mod parse;

pub use error::MapError;
pub use model::{Axes, Brush, Entity, Face, Map};

/// Reads .map text.
pub fn parse(text: &str) -> Result<Map, MapError> {
    parse::map(text)
}
