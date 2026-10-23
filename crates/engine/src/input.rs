//! Input state, built from platform events and sampled per simulation
//! tick.
//!
//! "Pressed" edges and mouse delta accumulate until `end_tick`, so a tap
//! between ticks is never lost and never seen twice.

use base::{Button, Key};
use platform::{Event, KeyState};

/// Keyboard and mouse state as seen by one simulation tick.
#[derive(Default)]
pub struct Input {
    keys_down: [bool; Key::COUNT],
    keys_hit: [bool; Key::COUNT],
    buttons_down: [bool; Button::COUNT],
    buttons_hit: [bool; Button::COUNT],
    mouse_delta: (f32, f32),
}

impl Input {
    /// True while the key is held.
    pub fn is_down(&self, key: Key) -> bool {
        self.keys_down[key as usize]
    }

    /// True on the first tick after the key went down.
    pub fn was_pressed(&self, key: Key) -> bool {
        self.keys_hit[key as usize]
    }

    /// True while the button is held.
    pub fn button_down(&self, button: Button) -> bool {
        self.buttons_down[button as usize]
    }

    /// True on the first tick after the button went down.
    pub fn button_pressed(&self, button: Button) -> bool {
        self.buttons_hit[button as usize]
    }

    /// Raw mouse motion since the last tick, in device units.
    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
    }

    /// Folds one platform event into the state. Non-input events are ignored.
    pub(crate) fn apply(&mut self, event: Event) {
        match event {
            Event::Key { key, state } => {
                let i = key as usize;
                press(&mut self.keys_down[i], &mut self.keys_hit[i], state);
            }
            Event::Button { button, state } => {
                let i = button as usize;
                press(&mut self.buttons_down[i], &mut self.buttons_hit[i], state);
            }
            Event::MouseMotion { dx, dy } => {
                self.mouse_delta.0 += dx;
                self.mouse_delta.1 += dy;
            }
            // Releases go unreported once focus is gone; drop held state
            // so keys do not stick.
            Event::FocusLost => {
                self.keys_down = [false; Key::COUNT];
                self.buttons_down = [false; Button::COUNT];
            }
            Event::Resized { .. } => {}
        }
    }

    /// Clears per-tick edges. Call after each simulation tick.
    pub(crate) fn end_tick(&mut self) {
        self.keys_hit = [false; Key::COUNT];
        self.buttons_hit = [false; Button::COUNT];
        self.mouse_delta = (0.0, 0.0);
    }
}

/// Updates one key/button. OS key repeat re-sends Pressed while held;
/// only the first counts as a hit.
fn press(down: &mut bool, hit: &mut bool, state: KeyState) {
    let pressed = state == KeyState::Pressed;

    if pressed && !*down {
        *hit = true;
    }
    *down = pressed;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, state: KeyState) -> Event {
        Event::Key { key, state }
    }

    #[test]
    fn press_edge_lasts_one_tick() {
        let mut input = Input::default();
        input.apply(key(Key::Space, KeyState::Pressed));
        assert!(input.was_pressed(Key::Space));

        input.end_tick();
        assert!(!input.was_pressed(Key::Space));
        assert!(input.is_down(Key::Space));
    }

    #[test]
    fn key_repeat_does_not_retrigger() {
        let mut input = Input::default();
        input.apply(key(Key::W, KeyState::Pressed));
        input.end_tick();

        input.apply(key(Key::W, KeyState::Pressed));
        assert!(!input.was_pressed(Key::W));
    }

    #[test]
    fn tap_between_ticks_is_still_seen() {
        let mut input = Input::default();
        input.apply(key(Key::E, KeyState::Pressed));
        input.apply(key(Key::E, KeyState::Released));

        assert!(input.was_pressed(Key::E));
        assert!(!input.is_down(Key::E));
    }

    #[test]
    fn buttons_track_like_keys() {
        let mut input = Input::default();
        input.apply(Event::Button {
            button: Button::Left,
            state: KeyState::Pressed,
        });

        assert!(input.button_pressed(Button::Left));
        assert!(input.button_down(Button::Left));
        assert!(!input.button_down(Button::Right));
    }

    #[test]
    fn mouse_delta_accumulates_until_tick_end() {
        let mut input = Input::default();
        input.apply(Event::MouseMotion { dx: 1.0, dy: 2.0 });
        input.apply(Event::MouseMotion { dx: 3.0, dy: -1.0 });
        assert_eq!(input.mouse_delta(), (4.0, 1.0));

        input.end_tick();
        assert_eq!(input.mouse_delta(), (0.0, 0.0));
    }

    #[test]
    fn focus_loss_releases_everything() {
        let mut input = Input::default();
        input.apply(key(Key::A, KeyState::Pressed));
        input.apply(Event::FocusLost);

        assert!(!input.is_down(Key::A));
    }
}
