//! The world: entities, the solid geometry they collide with, their
//! scripts, and the per-tick rules that run them (movers, triggers, use).
//!
//! ```text
//!   Game::tick ─► walker.update(&world) ─► world.tick(dt, player box)
//!                                             ├─ movers: slide, unless
//!                                             │  the player is in the way
//!                                             ├─ snapshot for scripts (WorldView)
//!                                             ├─ triggers: player entered? ─► on_enter
//!                                             ├─ scripts: on_tick
//!                                             └─ fire queue: due names ─► movers,
//!                                                on_fire ─► scripts may queue more
//!   engine ─► world.draw(frame)   every entity model, interpolated
//! ```
//!
//! Firing is queued, not immediate: `fire`, triggers, buttons and
//! scripts all add to one queue that `tick` drains, at most
//! `MAX_FIRES_PER_TICK` at a time, so a script that fires its own name
//! cannot hang the game.
//!
//! Scripts read the world from a snapshot published before hooks run:
//! entity positions, mover states, the player, the time. They act
//! through commands applied after: fire, and open / close a mover
//! directly.
//!
//! The engine owns the `World`, loads scripts into it, and draws it; the
//! game spawns entities and advances it. Static level geometry is
//! entities too (models plus colliders), or invisible clip brushes via
//! `add_brush`.
//!
//! ```text
//!   mod.rs        entities, collision, use, triggers, tick, draw
//!   fires.rs      the fire queue
//!   scripting.rs  hooks, the script snapshot, script commands
//!   ../mover.rs   the door state machine
//! ```

use base::{SlotId, Slots};
use glam::{Mat4, Vec3};
use physics::BodyId;
use script::{EntityKey, Hook, Instance, Vm};

use crate::collision::{Aabb, Brush, Trace};
use crate::entity::{Entity, Repeat};
use crate::gfx::Frame;
use crate::mover::{MoverState, Obstacles};

use fires::Pending;
use scripting::run_hook;

mod fires;
mod scripting;
#[cfg(test)]
mod tests;

/// How far the player can reach to use something, meters.
pub const USE_REACH: f32 = 2.0;

/// Handle to an entity in a [`World`]. Detects use after despawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityId(SlotId);

impl EntityId {
    /// The handle as scripts carry it.
    fn key(self) -> EntityKey {
        self.0.to_bits()
    }
}

/// An entity plus what the engine tracks about it.
struct Slot {
    entity: Entity,
    /// Position at the previous tick, for interpolation.
    prev_position: Vec3,
    /// One physics body per collider brush.
    bodies: Vec<BodyId>,
    /// Used only when `entity.mover` is set.
    mover: MoverState,
    /// Player was inside the trigger volume last tick.
    inside: bool,
    /// A `Repeat::Once` trigger has fired.
    spent: bool,
    /// The entity's script state, if it has a script that loaded.
    instance: Option<Instance>,
}

/// Entities, solid geometry and scripts.
#[derive(Default)]
pub struct World {
    physics: physics::World,
    entities: Slots<Slot>,
    vm: Vm,
    pending: Vec<Pending>,
    /// The player's box at the last tick; None while not a body.
    player: Option<physics::Aabb>,
    /// Seconds simulated.
    time: f64,
}

impl World {
    /// Empty world.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds invisible solid geometry with no entity: clip brushes.
    pub fn add_brush(&mut self, brush: Brush) {
        self.physics.add(brush.0);
    }

    /// Adds an entity. Its colliders, if any, become solid at once; its
    /// script, if any, gets an instance and `on_spawn`. A script class
    /// that does not exist is logged, and the entity spawns without it.
    pub fn spawn(&mut self, entity: Entity) -> EntityId {
        let bodies = entity
            .colliders
            .iter()
            .map(|collider| {
                let body = self.physics.add(collider.0.clone());
                self.physics.set_offset(body, entity.position);
                body
            })
            .collect();
        let script = entity.script.clone();
        let name = entity.name.clone();

        let slot = Slot {
            prev_position: entity.position,
            bodies,
            mover: MoverState::new(entity.position),
            inside: false,
            spent: false,
            entity,
            instance: None,
        };
        let id = EntityId(self.entities.insert(slot));

        // The instance needs the entity's key, so it comes second.
        let Some(class) = script else {
            return id;
        };
        let Some(instance) = self.instantiate(&class, name.as_deref(), id.key()) else {
            return id;
        };
        self.publish_view();
        run_hook(&mut self.vm, &instance, Hook::Spawn);
        if let Some(slot) = self.entities.get_mut(id.0) {
            slot.instance = Some(instance);
        }
        self.absorb_commands();
        id
    }

    /// Removes an entity and its colliders. False if already gone.
    pub fn despawn(&mut self, id: EntityId) -> bool {
        let Some(slot) = self.entities.remove(id.0) else {
            return false;
        };

        for body in slot.bodies {
            self.physics.remove(body);
        }
        true
    }

