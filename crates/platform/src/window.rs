//! Shareable window handle. Exposes only what a GPU surface needs (raw
//! handles) plus size.

use std::sync::Arc;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

/// Cheap-to-clone handle to the game window.
#[derive(Clone, Debug)]
pub struct Window(pub(crate) Arc<winit::window::Window>);

impl Window {
    /// Drawable size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        let size = self.0.inner_size();
        (size.width, size.height)
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.0.display_handle()
    }
}
