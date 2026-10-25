//! Collision for a world built from convex brushes, Quake-style: the
//! level is a set of convex volumes, and a moving axis-aligned box is
//! traced through them.
//!
//! ```text
//!   Brush (planes) ──► World (bodies, each a brush + offset)
//!                         │
//!     trace(box, start→end) ─► Trace { fraction, normal, body }
//!                         │
//!   mover: slide / step / ground probe, built on trace
//! ```
//!
//! Units are meters, right-handed, Y up. No rotation of moving bodies:
//! a body moves by offset only (doors and platforms slide), and boxes
//! are always axis-aligned. Knows nothing about entities or input.
//!
//! Modules:
//!
//! ```text
//!   brush.rs   Brush, Plane, Face: convex solids, box clipping
//!   world.rs   World, BodyId, Trace: placed bodies and box traces
//!   mover.rs   slide, step_slide, ground, snap_down: character movement
//!   aabb.rs    Aabb
//! ```

mod aabb;
mod brush;
pub mod mover;
mod world;

pub use aabb::Aabb;
pub use brush::{Brush, Face, Plane};
pub use world::{BodyId, Trace, World};

/// Surfaces whose normal has at least this much +Y are floors: about
/// 45°. Steeper ones are walls, even if sloped.
pub const MIN_WALK_NORMAL: f32 = 0.7;

/// Gap kept between a box and the surfaces it touches, so the next
/// trace starts cleanly outside them.
pub(crate) const DIST_EPSILON: f32 = 0.001;
