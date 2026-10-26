//! Map compiler: TrenchBroom .map source to the engine's level data,
//! stored as .lvl. Does qbsp's job for the engine's own format; the
//! `mapc` command also lights the result with `bake`, doing light's.
//!
//! ```text
//!   .map ─► mapfile::parse ─► compile() ─► Level ─► bake::bake ─► level::write ─► .lvl
//!                              │ entities → models (compile.rs)            (main.rs)
//!                              │ brushes  → solids: planes + polygons (solid.rs)
//!                              │ solids   → visible faces (csg.rs)
//!                              └ faces    → texels (projection.rs), textures.rs
//! ```
//!
//! No BSP tree: the engine draws every face and collides against the
//! brushes directly, so the compiler only needs polygons. Not yet:
//! merging fragments, and dropping faces that face the void outside a
//! sealed map.
//!
//! Modules (start with `compile.rs`):
//!
//! ```text
//!   lib.rs          compile(), png_sizes(), png_albedos()
//!   compile.rs      entities → models; solids → level faces
//!   solid.rs        map brush → planes, side polygons, contents
//!   csg.rs          cut away faces inside other brushes
//!   outside.rs      flood fill: faces facing the void outside
//!   merge.rs        rejoin coplanar fragments
//!   projection.rs   Quake standard / Valve 220 texture axes → texels
//!   textures.rs     texture table; sizes and albedos from PNGs
//!   winding.rs      convex polygons: base square, cut, clip
//!   plane.rs        f64 planes from three points
//!   main.rs         the `mapc` command: compile, bake, write
//! ```

mod compile;
mod csg;
mod merge;
mod outside;
mod plane;
mod projection;
mod solid;
#[cfg(test)]
mod testing;
mod textures;
mod winding;

pub use compile::compile;
pub use textures::{TextureSizes, png_albedos, png_sizes};
