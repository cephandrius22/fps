//! OS event loop. Translates OS events for the handler and asks it for
//! a frame on every redraw.
//!
//!   OS events ─► Runner ─► Handler::event(Event)
//!                  └─ redraw ─► Handler::frame ─► Flow

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::WindowId;

use crate::event::{Event, key_state, map_button, map_key};
use crate::window::Window;

/// The OS side failed.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// No event loop: no display, or created twice.
    #[error("create event loop")]
    EventLoop(#[from] winit::error::EventLoopError),
    /// The OS refused to create the window.
    #[error("create window")]
    CreateWindow(#[from] winit::error::OsError),
    /// The loop ended before the OS ever resumed the app.
    #[error("event loop exited before window creation")]
    NoWindow,
    /// No cursor grab mode is supported here (e.g. some remote desktops).
    #[error("capture cursor")]
    CursorGrab(#[source] winit::error::ExternalError),
}

/// Why [`run`] stopped with an error. `E` is the handler's init error.
#[derive(Debug, thiserror::Error)]
pub enum RunError<E> {
    /// The OS side failed.
    #[error(transparent)]
    Platform(#[from] PlatformError),
    /// [`Handler::init`] failed.
    #[error("handler init failed")]
    Init(#[source] E),
}

/// Initial window settings.
pub struct WindowConfig {
    /// Title bar text.
    pub title: String,
    /// Logical (DPI-independent) width.
    pub width: u32,
    /// Logical (DPI-independent) height.
    pub height: u32,
}

/// What the event loop should do after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Keep running.
    Continue,
    /// Close the window and return from [`run`].
    Exit,
}

/// Implemented by the layer above (the engine).
pub trait Handler {
    /// The handler's own init failure type.
    type Error;

    /// Window is ready. An error aborts [`run`] as [`RunError::Init`].
    fn init(&mut self, window: Window) -> Result<(), Self::Error>;
    /// An OS event arrived. Called between frames.
    fn event(&mut self, event: Event);
    /// Produce one frame.
    fn frame(&mut self) -> Flow;
}

/// Blocks until the window closes or the handler returns `Flow::Exit`.
pub fn run<H: Handler>(config: WindowConfig, handler: H) -> Result<(), RunError<H::Error>> {
    let event_loop = EventLoop::new().map_err(PlatformError::from)?;
    let mut runner = Runner {
        config,
        handler,
        window: None,
        error: None,
    };

    event_loop
        .run_app(&mut runner)
        .map_err(PlatformError::from)?;

    match runner.error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

struct Runner<H: Handler> {
    config: WindowConfig,
    handler: H,
    window: Option<Arc<winit::window::Window>>,
    error: Option<RunError<H::Error>>,
}

impl<H: Handler> Runner<H> {
    fn fail(&mut self, event_loop: &ActiveEventLoop, err: RunError<H::Error>) {
        self.error = Some(err);
        event_loop.exit();
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), RunError<H::Error>> {
        let size = LogicalSize::new(self.config.width, self.config.height);
        let attrs = winit::window::Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(size);

        let window = event_loop
            .create_window(attrs)
            .map_err(PlatformError::from)?;
        let window = Arc::new(window);
        self.handler
            .init(Window(window.clone()))
            .map_err(RunError::Init)?;

        window.request_redraw();
        self.window = Some(window);

        Ok(())
    }
}

impl<H: Handler> ApplicationHandler for Runner<H> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Desktop resumes once; mobile may resume repeatedly.
        if self.window.is_some() {
            return;
        }

        if let Err(err) = self.create_window(event_loop) {
            self.fail(event_loop, err);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let translated = match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }

            WindowEvent::RedrawRequested => {
                self.redraw(event_loop);
                return;
            }

            WindowEvent::Resized(size) => Some(Event::Resized {
                width: size.width,
                height: size.height,
            }),

            WindowEvent::Focused(false) => Some(Event::FocusLost),

            WindowEvent::KeyboardInput { event, .. } => match event.physical_key {
                PhysicalKey::Code(code) => map_key(code).map(|key| Event::Key {
                    key,
                    state: key_state(event.state),
                }),
                PhysicalKey::Unidentified(_) => None,
            },

            WindowEvent::MouseInput { state, button, .. } => {
                map_button(button).map(|button| Event::Button {
                    button,
                    state: key_state(state),
                })
            }

            _ => None,
        };

        if let Some(event) = translated {
            self.handler.event(event);
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.handler.event(Event::MouseMotion {
                dx: delta.0 as f32,
                dy: delta.1 as f32,
            });
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        if self.window.is_none() && self.error.is_none() {
            self.error = Some(PlatformError::NoWindow.into());
        }
    }
}

impl<H: Handler> Runner<H> {
    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.clone() else {
            return;
        };

        match self.handler.frame() {
            Flow::Continue => window.request_redraw(),
            Flow::Exit => event_loop.exit(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("game broke")]
    struct GameError;

    fn assert_send_sync<T: Send + Sync + 'static>() {}

    #[test]
    fn errors_are_thread_safe() {
        assert_send_sync::<PlatformError>();
        assert_send_sync::<RunError<GameError>>();
    }

    #[test]
    fn init_error_keeps_handler_cause() {
        let err: RunError<GameError> = RunError::Init(GameError);
        let source = std::error::Error::source(&err).unwrap();

        assert_eq!(source.to_string(), "game broke");
        assert_eq!(
            base::chain(&err).to_string(),
            "handler init failed: game broke"
        );
    }

    #[test]
    fn platform_error_is_transparent() {
        let err: RunError<GameError> = PlatformError::NoWindow.into();
        assert_eq!(err.to_string(), "event loop exited before window creation");
    }
}
