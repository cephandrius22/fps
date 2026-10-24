//! Shareable window handle. Exposes what a GPU surface needs (raw
//! handles), size, and cursor capture for mouse look.

use std::sync::Arc;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
use winit::window::CursorGrabMode;

use crate::run::PlatformError;

/// What the mouse cursor does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorMode {
    /// Visible and free to leave the window. Menus, editors.
    Free,
    /// Hidden and held by the window; only raw motion is reported.
    /// Mouse look.
    Captured,
}

/// Cheap-to-clone handle to the game window.
#[derive(Clone, Debug)]
pub struct Window(pub(crate) Arc<winit::window::Window>);

impl Window {
    /// Drawable size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        let size = self.0.inner_size();
        (size.width, size.height)
    }

    /// Captures or frees the cursor. Tries each grab mode the platform
    /// might support; fails only if none works.
    pub fn set_cursor(&self, mode: CursorMode) -> Result<(), PlatformError> {
        let mut last_err = None;

        for &grab in grab_modes(mode) {
            match self.0.set_cursor_grab(grab) {
                Ok(()) => {
                    last_err = None;
                    break;
                }
                Err(err) => last_err = Some(err),
            }
        }

        if let Some(err) = last_err {
            return Err(PlatformError::CursorGrab(err));
        }

        self.0.set_cursor_visible(mode == CursorMode::Free);
        Ok(())
    }
}

/// Grab modes to try, best first. Locked pins the cursor in place
/// (macOS, Wayland); Confined keeps it inside the window (Windows, X11).
fn grab_modes(mode: CursorMode) -> &'static [CursorGrabMode] {
    match mode {
        CursorMode::Free => &[CursorGrabMode::None],
        CursorMode::Captured => &[CursorGrabMode::Locked, CursorGrabMode::Confined],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_prefers_locked_then_confined() {
        assert_eq!(
            grab_modes(CursorMode::Captured),
            [CursorGrabMode::Locked, CursorGrabMode::Confined]
        );
    }

    #[test]
    fn free_releases_grab() {
        assert_eq!(grab_modes(CursorMode::Free), [CursorGrabMode::None]);
    }
}
