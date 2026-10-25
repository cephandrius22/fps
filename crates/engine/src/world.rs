//! The world: entities, the solid geometry they collide with, and the
//! per-tick rules that run them (movers, triggers, use).
//!
//! ```text
//!   Game::tick ─► walker.update(&world) ─► world.tick(dt, player box)
//!                                             ├─ movers: slide, unless
//!                                             │  the player is in the way
//!                                             └─ triggers: player entered?
//!                                                └─► fire(target) ─► movers
//!   Game::draw ─► world.draw(frame)   every entity model, interpolated
//! ```
//!
//! The game owns its `World`, like it owns its camera. Static level
//! geometry is entities too (a model plus a collider), or invisible
//! clip brushes via `add_brush`.

use base::{SlotId, Slots};
use glam::{Mat4, Vec3};
use physics::BodyId;

use crate::collision::{Aabb, Brush, Trace};
use crate::entity::{Close, Entity, Mover, Repeat};
use crate::gfx::Frame;

/// How far the player can reach to use something, meters.
pub const USE_REACH: f32 = 2.0;
/// Movers shorter than this (meters) arrive instantly.
const MIN_TRAVEL: f32 = 1e-4;

/// Handle to an entity in a [`World`]. Detects use after despawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityId(SlotId);

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

/// Mover progress: 0 at spawn, 1 at spawn + travel.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MoverState {
    progress: f32,
    motion: Motion,
}

impl MoverState {
    /// Fired: closed or closing movers open; open toggles close.
    fn fire(&mut self, close: Close) {
        self.motion = match self.motion {
            Motion::Closed | Motion::Closing => Motion::Opening,
            Motion::Open { .. } if close == Close::OnFire => Motion::Closing,
            other => other,
        };
    }
}

/// An entity plus what the engine tracks about it.
struct Slot {
    entity: Entity,
    spawn: Vec3,
    /// Position at the previous tick, for interpolation.
    prev_position: Vec3,
    body: Option<BodyId>,
    mover: MoverState,
    /// Player was inside the trigger volume last tick.
    inside: bool,
    /// A `Repeat::Once` trigger has fired.
    spent: bool,
}

/// Entities and solid geometry.
#[derive(Default)]
pub struct World {
    physics: physics::World,
    entities: Slots<Slot>,
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

    /// Adds an entity. Its collider, if any, becomes solid at once.
    pub fn spawn(&mut self, entity: Entity) -> EntityId {
        let body = entity.collider.as_ref().map(|collider| {
            let body = self.physics.add(collider.0.clone());
            self.physics.set_offset(body, entity.position);
            body
        });

        let slot = Slot {
            spawn: entity.position,
            prev_position: entity.position,
            body,
            mover: MoverState {
                progress: 0.0,
                motion: Motion::Closed,
            },
            inside: false,
            spent: false,
            entity,
        };
        EntityId(self.entities.insert(slot))
    }

    /// Removes an entity and its collider. False if already gone.
    pub fn despawn(&mut self, id: EntityId) -> bool {
        let Some(slot) = self.entities.remove(id.0) else {
            return false;
        };

        if let Some(body) = slot.body {
            self.physics.remove(body);
        }
        true
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

    /// Fires `target`: every entity with that name reacts. Movers open,
    /// or close if open and `Close::OnFire`.
    pub fn fire(&mut self, target: &str) {
        let mut matched = false;

        for (_, slot) in self.entities.iter_mut() {
            if slot.entity.name.as_deref() != Some(target) {
                continue;
            }
            matched = true;

            if let Some(mover) = &slot.entity.mover {
                slot.mover.fire(mover.close);
            }
        }

        if !matched {
            log::warn!("fired {target:?}, but no entity has that name");
        }
    }

    /// The player pressed use, looking along `direction` from `eye`.
    /// Fires the target of the first usable entity within
    /// [`USE_REACH`]. True if something was used.
    pub fn use_from(&mut self, eye: Vec3, direction: Vec3) -> bool {
        let end = eye + direction.normalize_or_zero() * USE_REACH;
        let trace = self.trace(eye, end, Vec3::ZERO);

        let usable = trace
            .entity
            .and_then(|id| self.entity(id))
            .and_then(|entity| entity.usable.as_ref());
        let Some(usable) = usable else {
            return false;
        };

        let target = usable.target.clone();
        self.fire(&target);
        true
    }

    /// One simulation step: movers slide, triggers check `player` (the
    /// player's box; None to ignore the player, e.g. while flying).
    pub fn tick(&mut self, dt: f32, player: Option<Aabb>) {
        let player = player.map(Aabb::to_physics);

        for (_, slot) in self.entities.iter_mut() {
            slot.prev_position = slot.entity.position;
            step_mover(slot, &mut self.physics, player, dt);
        }

        let fired = self.entered_triggers(player);
        for target in fired {
            self.fire(&target);
        }
    }

    /// Queues every entity's model, between the last two ticks.
    pub fn draw(&self, frame: &mut Frame, alpha: f32) {
        for (_, slot) in self.entities.iter() {
            let Some(model) = slot.entity.model else {
                continue;
            };

            let position = slot.prev_position.lerp(slot.entity.position, alpha);
            let transform = Mat4::from_rotation_translation(slot.entity.rotation, position);
            match model.texture {
                Some(texture) => frame.draw_textured(model.mesh, texture, transform),
                None => frame.draw_mesh(model.mesh, transform),
            }
        }
    }

    pub(crate) fn physics(&self) -> &physics::World {
        &self.physics
    }

    /// Targets of triggers the player entered this tick.
    fn entered_triggers(&mut self, player: Option<physics::Aabb>) -> Vec<String> {
        let mut fired = Vec::new();

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
            fired.push(trigger.target.clone());
        }

        fired
    }

