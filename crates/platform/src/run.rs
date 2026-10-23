//! OS event loop. Translates OS events for the handler and asks it for
//! a frame on every redraw.
//!
//!   OS events ─► Runner ─► Handler::event(Event)
//!                  └─ redraw ─► Handler::frame ─► Flow

use std::sync::Arc;

use anyhow::{Result, anyhow};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::WindowId;

use crate::event::{Event, key_state, map_button, map_key};
use crate::window::Window;

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
    /// Window is ready. An error aborts [`run`] and is returned from it.
    fn init(&mut self, window: Window) -> Result<()>;
    /// An OS event arrived. Called between frames.
    fn event(&mut self, event: Event);
    /// Produce one frame.
    fn frame(&mut self) -> Flow;
}

/// Blocks until the window closes or the handler returns `Flow::Exit`.
pub fn run(config: WindowConfig, handler: impl Handler) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let mut runner = Runner {
        config,
        handler,
        window: None,
        error: None,
    };

    event_loop.run_app(&mut runner)?;

    match runner.error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

struct Runner<H> {
    config: WindowConfig,
    handler: H,
    window: Option<Arc<winit::window::Window>>,
    error: Option<anyhow::Error>,
}

impl<H: Handler> Runner<H> {
    fn fail(&mut self, event_loop: &ActiveEventLoop, err: anyhow::Error) {
        self.error = Some(err);
        event_loop.exit();
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let size = LogicalSize::new(self.config.width, self.config.height);
        let attrs = winit::window::Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(size);

        let window = Arc::new(event_loop.create_window(attrs)?);
        self.handler.init(Window(window.clone()))?;

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
            self.error = Some(anyhow!("event loop exited before window creation"));
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
