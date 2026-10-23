//! Input state, sampled per simulation tick.
//!
//! "Pressed" edges and mouse delta accumulate until `end_tick`, so a tap
//! between ticks is never lost and never seen twice.

use winit::keyboard::KeyCode;

/// Physical keys the engine knows about. Layout-independent (WASD stays
/// WASD on AZERTY).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Key {
    W,
    A,
    S,
    D,
    Q,
    E,
    R,
    F,
    Space,
    LShift,
    LCtrl,
    Tab,
    Escape,
    Enter,
    Up,
    Down,
    Left,
    Right,
    F1,
    F2,
    F3,
    F4,
    F12,
}

const KEY_COUNT: usize = Key::F12 as usize + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Button {
    Left,
    Right,
    Middle,
}

const BUTTON_COUNT: usize = Button::Middle as usize + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyState {
    Pressed,
    Released,
}

#[derive(Default)]
pub struct Input {
    keys_down: [bool; KEY_COUNT],
    keys_hit: [bool; KEY_COUNT],
    buttons_down: [bool; BUTTON_COUNT],
    buttons_hit: [bool; BUTTON_COUNT],
    mouse_delta: (f32, f32),
}

impl Input {
    pub fn is_down(&self, key: Key) -> bool {
        self.keys_down[key as usize]
    }

    /// True on the first tick after the key went down.
    pub fn was_pressed(&self, key: Key) -> bool {
        self.keys_hit[key as usize]
    }

    pub fn button_down(&self, button: Button) -> bool {
        self.buttons_down[button as usize]
    }

    pub fn button_pressed(&self, button: Button) -> bool {
        self.buttons_hit[button as usize]
    }

    /// Raw mouse motion since the last tick, in device units.
    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
    }

    /// Clears per-tick edges. Call after each simulation tick.
    pub fn end_tick(&mut self) {
        self.keys_hit = [false; KEY_COUNT];
        self.buttons_hit = [false; BUTTON_COUNT];
        self.mouse_delta = (0.0, 0.0);
    }

    pub(crate) fn set_key(&mut self, key: Key, state: KeyState) {
        let i = key as usize;
        let down = state == KeyState::Pressed;

        if down && !self.keys_down[i] {
            self.keys_hit[i] = true;
        }
        self.keys_down[i] = down;
    }

    pub(crate) fn set_button(&mut self, button: Button, state: KeyState) {
        let i = button as usize;
        let down = state == KeyState::Pressed;

        if down && !self.buttons_down[i] {
            self.buttons_hit[i] = true;
        }
        self.buttons_down[i] = down;
    }

    pub(crate) fn add_mouse_delta(&mut self, dx: f32, dy: f32) {
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
    }

    /// Drops held state, e.g. when the window loses focus.
    pub(crate) fn release_all(&mut self) {
        self.keys_down = [false; KEY_COUNT];
        self.buttons_down = [false; BUTTON_COUNT];
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
    fn press_edge_lasts_one_tick() {
        let mut input = Input::default();
        input.set_key(Key::Space, KeyState::Pressed);
        assert!(input.was_pressed(Key::Space));

        input.end_tick();
        assert!(!input.was_pressed(Key::Space));
        assert!(input.is_down(Key::Space));

        // Key repeat must not re-trigger the edge.
        input.set_key(Key::Space, KeyState::Pressed);
        assert!(!input.was_pressed(Key::Space));
    }
}
