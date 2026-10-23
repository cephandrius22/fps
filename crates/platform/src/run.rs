//! OS event loop. Collects input, forwards resize, and asks the handler
//! for a frame on every redraw.
//!
//!   OS events ─► Runner ─► Input ─┐
//!                  │              ▼
//!                  └─ redraw ─► Handler::frame ─► Flow

use std::sync::Arc;

use anyhow::{Result, anyhow};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::WindowId;

use crate::input::{Button, Input, KeyState, map_key};
use crate::window::Window;

pub struct WindowConfig {
    pub title: String,
    /// Logical (DPI-independent) size.
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
}

/// Implemented by the layer above (the engine).
pub trait Handler {
    fn init(&mut self, window: Window) -> Result<()>;
    fn resized(&mut self, width: u32, height: u32);
    fn frame(&mut self, input: &mut Input) -> Flow;
}

/// Blocks until the window closes or the handler returns `Flow::Exit`.
pub fn run(config: WindowConfig, handler: impl Handler) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let mut runner = Runner {
        config,
        handler,
        window: None,
        input: Input::default(),
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
    input: Input,
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
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => self.handler.resized(size.width, size.height),

            WindowEvent::Focused(false) => self.input.release_all(),

            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let Some(key) = map_key(code) else {
                    return;
                };

                self.input.set_key(key, key_state(event.state));
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = map_button(button) else {
                    return;
                };

                self.input.set_button(button, key_state(state));
            }

            WindowEvent::RedrawRequested => {
                let Some(window) = self.window.clone() else {
                    return;
                };

                match self.handler.frame(&mut self.input) {
                    Flow::Continue => window.request_redraw(),
                    Flow::Exit => event_loop.exit(),
                }
            }

            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        // Raw motion: unaffected by cursor acceleration or screen edges.
        if let DeviceEvent::MouseMotion { delta } = event {
            self.input.add_mouse_delta(delta.0 as f32, delta.1 as f32);
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        if self.window.is_none() && self.error.is_none() {
            self.error = Some(anyhow!("event loop exited before window creation"));
        }
    }
}

fn key_state(state: ElementState) -> KeyState {
    match state {
        ElementState::Pressed => KeyState::Pressed,
        ElementState::Released => KeyState::Released,
    }
}

fn map_button(button: MouseButton) -> Option<Button> {
    match button {
        MouseButton::Left => Some(Button::Left),
        MouseButton::Right => Some(Button::Right),
        MouseButton::Middle => Some(Button::Middle),
        _ => None,
    }
}
