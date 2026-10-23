//! Simulation stepping: runs the game's fixed ticks for one frame.

use platform::Flow;

use crate::Game;
use crate::input::Input;

/// One simulation step, handed to `Game::tick`.
pub struct Tick<'a> {
    /// Input as of this tick.
    pub input: &'a Input,
    /// Seconds per tick (fixed).
    pub dt: f32,
    quit: bool,
}

impl Tick<'_> {
    /// Ends the game after this tick.
    pub fn quit(&mut self) {
        self.quit = true;
    }
}

/// Runs `ticks` steps. Input edges are cleared after each, so only the
/// first tick of a frame sees a fresh press. Stops early on quit.
pub(crate) fn run_ticks(game: &mut impl Game, input: &mut Input, ticks: u32, dt: f32) -> Flow {
    for _ in 0..ticks {
        let mut tick = Tick {
            input,
            dt,
            quit: false,
        };
        game.tick(&mut tick);
        let quit = tick.quit;

        input.end_tick();

        if quit {
            return Flow::Exit;
        }
    }

    Flow::Continue
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use base::Key;
    use platform::{Event, KeyState};

    use super::*;
    use crate::{Frame, Gfx};

    /// Records what each tick saw; quits on the configured tick.
    #[derive(Default)]
    struct Recorder {
        pressed: Vec<bool>,
        dts: Vec<f32>,
        quit_on: Option<usize>,
    }

    impl Game for Recorder {
        fn init(&mut self, _: &mut Gfx) -> Result<()> {
            Ok(())
        }

        fn tick(&mut self, tick: &mut Tick) {
            self.pressed.push(tick.input.was_pressed(Key::Space));
            self.dts.push(tick.dt);

            if self.quit_on == Some(self.pressed.len()) {
                tick.quit();
            }
        }

        fn draw(&mut self, _: &mut Frame) {}
    }

    fn pressed_space() -> Input {
        let mut input = Input::default();
        input.apply(Event::Key {
            key: Key::Space,
            state: KeyState::Pressed,
        });
        input
    }

    #[test]
    fn runs_each_tick_with_fixed_dt() {
        let mut game = Recorder::default();
        let flow = run_ticks(&mut game, &mut Input::default(), 3, 0.5);

        assert_eq!(flow, Flow::Continue);
        assert_eq!(game.dts, [0.5, 0.5, 0.5]);
    }

    #[test]
    fn only_first_tick_sees_a_press() {
        let mut game = Recorder::default();
        run_ticks(&mut game, &mut pressed_space(), 3, 0.1);

        assert_eq!(game.pressed, [true, false, false]);
    }

    #[test]
    fn press_survives_zero_tick_frames() {
        let mut game = Recorder::default();
        let mut input = pressed_space();

        run_ticks(&mut game, &mut input, 0, 0.1);
        run_ticks(&mut game, &mut input, 1, 0.1);

        assert_eq!(game.pressed, [true]);
    }

    #[test]
    fn quit_stops_remaining_ticks() {
        let mut game = Recorder {
            quit_on: Some(2),
            ..Default::default()
        };
        let flow = run_ticks(&mut game, &mut Input::default(), 5, 0.1);

        assert_eq!(flow, Flow::Exit);
        assert_eq!(game.pressed.len(), 2);
    }
}
