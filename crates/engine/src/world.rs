//! The world: entities, the solid geometry they collide with, their
//! scripts, and the per-tick rules that run them (movers, triggers, use).
//!
//! ```text
//!   Game::tick ─► walker.update(&world) ─► world.tick(dt, player box)
//!                                             ├─ movers: slide, unless
//!                                             │  the player is in the way
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
//! The engine owns the `World`, loads scripts into it, and draws it; the
//! game spawns entities and advances it. Static level geometry is
//! entities too (models plus colliders), or invisible clip brushes via
//! `add_brush`.

use base::{SlotId, Slots};
use glam::{Mat4, Vec3};
use physics::BodyId;
use script::{Command, Hook, Instance, ScriptError, Vm};

use crate::collision::{Aabb, Brush, Trace};
use crate::entity::{Entity, Repeat};
use crate::error::EngineError;
use crate::gfx::Frame;
use crate::mover::{MoverState, Obstacles};

/// How far the player can reach to use something, meters.
pub const USE_REACH: f32 = 2.0;
/// Fires handled per tick; the rest wait for the next. Stops a script
/// that fires its own name from looping forever within one tick.
const MAX_FIRES_PER_TICK: usize = 64;
/// Log target for script output: `RUST_LOG=script=debug` and the like.
const SCRIPT_LOG: &str = "script";

/// Handle to an entity in a [`World`]. Detects use after despawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityId(SlotId);

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

/// A name waiting to be fired.
struct Pending {
    target: String,
    /// Seconds left; due at 0 or below.
    delay: f32,
}

/// Entities, solid geometry and scripts.
#[derive(Default)]
pub struct World {
    physics: physics::World,
    entities: Slots<Slot>,
    vm: Vm,
    pending: Vec<Pending>,
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
        let instance = entity
            .script
            .as_deref()
            .and_then(|class| self.instantiate(class, entity.name.as_deref()));

        let slot = Slot {
            prev_position: entity.position,
            bodies,
            mover: MoverState::new(entity.position),
            inside: false,
            spent: false,
            entity,
            instance,
        };
        let id = EntityId(self.entities.insert(slot));

        let instance = self.entities.get(id.0).and_then(|s| s.instance.as_ref());
        if let Some(instance) = instance {
            run_hook(&mut self.vm, instance, Hook::Spawn);
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

    /// The player pressed use, looking along `direction` from `eye`, at
    /// the first solid entity within [`USE_REACH`]. A `Usable` entity
    /// fires its target; a scripted one gets `on_use`. True if either.
    pub fn use_from(&mut self, eye: Vec3, direction: Vec3) -> bool {
        let end = eye + direction.normalize_or_zero() * USE_REACH;
        let trace = self.trace(eye, end, Vec3::ZERO);
        let Some(slot) = trace.entity.and_then(|id| self.entities.get(id.0)) else {
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
                match model.texture {
                    Some(texture) => frame.draw_textured(model.mesh, texture, transform),
                    None => frame.draw_mesh(model.mesh, transform),
                }
            }
        }
    }

