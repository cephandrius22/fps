//! Mover state machine: doors and gates sliding between their spawn
//! position and `spawn + travel`.
//!
//! ```text
//!   Closed ──fire──► Opening ──reaches the end──► Open { elapsed }
//!     ▲               ▲   ▲                              │
//!     │       blocked │   │ fire                         │ wait over (After)
//!     │               │   │                              │ or fired (OnFire)
//!     └──reaches──── Closing ◄───────────────────────────┘
//!        spawn
//! ```
//!
//! "Blocked" means the next step would push into the player: a closing
//! mover reopens, an opening one waits in place. Movers never crush.

use glam::Vec3;
use physics::BodyId;

use crate::entity::{Close, Mover};

/// Movers shorter than this (meters) arrive instantly.
const MIN_TRAVEL: f32 = 1e-4;

/// Where a mover is in its cycle.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Motion {
    Closed,
    Opening,
    /// Fully open for `elapsed` seconds.
    Open {
        elapsed: f32,
    },
    Closing,
}

/// Where a mover is in its cycle, as reported outside this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Closed,
    Opening,
    Open,
    Closing,
}

/// What a moving body may not push into this tick.
pub(crate) struct Obstacles<'a> {
    pub physics: &'a mut physics::World,
    /// The player's box; None while the player is not solid (flying).
    pub player: Option<physics::Aabb>,
}

/// One mover's progress: 0 at spawn, 1 at spawn + travel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MoverState {
    spawn: Vec3,
    progress: f32,
    motion: Motion,
}

impl MoverState {
    /// Closed, at `spawn`.
    pub fn new(spawn: Vec3) -> Self {
        Self {
            spawn,
            progress: 0.0,
            motion: Motion::Closed,
        }
    }

    /// Fired: closed or closing movers open; an open toggle closes.
    pub fn fire(&mut self, close: Close) {
        self.motion = match self.motion {
            Motion::Closed | Motion::Closing => Motion::Opening,
            Motion::Open { .. } if close == Close::OnFire => Motion::Closing,
            other => other,
        };
    }

    /// Opens, whatever the state: closed or closing movers start opening;
    /// an open one restarts its wait before closing.
    pub fn open(&mut self) {
        self.motion = match self.motion {
            Motion::Open { .. } => Motion::Open { elapsed: 0.0 },
            _ => Motion::Opening,
        };
    }

    /// Closes, whatever the state: open or opening movers start closing.
    pub fn close(&mut self) {
        if matches!(self.motion, Motion::Open { .. } | Motion::Opening) {
            self.motion = Motion::Closing;
        }
    }

    pub fn phase(&self) -> Phase {
        match self.motion {
            Motion::Closed => Phase::Closed,
            Motion::Opening => Phase::Opening,
            Motion::Open { .. } => Phase::Open,
            Motion::Closing => Phase::Closing,
        }
    }

    /// 0 at spawn, 1 fully open.
    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// Advances by `dt`, moving `bodies` along. A step that would push
    /// any of them into the player is skipped: opening waits, closing
    /// turns back. Returns the new position when the mover moved.
    pub fn step(
        &mut self,
        mover: &Mover,
        bodies: &[BodyId],
        obstacles: &mut Obstacles,
        dt: f32,
    ) -> Option<Vec3> {
        let direction = match self.motion {
            Motion::Closed => return None,
            Motion::Open { elapsed } => {
                self.wait(mover.close, elapsed + dt);
                return None;
            }
            Motion::Opening => 1.0,
            Motion::Closing => -1.0,
        };

        let progress = (self.progress + direction * progress_rate(mover) * dt).clamp(0.0, 1.0);
        let from = self.position(mover, self.progress);
        let to = self.position(mover, progress);

        if !try_move(obstacles, bodies, from, to) {
            if self.motion == Motion::Closing {
                self.motion = Motion::Opening;
            }
            return None;
        }

        self.progress = progress;
        if self.motion == Motion::Opening && progress >= 1.0 {
            self.motion = Motion::Open { elapsed: 0.0 };
        }
        if self.motion == Motion::Closing && progress <= 0.0 {
            self.motion = Motion::Closed;
        }
        Some(to)
    }

    /// Fully open for `elapsed` seconds: start closing once a timed
    /// mover's wait is over.
    fn wait(&mut self, close: Close, elapsed: f32) {
        self.motion = match close {
            Close::After(wait) if elapsed >= wait => Motion::Closing,
            _ => Motion::Open { elapsed },
        };
    }

    fn position(&self, mover: &Mover, progress: f32) -> Vec3 {
        self.spawn + mover.travel * progress
    }
}

/// Share of the full travel covered per second.
fn progress_rate(mover: &Mover) -> f32 {
    let distance = mover.travel.length();
    if distance < MIN_TRAVEL {
        return f32::INFINITY;
    }

    mover.speed / distance
}

