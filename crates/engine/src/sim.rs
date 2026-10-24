//! Simulation stepping: runs the game's fixed ticks for one frame.

use platform::Flow;

use crate::Game;
use crate::input::{CursorMode, Input};

/// One simulation step, handed to `Game::tick`.
pub struct Tick<'a> {
    /// Input as of this tick.
    pub input: &'a Input,
    /// Seconds per tick (fixed).
    pub dt: f32,
    quit: bool,
    cursor: CursorMode,
}

impl Tick<'_> {
    /// Ends the game after this tick.
    pub fn quit(&mut self) {
        self.quit = true;
    }

    /// Current cursor mode. The engine resets it to `Free` when the
    /// window loses focus.
    pub fn cursor(&self) -> CursorMode {
        self.cursor
    }

    /// Requests a cursor mode; applied to the window after this frame's
    /// ticks.
    pub fn set_cursor(&mut self, mode: CursorMode) {
        self.cursor = mode;
    }
}

/// Runs `ticks` steps. Input edges are cleared after each, so only the
/// first tick of a frame sees a fresh press. Stops early on quit.
/// `cursor` carries the game's cursor requests in and out.
pub(crate) fn run_ticks(
    game: &mut impl Game,
    input: &mut Input,
    cursor: &mut CursorMode,
    ticks: u32,
    dt: f32,
) -> Flow {
    for _ in 0..ticks {
        let mut tick = Tick {
            input,
            dt,
            quit: false,
            cursor: *cursor,
        };
        game.tick(&mut tick);
        let quit = tick.quit;
        *cursor = tick.cursor;

        input.end_tick();

        if quit {
            return Flow::Exit;
        }
    }

    Flow::Continue
}

#[cfg(test)]
mod tests {
    use base::{BoxError, Key};
    use platform::{Event, KeyState};

    use super::*;
    use crate::{Frame, Gfx};

    /// Records what each tick saw; quits or captures on configured ticks.
    #[derive(Default)]
    struct Recorder {
        pressed: Vec<bool>,
        dts: Vec<f32>,
        cursors: Vec<CursorMode>,
        quit_on: Option<usize>,
        capture_on: Option<usize>,
    }

    impl Game for Recorder {
        fn init(&mut self, _: &mut Gfx) -> Result<(), BoxError> {
            Ok(())
        }

        fn tick(&mut self, tick: &mut Tick) {
            self.pressed.push(tick.input.was_pressed(Key::Space));
            self.dts.push(tick.dt);
            self.cursors.push(tick.cursor());

            let n = Some(self.pressed.len());
            if self.capture_on == n {
                tick.set_cursor(CursorMode::Captured);
            }
            if self.quit_on == n {
                tick.quit();
            }
        }

        fn draw(&mut self, _: &mut Frame) {}
    }

    /// `run_ticks` with a free cursor.
    fn run(game: &mut Recorder, input: &mut Input, ticks: u32, dt: f32) -> Flow {
        run_ticks(game, input, &mut CursorMode::Free, ticks, dt)
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
        let flow = run(&mut game, &mut Input::default(), 3, 0.5);

        assert_eq!(flow, Flow::Continue);
        assert_eq!(game.dts, [0.5, 0.5, 0.5]);
    }

    #[test]
    fn only_first_tick_sees_a_press() {
        let mut game = Recorder::default();
        run(&mut game, &mut pressed_space(), 3, 0.1);

        assert_eq!(game.pressed, [true, false, false]);
    }

    #[test]
    fn press_survives_zero_tick_frames() {
        let mut game = Recorder::default();
        let mut input = pressed_space();

        run(&mut game, &mut input, 0, 0.1);
        run(&mut game, &mut input, 1, 0.1);

        assert_eq!(game.pressed, [true]);
    }

    #[test]
    fn quit_stops_remaining_ticks() {
        let mut game = Recorder {
            quit_on: Some(2),
            ..Default::default()
        };
        let flow = run(&mut game, &mut Input::default(), 5, 0.1);

        assert_eq!(flow, Flow::Exit);
        assert_eq!(game.pressed.len(), 2);
    }

    #[test]
    fn cursor_request_carries_to_later_ticks_and_out() {
        let mut game = Recorder {
            capture_on: Some(1),
            ..Default::default()
        };
        let mut cursor = CursorMode::Free;

        run_ticks(&mut game, &mut Input::default(), &mut cursor, 2, 0.1);

        assert_eq!(game.cursors, [CursorMode::Free, CursorMode::Captured]);
        assert_eq!(cursor, CursorMode::Captured);
    }
}
