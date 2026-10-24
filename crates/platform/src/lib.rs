//! OS layer. Hides winit behind engine-owned types so nothing above
//! this crate sees the windowing library. Reports what happened
//! (events); deciding what it means (input state) is the engine's job.

mod clock;
mod event;
mod run;
mod window;

pub use clock::FixedStep;
pub use event::{Event, KeyState};
pub use run::{Flow, Handler, PlatformError, RunError, WindowConfig, run};
pub use window::{CursorMode, Window};
