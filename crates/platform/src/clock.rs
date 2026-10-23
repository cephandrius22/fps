//! Fixed-timestep clock. Simulation advances in whole ticks; rendering
//! interpolates between the last two ticks with `alpha`.
//!
//!   frame time ─► accumulator ─► N ticks of `dt` ─► leftover / dt = alpha

use std::time::{Duration, Instant};

/// Caps catch-up after a stall (breakpoint, window drag) to avoid a
/// spiral of death.
const MAX_FRAME: Duration = Duration::from_millis(250);

pub struct FixedStep {
    step: Duration,
    accumulator: Duration,
    last: Instant,
}

impl FixedStep {
    pub fn new(hz: u32) -> Self {
        Self {
            step: Duration::from_secs(1) / hz.max(1),
            accumulator: Duration::ZERO,
            last: Instant::now(),
        }
    }

    /// Consumes elapsed real time; returns how many ticks to run.
    pub fn advance(&mut self) -> u32 {
        let now = Instant::now();
        let frame = (now - self.last).min(MAX_FRAME);
        self.last = now;
        self.accumulator += frame;

        let mut ticks = 0;
        while self.accumulator >= self.step {
            self.accumulator -= self.step;
            ticks += 1;
        }

        ticks
    }

    /// Seconds per tick.
    pub fn dt(&self) -> f32 {
        self.step.as_secs_f32()
    }

    /// Progress toward the next tick, 0..1.
    pub fn alpha(&self) -> f32 {
        self.accumulator.as_secs_f32() / self.step.as_secs_f32()
    }
}