/// Moves every body from `from` to `to`. If any then overlaps the
/// player, puts them all back and returns false. Movers without bodies
/// always move.
fn try_move(obstacles: &mut Obstacles, bodies: &[BodyId], from: Vec3, to: Vec3) -> bool {
    let physics = &mut *obstacles.physics;
    for &body in bodies {
        physics.set_offset(body, to);
    }

    let crushing = obstacles
        .player
        .is_some_and(|player| bodies.iter().any(|&body| physics.overlaps(body, &player)));
    if crushing {
        for &body in bodies {
            physics.set_offset(body, from);
        }
    }

    !crushing
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.1;

    /// 1 m up at 1 m/s.
    fn lift(close: Close) -> Mover {
        Mover {
            travel: Vec3::Y,
            speed: 1.0,
            close,
        }
    }

    /// Steps a bodiless mover for `seconds`; returns the last position
    /// it moved to, if it moved at all.
    fn run(state: &mut MoverState, mover: &Mover, seconds: f32) -> Option<Vec3> {
        let mut physics = physics::World::default();
        let mut obstacles = Obstacles {
            physics: &mut physics,
            player: None,
        };

        (0..(seconds / DT).round() as u32)
            .filter_map(|_| state.step(mover, &[], &mut obstacles, DT))
            .last()
    }

    #[test]
    fn fire_opens_closed_and_closing_movers() {
        let mut state = MoverState::new(Vec3::ZERO);
        state.fire(Close::OnFire);
        assert_eq!(state.motion, Motion::Opening);

        state.motion = Motion::Closing;
        state.fire(Close::After(1.0));
        assert_eq!(state.motion, Motion::Opening);
    }

    #[test]
    fn only_toggles_close_when_fired_open() {
        let open = Motion::Open { elapsed: 0.5 };

        let mut toggle = MoverState {
            motion: open,
            ..MoverState::new(Vec3::ZERO)
        };
        toggle.fire(Close::OnFire);
        assert_eq!(toggle.motion, Motion::Closing);

        let mut timed = MoverState {
            motion: open,
            ..MoverState::new(Vec3::ZERO)
        };
        timed.fire(Close::After(3.0));
        assert_eq!(timed.motion, open);
    }

    #[test]
    fn opens_to_spawn_plus_travel_then_waits() {
        let mover = lift(Close::After(5.0));
        let mut state = MoverState::new(Vec3::X);
        state.fire(mover.close);

        assert_eq!(run(&mut state, &mover, 1.5), Some(Vec3::new(1.0, 1.0, 0.0)));
        assert!(matches!(state.motion, Motion::Open { .. }));
    }

    #[test]
    fn timed_mover_closes_back_to_spawn() {
        let mover = lift(Close::After(0.2));
        let mut state = MoverState::new(Vec3::ZERO);
        state.fire(mover.close);

        run(&mut state, &mover, 1.5);
        assert_eq!(run(&mut state, &mover, 2.0), Some(Vec3::ZERO));
        assert_eq!(state.motion, Motion::Closed);
    }

    #[test]
    fn toggle_stays_open_until_fired() {
        let mover = lift(Close::OnFire);
        let mut state = MoverState::new(Vec3::ZERO);
        state.fire(mover.close);
        run(&mut state, &mover, 1.5);

        assert_eq!(run(&mut state, &mover, 5.0), None);
        state.fire(mover.close);
        assert_eq!(run(&mut state, &mover, 1.5), Some(Vec3::ZERO));
    }

    #[test]
    fn open_and_close_override_the_cycle() {
        let mover = lift(Close::OnFire);
        let mut state = MoverState::new(Vec3::ZERO);

        // Closing a closed mover does nothing; open starts it.
        state.close();
        assert_eq!(state.phase(), Phase::Closed);
        state.open();
        run(&mut state, &mover, 0.5);
        assert_eq!(state.phase(), Phase::Opening);

        // Close mid-way: it turns back from where it is.
        let halfway = state.progress();
        state.close();
        run(&mut state, &mover, 0.2);
        assert_eq!(state.phase(), Phase::Closing);
        assert!(state.progress() < halfway);
    }

    #[test]
    fn open_on_an_open_timed_mover_restarts_its_wait() {
        let mover = lift(Close::After(1.0));
        let mut state = MoverState::new(Vec3::ZERO);
        state.open();
        run(&mut state, &mover, 1.5);

        // 0.5 s into a 1 s wait: reopening buys another full second.
        state.open();
        run(&mut state, &mover, 0.8);
        assert_eq!(state.phase(), Phase::Open);
        run(&mut state, &mover, 0.5);
        assert_eq!(state.phase(), Phase::Closing);
    }

    #[test]
    fn zero_travel_arrives_at_once() {
        let mover = Mover {
            travel: Vec3::ZERO,
            ..lift(Close::OnFire)
        };
        assert_eq!(progress_rate(&mover), f32::INFINITY);
    }
}