    /// The entity owning `body`. A linear scan: fine for a sandbox's
    /// handful of entities; a body → entity map is the fix if levels grow.
    fn entity_of(&self, body: BodyId) -> Option<EntityId> {
        self.entities
            .iter()
            .find(|(_, slot)| slot.body == Some(body))
            .map(|(id, _)| EntityId(id))
    }
}

/// Advances one mover by `dt`. A step that would push into the player is
/// skipped: opening waits, closing turns back.
fn step_mover(
    slot: &mut Slot,
    physics: &mut physics::World,
    player: Option<physics::Aabb>,
    dt: f32,
) {
    let Some(mover) = slot.entity.mover else {
        return;
    };
    let state = &mut slot.mover;

    let direction = match state.motion {
        Motion::Closed => return,
        Motion::Open { elapsed } => {
            let elapsed = elapsed + dt;
            state.motion = match mover.close {
                Close::After(wait) if elapsed >= wait => Motion::Closing,
                _ => Motion::Open { elapsed },
            };
            return;
        }
        Motion::Opening => 1.0,
        Motion::Closing => -1.0,
    };

    let progress = (state.progress + direction * progress_rate(&mover) * dt).clamp(0.0, 1.0);
    let position = slot.spawn + mover.travel * progress;

    let moved = try_move(physics, slot.body, slot.entity.position, position, player);
    if !moved {
        if state.motion == Motion::Closing {
            state.motion = Motion::Opening;
        }
        return;
    }

    state.progress = progress;
    slot.entity.position = position;
    if state.motion == Motion::Opening && progress >= 1.0 {
        state.motion = Motion::Open { elapsed: 0.0 };
    }
    if state.motion == Motion::Closing && progress <= 0.0 {
        state.motion = Motion::Closed;
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

/// Moves the body from `from` to `to`. If that overlaps the player, puts
/// it back and returns false. Movers without a body always move.
fn try_move(
    physics: &mut physics::World,
    body: Option<BodyId>,
    from: Vec3,
    to: Vec3,
    player: Option<physics::Aabb>,
) -> bool {
    let Some(body) = body else {
        return true;
    };

    physics.set_offset(body, to);
    let crushing = player.is_some_and(|p| physics.overlaps(body, &p));
    if crushing {
        physics.set_offset(body, from);
    }

    !crushing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Trigger, Usable};

    const DT: f32 = 0.1;
    const DOOR: &str = "door";
    const HALF: Vec3 = Vec3::new(0.3, 0.9, 0.3);

    /// 2 m wide, 2 m tall, 0.2 m thick door, opening upward by 2 m.
    fn door(close: Close) -> Entity {
        Entity {
            name: Some(DOOR.into()),
            collider: Some(Brush::cuboid(
                Vec3::new(-1.0, 0.0, -0.1),
                Vec3::new(1.0, 2.0, 0.1),
            )),
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
    fn trigger_fires_on_entry_once_or_every_time() {
        let mut world = World::new();
        let id = world.spawn(door(Close::OnFire));
        let trigger = |repeat| Entity {
            position: Vec3::Z * 3.0,
            trigger: Some(Trigger {
                volume: Aabb::from_center(Vec3::ZERO, Vec3::ONE),
                target: DOOR.into(),
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
            collider: Some(Brush::cuboid(Vec3::splat(-0.1), Vec3::splat(0.1))),
            usable: Some(Usable {
                target: DOOR.into(),
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

    #[test]
    fn firing_an_unknown_name_changes_nothing() {
        let mut world = World::new();
        let id = world.spawn(door(Close::OnFire));

        world.fire("nobody");
        run(&mut world, 1.0, None);
        assert_eq!(door_y(&world, id), 0.0);
    }
}