    pub(crate) fn physics(&self) -> &physics::World {
        &self.physics
    }

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
    fn instantiate(&mut self, class: &str, name: Option<&str>) -> Option<Instance> {
        self.vm
            .instance(class, name)
            .inspect_err(|err| log::error!(target: SCRIPT_LOG, "{}", base::chain(err)))
            .ok()
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

    /// `on_tick` for every scripted entity whose class has one.
    fn tick_scripts(&mut self, dt: f32) {
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

    /// Counts down delayed fires, then fires what is due, oldest first.
    /// Scripts reacting to a fire may queue more; those run this tick
    /// too, up to `MAX_FIRES_PER_TICK`.
    fn run_fires(&mut self, dt: f32) {
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
    fn fire_now(&mut self, target: &str) {
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

    /// Applies what scripts asked for: fires join the queue, logs print.
    fn absorb_commands(&mut self) {
        for command in self.vm.drain() {
            match command {
                Command::Fire { target, delay } => self.pending.push(Pending { target, delay }),
                Command::Log(text) => log::info!(target: SCRIPT_LOG, "{text}"),
            }
        }
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

/// Calls a script hook; a failing hook is logged, never fatal.
fn run_hook(vm: &mut Vm, instance: &Instance, hook: Hook) {
    if let Err(err) = vm.call(instance, hook) {
        log::error!(target: SCRIPT_LOG, "{}", base::chain(&err));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Close, Mover, Trigger, Usable};

    const DT: f32 = 0.1;
    const DOOR: &str = "door";
    const HALF: Vec3 = Vec3::new(0.3, 0.9, 0.3);

    /// 2 m wide, 2 m tall, 0.2 m thick door, opening upward by 2 m.
    fn door(close: Close) -> Entity {
        Entity {
            name: Some(DOOR.into()),
            colliders: vec![Brush::cuboid(
                Vec3::new(-1.0, 0.0, -0.1),
                Vec3::new(1.0, 2.0, 0.1),
            )],
            mover: Some(Mover {
                travel: Vec3::Y * 2.0,
                speed: 4.0,
                close,
            }),
            ..Default::default()
        }
    }

    fn run(world: &mut World, seconds: f32, player: Option<Aabb>) {
        for _ in 0..(seconds / DT).round() as u32 {
            world.tick(DT, player);
        }
    }

    fn door_y(world: &World, id: EntityId) -> f32 {
        world.entity(id).unwrap().position.y
    }

    /// Player standing in the doorway.
    fn in_doorway() -> Option<Aabb> {
        Some(Aabb::from_center(Vec3::new(0.0, 0.9, 0.0), HALF))
    }

    #[test]
    fn find_returns_the_named_entity() {
        let mut world = World::new();
        let id = world.spawn(door(Close::OnFire));

        assert_eq!(world.find(DOOR), Some(id));
        assert_eq!(world.find("nobody"), None);
    }

    #[test]
    fn collider_blocks_traces_until_despawned() {
        let mut world = World::new();
        let id = world.spawn(door(Close::OnFire));
        let ray = |w: &World| {
            w.trace(
                Vec3::new(0.0, 1.0, 5.0),
                Vec3::new(0.0, 1.0, -5.0),
                Vec3::ZERO,
            )
        };

        let hit = ray(&world);
        assert_eq!(hit.entity, Some(id));
        assert_eq!(hit.normal, Some(Vec3::Z));

        assert!(world.despawn(id));
        assert!(!ray(&world).hit());
        assert!(!world.despawn(id));
    }

    #[test]
    fn static_brushes_collide_without_an_entity() {
        let mut world = World::new();
        world.add_brush(Brush::cuboid(Vec3::splat(-1.0), Vec3::ONE));

        let trace = world.trace(Vec3::X * 5.0, Vec3::ZERO, Vec3::ZERO);
        assert!(trace.hit());
        assert_eq!(trace.entity, None);
        assert!(!world.is_clear(Aabb::from_center(Vec3::ZERO, HALF)));
        assert!(world.is_clear(Aabb::from_center(Vec3::X * 3.0, HALF)));
    }

    #[test]
    fn fired_door_opens_and_closes_after_its_wait() {
        let mut world = World::new();
        let id = world.spawn(door(Close::After(1.0)));

        world.fire(DOOR);
        run(&mut world, 0.6, None);
        assert_eq!(door_y(&world, id), 2.0);

        // Open for a second, then 0.5 s back down.
        run(&mut world, 1.1, None);
        assert!(door_y(&world, id) > 0.0);
        run(&mut world, 0.5, None);
        assert_eq!(door_y(&world, id), 0.0);
    }

    #[test]
    fn toggle_door_closes_only_when_fired_again() {
        let mut world = World::new();
        let id = world.spawn(door(Close::OnFire));

        world.fire(DOOR);
        run(&mut world, 5.0, None);
        assert_eq!(door_y(&world, id), 2.0);

        world.fire(DOOR);
        run(&mut world, 1.0, None);
        assert_eq!(door_y(&world, id), 0.0);
    }

    #[test]
    fn closing_door_reopens_instead_of_crushing() {
        let mut world = World::new();
        let id = world.spawn(door(Close::After(0.0)));

        world.fire(DOOR);
        run(&mut world, 0.6, None);
        // Player steps under it; it starts down, touches, and goes back up.
        run(&mut world, 2.0, in_doorway());
        assert!(door_y(&world, id) > 0.2);

        run(&mut world, 1.5, None);
        assert_eq!(door_y(&world, id), 0.0);
    }

    #[test]
    fn multi_brush_door_moves_and_backs_off_as_one() {
        // Two 1 m halves side by side, x −1..0 and 0..1.
        let half = |x0: f32| Brush::cuboid(Vec3::new(x0, 0.0, -0.1), Vec3::new(x0 + 1.0, 2.0, 0.1));
        let mut world = World::new();
        let id = world.spawn(Entity {
            colliders: vec![half(-1.0), half(0.0)],
            ..door(Close::After(0.0))
        });
        let solid_at = |world: &World, x: f32| {
            let ray = world.trace(Vec3::new(x, 1.0, 5.0), Vec3::new(x, 1.0, -5.0), Vec3::ZERO);
            ray.entity == Some(id)
        };

        world.fire(DOOR);
        run(&mut world, 0.6, None);
        assert!(
            !solid_at(&world, -0.5) && !solid_at(&world, 0.5),
            "both halves open"
        );

        // Player under the right half only: the whole door backs off.
        let under_right = Some(Aabb::from_center(Vec3::new(0.5, 0.9, 0.0), HALF));
        run(&mut world, 2.0, under_right);
        assert!(door_y(&world, id) > 0.2);
        assert!(!solid_at(&world, -0.5), "left half stayed with the right");

        run(&mut world, 1.5, None);
        assert!(
            solid_at(&world, -0.5) && solid_at(&world, 0.5),
            "both closed"
        );
    }

    #[test]
    fn trigger_fires_on_entry_once_or_every_time() {
        let mut world = World::new();
        let id = world.spawn(door(Close::OnFire));
        let trigger = |repeat| Entity {
            position: Vec3::Z * 3.0,
            trigger: Some(Trigger {
                volume: Aabb::from_center(Vec3::ZERO, Vec3::ONE),
                target: Some(DOOR.into()),
                repeat,
            }),
            ..Default::default()
        };
        let inside = Some(Aabb::from_center(Vec3::Z * 3.0, HALF));
        let outside = Some(Aabb::from_center(Vec3::Z * 10.0, HALF));

        let once = world.spawn(trigger(Repeat::Once));
        // Staying inside fires once, not every tick.
        run(&mut world, 1.0, inside);
        assert_eq!(door_y(&world, id), 2.0);

        // Re-entering a spent trigger does nothing.
        run(&mut world, 0.1, outside);
        run(&mut world, 1.0, inside);
        assert_eq!(door_y(&world, id), 2.0);

        world.despawn(once);
        world.spawn(trigger(Repeat::EveryEntry));
        run(&mut world, 0.1, outside);
        run(&mut world, 1.0, inside);
        assert_eq!(door_y(&world, id), 0.0);
    }

    #[test]
    fn use_fires_a_usable_entity_within_reach() {
        let mut world = World::new();
        let id = world.spawn(door(Close::OnFire));
        world.spawn(Entity {
            position: Vec3::new(3.0, 1.0, 0.0),
            colliders: vec![Brush::cuboid(Vec3::splat(-0.1), Vec3::splat(0.1))],
            usable: Some(Usable {
                target: Some(DOOR.into()),
            }),
            ..Default::default()
        });

        let far = Vec3::new(3.0, 1.0, 5.0);
        assert!(!world.use_from(far, Vec3::NEG_Z));

        let near = Vec3::new(3.0, 1.0, 1.5);
        assert!(!world.use_from(near, Vec3::Z), "looking away");
        assert!(world.use_from(near, Vec3::NEG_Z));
        run(&mut world, 1.0, None);
        assert_eq!(door_y(&world, id), 2.0);

        // The door itself is solid but not usable.
        assert!(!world.use_from(Vec3::new(0.0, 3.0, 1.0), Vec3::NEG_Z));
    }

    /// World with the 2 m door plus `source` loaded as a script.
    fn scripted(source: &str) -> (World, EntityId) {
        let mut world = World::new();
        world.load_script("scripts/test.lua", source).unwrap();
        let door = world.spawn(door(Close::OnFire));
        (world, door)
    }

    /// A small solid box at `position` running `class`.
    fn scripted_box(class: &str, name: Option<&str>, position: Vec3) -> Entity {
        Entity {
            name: name.map(Into::into),
            position,
            colliders: vec![Brush::cuboid(Vec3::splat(-0.1), Vec3::splat(0.1))],
            script: Some(class.into()),
            ..Default::default()
        }
    }

    #[test]
    fn script_relays_a_fire_to_the_door() {
        let (mut world, door) =
            scripted("Relay = {} function Relay:on_fire() world.fire('door') end");
        world.spawn(scripted_box("Relay", Some("relay"), Vec3::X * 5.0));

        world.fire("relay");
        run(&mut world, 1.0, None);
        assert_eq!(door_y(&world, door), 2.0);
    }

    #[test]
    fn spawn_hook_runs_and_unknown_classes_are_skipped() {
        let (mut world, door) =
            scripted("Opener = {} function Opener:on_spawn() world.fire('door') end");

        let missing = world.spawn(scripted_box("Missing", None, Vec3::X * 5.0));
        assert!(
            world.entity(missing).is_some(),
            "spawned without its script"
        );

        world.spawn(scripted_box("Opener", None, Vec3::X * 5.0));
        run(&mut world, 1.0, None);
        assert_eq!(door_y(&world, door), 2.0);
    }

    #[test]
    fn use_reaches_a_script_without_usable() {
        let (mut world, door) = scripted("Knob = {} function Knob:on_use() world.fire('door') end");
        world.spawn(scripted_box("Knob", None, Vec3::new(3.0, 1.0, 0.0)));

        assert!(world.use_from(Vec3::new(3.0, 1.0, 1.0), Vec3::NEG_Z));
        run(&mut world, 1.0, None);
        assert_eq!(door_y(&world, door), 2.0);
    }

    #[test]
    fn trigger_without_target_still_calls_on_enter() {
        let (mut world, door) =
            scripted("Plate = {} function Plate:on_enter() world.fire('door') end");
        world.spawn(Entity {
            position: Vec3::Z * 3.0,
            trigger: Some(Trigger {
                volume: Aabb::from_center(Vec3::ZERO, Vec3::ONE),
                target: None,
                repeat: Repeat::Once,
            }),
            script: Some("Plate".into()),
            ..Default::default()
        });

        run(
            &mut world,
            1.0,
            Some(Aabb::from_center(Vec3::Z * 3.0, HALF)),
        );
        assert_eq!(door_y(&world, door), 2.0);
    }

    #[test]
    fn delayed_fire_waits_its_turn() {
        let (mut world, door) =
            scripted("Timer = {} function Timer:on_spawn() world.fire_after(0.5, 'door') end");
        world.spawn(scripted_box("Timer", None, Vec3::X * 5.0));

        run(&mut world, 0.3, None);
        assert_eq!(door_y(&world, door), 0.0);
        run(&mut world, 1.0, None);
        assert_eq!(door_y(&world, door), 2.0);
    }

    #[test]
    fn tick_hook_runs_every_tick() {
        let (mut world, door) = scripted(
            "Count = {}
             function Count:on_tick(dt)
                 self.t = (self.t or 0) + dt
                 if self.t >= 0.3 and not self.done then
                     self.done = true
                     world.fire('door')
                 end
             end",
        );
        world.spawn(scripted_box("Count", None, Vec3::X * 5.0));

        run(&mut world, 0.2, None);
        assert_eq!(door_y(&world, door), 0.0);
        run(&mut world, 1.0, None);
        assert_eq!(door_y(&world, door), 2.0);
    }

    #[test]
    fn a_script_firing_itself_cannot_hang_a_tick() {
        let (mut world, _) =
            scripted("Echo = {} function Echo:on_fire() world.fire(self.name) end");
        world.spawn(scripted_box("Echo", Some("echo"), Vec3::X * 5.0));

        world.fire("echo");
        run(&mut world, 0.3, None);
        // Still echoing, a bounded number of times per tick.
        assert!(world.pending.len() <= 1);
    }

    #[test]
    fn firing_an_unknown_name_changes_nothing() {
        let mut world = World::new();
        let id = world.spawn(door(Close::OnFire));

        world.fire("nobody");
        run(&mut world, 1.0, None);
        assert_eq!(door_y(&world, id), 0.0);
    }
}