    /// The first entity named `name`, e.g. a door from a map.
    pub fn find(&self, name: &str) -> Option<EntityId> {
        self.entities
            .iter()
            .find(|(_, slot)| slot.entity.name.as_deref() == Some(name))
            .map(|(id, _)| EntityId(id))
    }

    /// The entity, as of the latest tick (movers update `position`).
    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(id.0).map(|slot| &slot.entity)
    }

    /// True when nothing solid overlaps `bounds`, e.g. before placing a
    /// player there. Touching a surface counts as overlapping, so leave a
    /// small gap (the walker spawns 1 cm above its feet position).
    pub fn is_clear(&self, bounds: Aabb) -> bool {
        let b = bounds.to_physics();
        !self
            .physics
            .trace(b.center(), b.center(), b.half())
            .start_solid
    }

    /// Sweeps a box of half-size `half` from `start` to `end`. A zero
    /// `half` traces a ray.
    pub fn trace(&self, start: Vec3, end: Vec3, half: Vec3) -> Trace {
        let trace = self.physics.trace(start, end, half);

        Trace {
            fraction: trace.fraction,
            end: trace.end,
            normal: trace.normal,
            entity: trace.body.and_then(|body| self.entity_of(body)),
            start_solid: trace.start_solid,
        }
    }

    /// The player pressed use, looking along `direction` from `eye`, at
    /// the first solid entity within [`USE_REACH`]. A `Usable` entity
    /// fires its target; a scripted one gets `on_use`. True if either.
    pub fn use_from(&mut self, eye: Vec3, direction: Vec3) -> bool {
        let end = eye + direction.normalize_or_zero() * USE_REACH;
        let trace = self.trace(eye, end, Vec3::ZERO);
        let Some(id) = trace.entity else {
            return false;
        };
        if self
            .entities
            .get(id.0)
            .is_some_and(|s| s.instance.is_some())
        {
            self.publish_view();
        }
        let Some(slot) = self.entities.get(id.0) else {
            return false;
        };

        let target = slot.entity.usable.as_ref().and_then(|u| u.target.clone());
        let scripted = slot.instance.is_some();
        if let Some(instance) = &slot.instance {
            run_hook(&mut self.vm, instance, Hook::Use);
        }
        if let Some(target) = &target {
            self.fire(target);
        }
        self.absorb_commands();

        target.is_some() || scripted
    }

    /// One simulation step: movers slide, triggers check `player` (the
    /// player's box; None to ignore the player, e.g. while flying),
    /// scripts tick, and due fires run.
    pub fn tick(&mut self, dt: f32, player: Option<Aabb>) {
        let player = player.map(Aabb::to_physics);
        let mut obstacles = Obstacles {
            physics: &mut self.physics,
            player,
        };

        for (_, slot) in self.entities.iter_mut() {
            slot.prev_position = slot.entity.position;

            let Some(mover) = &slot.entity.mover else {
                continue;
            };
            if let Some(position) = slot.mover.step(mover, &slot.bodies, &mut obstacles, dt) {
                slot.entity.position = position;
            }
        }

        self.time += f64::from(dt);
        self.player = player;
        if self.has_scripts() {
            self.publish_view();
        }
        self.enter_triggers(player);
        self.tick_scripts(dt);
        self.run_fires(dt);
    }

    /// Queues every entity's models, between the last two ticks.
    pub fn draw(&self, frame: &mut Frame) {
        for (_, slot) in self.entities.iter() {
            if slot.entity.models.is_empty() {
                continue;
            }

            let position = slot.prev_position.lerp(slot.entity.position, frame.alpha);
            let transform = Mat4::from_rotation_translation(slot.entity.rotation, position);
            for model in &slot.entity.models {
                frame.push(model.mesh, model.texture, model.lightmap, transform);
            }
        }
    }

    pub(crate) fn physics(&self) -> &physics::World {
        &self.physics
    }

    /// Triggers the player entered this tick: fire their targets and call
    /// their scripts' `on_enter`.
    fn enter_triggers(&mut self, player: Option<physics::Aabb>) {
        for (_, slot) in self.entities.iter_mut() {
            let Some(trigger) = &slot.entity.trigger else {
                continue;
            };

            let volume = trigger.volume.shifted(slot.entity.position).to_physics();
            let inside = player.is_some_and(|p| volume.overlaps(&p));
            let entered = inside && !slot.inside;
            slot.inside = inside;

            if !entered || slot.spent {
                continue;
            }
            slot.spent = trigger.repeat == Repeat::Once;

            if let Some(target) = &trigger.target {
                self.pending.push(Pending {
                    target: target.clone(),
                    delay: 0.0,
                });
            }
            if let Some(instance) = &slot.instance {
                run_hook(&mut self.vm, instance, Hook::Enter);
            }
        }

        self.absorb_commands();
    }

    /// The entity owning `body`. A linear scan: fine for a sandbox's
    /// handful of entities; a body → entity map is the fix if levels grow.
    fn entity_of(&self, body: BodyId) -> Option<EntityId> {
        self.entities
            .iter()
            .find(|(_, slot)| slot.bodies.contains(&body))
            .map(|(id, _)| EntityId(id))
    }
}
