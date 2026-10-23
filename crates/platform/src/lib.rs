//! OS layer. Hides winit behind engine-owned types so the rest of the
//! engine never sees the windowing library.

mod clock;
mod input;
mod run;
mod window;

pub use clock::FixedStep;
pub use input::{Button, Input, Key};
pub use run::{Flow, Handler, WindowConfig, run};
pub use window::Window;
