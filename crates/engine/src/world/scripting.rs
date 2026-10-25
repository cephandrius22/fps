//! The world's side of scripting: loading scripts, running hooks,
//! publishing the snapshot scripts read, and applying the commands they
//! queue. The only place engine and `script` types meet.
//!
//! ```text
//!   publish_view ─► vm.set_view(WorldView)   before hooks run
//!   run_hook     ─► vm.call(instance, hook)   errors logged, never fatal
//!   absorb_commands ◄─ vm.drain()             fire → queue, open/close → movers
//! ```

use glam::Vec3;
use script::{
    Command, EntityKey, EntityView, Hook, Instance, MoverPhase, MoverView, ScriptError, Target, Vm,
    WorldView,
};

use super::fires::Pending;
use super::{EntityId, Slot, World};
use crate::error::EngineError;
use crate::mover::Phase;

/// Log target for script output: `RUST_LOG=script=debug` and the like.
const SCRIPT_LOG: &str = "script";

/// What a script asked a mover to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drive {
    Open,
    Close,
}

impl World {
    /// Runs Lua source, defining or redefining its classes. Live
    /// instances keep their state and use the new methods. `chunk` names
    /// it in errors, e.g. "scripts/sandbox.lua". The engine already loads
    /// `assets/scripts/*.lua`; this is for scripts from elsewhere, and
    /// tests.
    pub fn load_script(&mut self, chunk: &str, source: &str) -> Result<(), EngineError> {
        let loaded = self.vm.load(chunk, source);
        self.absorb_commands();

        loaded.map_err(|err: ScriptError| EngineError::Load {
            path: chunk.to_owned(),
            source: err.into(),
        })
    }

    /// Script instance for a new entity; None (logged) if the class is
    /// unknown.
    pub(super) fn instantiate(
        &mut self,
        class: &str,
        name: Option<&str>,
        key: EntityKey,
    ) -> Option<Instance> {
        self.vm
            .instance(class, name, key)
            .inspect_err(|err| log::error!(target: SCRIPT_LOG, "{}", base::chain(err)))
            .ok()
    }

    /// `on_tick` for every scripted entity whose class has one.
    pub(super) fn tick_scripts(&mut self, dt: f32) {
        let hook = Hook::Tick { dt };

        for (_, slot) in self.entities.iter() {
            let Some(instance) = &slot.instance else {
                continue;
            };
            if self.vm.has_hook(instance, hook) {
                run_hook(&mut self.vm, instance, hook);
            }
        }

        self.absorb_commands();
    }

    /// Applies what scripts asked for: fires join the queue, logs print.
    pub(super) fn absorb_commands(&mut self) {
        for command in self.vm.drain() {
            match command {
                Command::Fire { target, delay } => self.pending.push(Pending { target, delay }),
                Command::Open(target) => self.drive(&target, Drive::Open),
                Command::Close(target) => self.drive(&target, Drive::Close),
                Command::Log(text) => log::info!(target: SCRIPT_LOG, "{text}"),
            }
        }
    }

    /// Opens or closes every mover `target` names.
    fn drive(&mut self, target: &Target, drive: Drive) {
        let mut matched = false;

        for (id, slot) in self.entities.iter_mut() {
            let named = match target {
                Target::Name(name) => slot.entity.name.as_deref() == Some(name),
                Target::Entity(key) => EntityId(id).key() == *key,
            };
            if !named || slot.entity.mover.is_none() {
                continue;
            }

            matched = true;
            match drive {
                Drive::Open => slot.mover.open(),
                Drive::Close => slot.mover.close(),
            }
        }

        if !matched {
            log::warn!(target: SCRIPT_LOG, "{drive:?}: no mover matches {target:?}");
        }
    }

    pub(super) fn has_scripts(&self) -> bool {
        self.entities
            .iter()
            .any(|(_, slot)| slot.instance.is_some())
    }

    /// Hands scripts a fresh snapshot of the world.
    pub(super) fn publish_view(&mut self) {
        let view = self.view();
        self.vm.set_view(view);
    }

    /// The world as scripts see it right now.
    fn view(&self) -> WorldView {
        let entities = self
            .entities
            .iter()
            .map(|(id, slot)| EntityView {
                key: EntityId(id).key(),
                name: slot.entity.name.clone(),
                position: self.shape_center(slot).to_array(),
                mover: slot.entity.mover.as_ref().map(|_| MoverView {
                    phase: script_phase(slot.mover.phase()),
                    progress: slot.mover.progress(),
                }),
            })
            .collect();

        WorldView {
            entities,
            player: self.player.map(|p| [p.center().x, p.min.y, p.center().z]),
            time: self.time,
        }
    }

    /// Middle of the entity's solid shape, or its origin without one:
    /// level geometry and map doors keep their origin at zero.
    fn shape_center(&self, slot: &Slot) -> Vec3 {
        slot.bodies
            .iter()
            .filter_map(|&body| self.physics.bounds(body))
            .reduce(|a, b| a.union(&b))
            .map_or(slot.entity.position, |bounds| bounds.center())
    }
}

fn script_phase(phase: Phase) -> MoverPhase {
    match phase {
        Phase::Closed => MoverPhase::Closed,
        Phase::Opening => MoverPhase::Opening,
        Phase::Open => MoverPhase::Open,
        Phase::Closing => MoverPhase::Closing,
    }
}

/// Calls a script hook; a failing hook is logged, never fatal.
pub(super) fn run_hook(vm: &mut Vm, instance: &Instance, hook: Hook) {
    if let Err(err) = vm.call(instance, hook) {
        log::error!(target: SCRIPT_LOG, "{}", base::chain(&err));
    }
}
