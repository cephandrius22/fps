//! Fixed-timestep clock. Simulation advances in whole ticks; rendering
//! interpolates between the last two ticks with `alpha`.
//!
//!   frame time ─► accumulator ─► N ticks of `dt` ─► leftover / dt = alpha

use std::time::{Duration, Instant};

/// Caps catch-up after a stall (breakpoint, window drag) to avoid a
/// spiral of death.
const MAX_FRAME: Duration = Duration::from_millis(250);

/// Converts real elapsed time into a whole number of simulation ticks.
pub struct FixedStep {
    step: Duration,
    accumulator: Duration,
    last: Instant,
}

impl FixedStep {
    /// Clock ticking `hz` times per second. Starts counting now.
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
        let elapsed = now - self.last;
        self.last = now;

        self.advance_by(elapsed)
    }

    /// Seconds per tick.
    pub fn dt(&self) -> f32 {
        self.step.as_secs_f32()
    }

    /// Progress toward the next tick, 0..1.
    pub fn alpha(&self) -> f32 {
        self.accumulator.as_secs_f32() / self.step.as_secs_f32()
    }

    /// Time-source-free core of `advance`, so tests control time.
    fn advance_by(&mut self, elapsed: Duration) -> u32 {
        self.accumulator += elapsed.min(MAX_FRAME);

        let mut ticks = 0;
        while self.accumulator >= self.step {
            self.accumulator -= self.step;
            ticks += 1;
        }

        ticks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HZ: u32 = 100;
    const STEP: Duration = Duration::from_millis(10);

    #[test]
    fn whole_steps_become_ticks() {
        let mut clock = FixedStep::new(HZ);

        assert_eq!(clock.advance_by(STEP * 3), 3);
        assert!(clock.alpha() < 1e-6);
    }

    #[test]
    fn remainder_carries_into_alpha_and_next_frame() {
        let mut clock = FixedStep::new(HZ);

        assert_eq!(clock.advance_by(Duration::from_millis(15)), 1);
        assert!((clock.alpha() - 0.5).abs() < 1e-4);

        // 5 ms left over + 5 ms = one more tick.
        assert_eq!(clock.advance_by(Duration::from_millis(5)), 1);
    }

    #[test]
    fn short_frames_run_no_ticks() {
        let mut clock = FixedStep::new(HZ);
        assert_eq!(clock.advance_by(Duration::from_millis(4)), 0);
    }

    #[test]
    fn stalls_are_capped() {
        let mut clock = FixedStep::new(HZ);
        let max_ticks = (MAX_FRAME.as_millis() / STEP.as_millis()) as u32;

        assert_eq!(clock.advance_by(Duration::from_secs(10)), max_ticks);
    }

    #[test]
    fn dt_matches_rate_and_zero_hz_is_safe() {
        assert!((FixedStep::new(HZ).dt() - 0.01).abs() < 1e-6);
        assert!((FixedStep::new(0).dt() - 1.0).abs() < 1e-6);
    }
}
