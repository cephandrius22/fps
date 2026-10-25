//! OS layer. Hides winit behind engine-owned types so nothing above
//! this crate sees the windowing library. Reports what happened
//! (events); deciding what it means (input state) is the engine's job.
//!
//! Modules:
//!
//! ```text
//!   run.rs      run(), Handler, WindowConfig, Flow: the OS event loop
//!   event.rs    Event, KeyState: OS input in engine vocabulary
//!   window.rs   Window, CursorMode: size, cursor capture, raw GPU handles
//!   clock.rs    FixedStep: fixed-rate ticks with a stall cap
//! ```

mod clock;
mod event;
mod run;
mod window;

pub use clock::FixedStep;
pub use event::{Event, KeyState};
pub use run::{Flow, Handler, PlatformError, RunError, WindowConfig, run};
pub use window::{CursorMode, Window};
