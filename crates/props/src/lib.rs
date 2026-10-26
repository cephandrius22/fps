//! Prop models: glTF binary (.glb) files read into plain meshes and
//! textures, in model space (meters, Y up: the engine's own axes, so
//! no conversion). The engine turns them into draws and collision.
//!
//! ```text
//!   .glb ─► read() ─► Prop { meshes (one per material), textures }
//!   Prop ─► write() ─► .glb        (for models made in code)
//! ```
//!
//! Only self-contained .glb with PNG textures: export from Blender as
//! "glTF Binary". Not yet: skinning, animation, JPEG textures.
//!
//! Modules:
//!
//! ```text
//!   lib.rs     re-exports
//!   model.rs   Prop, Mesh
//!   read.rs    read(): the scene's meshes, transforms applied
//!   write.rs   write(): one .glb, no external files
//!   error.rs   PropError
//! ```

mod error;
mod model;
mod read;
mod write;

pub use error::PropError;
pub use model::{Mesh, Prop};
pub use read::read;
pub use write::write;
