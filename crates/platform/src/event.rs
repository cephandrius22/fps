//! OS events translated to engine vocabulary.

use base::{Button, Key};
use winit::event::{ElementState, MouseButton};
use winit::keyboard::KeyCode;

/// Whether a key or button went down or up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    /// Went down (also sent on OS key repeat).
    Pressed,
    /// Went up.
    Released,
}

/// Something the OS reported. Keys without an engine mapping are dropped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Drawable size changed, in physical pixels. May be 0×0 when minimized.
    Resized {
        /// New width.
        width: u32,
        /// New height.
        height: u32,
    },
    /// The window lost keyboard focus; held keys will not report release.
    FocusLost,
    /// A keyboard key changed state.
    Key {
        /// Which key.
        key: Key,
        /// Down or up.
        state: KeyState,
    },
    /// A mouse button changed state.
    Button {
        /// Which button.
        button: Button,
        /// Down or up.
        state: KeyState,
    },
    /// Raw mouse motion in device units, unaffected by cursor
    /// acceleration or screen edges.
    MouseMotion {
        /// Horizontal motion, positive right.
        dx: f32,
        /// Vertical motion, positive down.
        dy: f32,
    },
}

pub(crate) fn key_state(state: ElementState) -> KeyState {
    match state {
        ElementState::Pressed => KeyState::Pressed,
        ElementState::Released => KeyState::Released,
    }
}

pub(crate) fn map_button(button: MouseButton) -> Option<Button> {
    match button {
        MouseButton::Left => Some(Button::Left),
        MouseButton::Right => Some(Button::Right),
        MouseButton::Middle => Some(Button::Middle),
        _ => None,
    }
}

pub(crate) fn map_key(code: KeyCode) -> Option<Key> {
    let key = match code {
        KeyCode::KeyW => Key::W,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyF => Key::F,
        KeyCode::Space => Key::Space,
        KeyCode::ShiftLeft => Key::LShift,
        KeyCode::ControlLeft => Key::LCtrl,
        KeyCode::Tab => Key::Tab,
        KeyCode::Escape => Key::Escape,
        KeyCode::Enter => Key::Enter,
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::F1 => Key::F1,
        KeyCode::F2 => Key::F2,
        KeyCode::F3 => Key::F3,
        KeyCode::F4 => Key::F4,
        KeyCode::F12 => Key::F12,
        _ => return None,
    };

    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_keys_and_drops_others() {
        assert_eq!(map_key(KeyCode::KeyW), Some(Key::W));
        assert_eq!(map_key(KeyCode::Escape), Some(Key::Escape));
        assert_eq!(map_key(KeyCode::F12), Some(Key::F12));
        assert_eq!(map_key(KeyCode::KeyZ), None);
    }

    #[test]
    fn maps_buttons_and_drops_extra_buttons() {
        assert_eq!(map_button(MouseButton::Left), Some(Button::Left));
        assert_eq!(map_button(MouseButton::Back), None);
    }

    #[test]
    fn maps_element_state() {
        assert_eq!(key_state(ElementState::Pressed), KeyState::Pressed);
        assert_eq!(key_state(ElementState::Released), KeyState::Released);
    }
}
