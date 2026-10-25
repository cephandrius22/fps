//! The fire queue: names waiting to fire, now or after a delay. Every
//! source of firing (`World::fire`, triggers, buttons, scripts) adds to
//! it, and `tick` drains it, so a chain of fires stays bounded per tick.
//!
//! ```text
//!   fire("door") ─► pending ──tick──► due? ─► movers react, scripts get
//!                      ▲                       on_fire ─► may fire more
//!                      └─────────────────────────────────┘
//!                      at most MAX_FIRES_PER_TICK per tick
//! ```

use script::Hook;

use super::World;
use super::scripting::run_hook;

/// Fires handled per tick; the rest wait for the next. Stops a script
/// that fires its own name from looping forever within one tick.
const MAX_FIRES_PER_TICK: usize = 64;

/// A name waiting to be fired.
pub(super) struct Pending {
    pub target: String,
    /// Seconds left; due at 0 or below.
    pub delay: f32,
}

impl World {
    /// Queues `target` to fire on the next `tick`: every entity with that
    /// name then reacts. Movers open (or close, if open and
    /// `Close::OnFire`); scripts get `on_fire`.
    pub fn fire(&mut self, target: &str) {
        self.fire_after(target, 0.0);
    }

    /// As [`fire`](Self::fire), `delay` seconds later.
    pub fn fire_after(&mut self, target: &str, delay: f32) {
        self.pending.push(Pending {
            target: target.to_owned(),
            delay,
        });
    }

    /// Counts down delayed fires, then fires what is due, oldest first.
    /// Scripts reacting to a fire may queue more; those run this tick
    /// too, up to `MAX_FIRES_PER_TICK`.
    pub(super) fn run_fires(&mut self, dt: f32) {
        for pending in &mut self.pending {
            pending.delay -= dt;
        }

        for _ in 0..MAX_FIRES_PER_TICK {
            let Some(due) = self.pending.iter().position(|p| p.delay <= 0.0) else {
                return;
            };
            let pending = self.pending.remove(due);
            self.fire_now(&pending.target);
        }

        if self.pending.iter().any(|p| p.delay <= 0.0) {
            log::warn!(
                "over {MAX_FIRES_PER_TICK} fires in one tick, the rest wait \
                 (a script firing its own name?)"
            );
        }
    }

    /// Every entity named `target` reacts now.
    pub(super) fn fire_now(&mut self, target: &str) {
        let mut matched = false;

        for (_, slot) in self.entities.iter_mut() {
            if slot.entity.name.as_deref() != Some(target) {
                continue;
            }
            matched = true;

            if let Some(mover) = &slot.entity.mover {
                slot.mover.fire(mover.close);
            }
            if let Some(instance) = &slot.instance {
                run_hook(&mut self.vm, instance, Hook::Fire);
            }
        }

        if !matched {
            log::warn!("fired {target:?}, but no entity has that name");
        }
        self.absorb_commands();
    }
}
