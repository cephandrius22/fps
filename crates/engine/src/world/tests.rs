//! World tests: entities, collision, movers, triggers, use, scripts.

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
    let (mut world, door) = scripted("Relay = {} function Relay:on_fire() world.fire('door') end");
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
    let (mut world, door) = scripted("Plate = {} function Plate:on_enter() world.fire('door') end");
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
    let (mut world, _) = scripted("Echo = {} function Echo:on_fire() world.fire(self.name) end");
    world.spawn(scripted_box("Echo", Some("echo"), Vec3::X * 5.0));

    world.fire("echo");
    run(&mut world, 0.3, None);
    // Still echoing, a bounded number of times per tick.
    assert!(world.pending.len() <= 1);
}

#[test]
fn scripts_read_the_player_and_entity_positions() {
    // Opens the door when the player is within 3 m of it.
    let (mut world, door) = scripted(
        "Sensor = {}
         function Sensor:on_tick()
             local p, d = world.player(), world.position('door')
             if p and math.abs(p.x - d.x) + math.abs(p.z - d.z) < 3 then
                 world.open('door')
             end
         end",
    );
    world.spawn(scripted_box("Sensor", None, Vec3::X * 9.0));
    let at = |z: f32| Some(Aabb::from_center(Vec3::new(0.0, 0.9, z), HALF));

    run(&mut world, 1.0, at(10.0));
    assert_eq!(door_y(&world, door), 0.0, "player far away");
    run(&mut world, 1.0, at(2.0));
    assert_eq!(door_y(&world, door), 2.0, "player close");
}

#[test]
fn scripts_read_mover_state_and_close_directly() {
    // Closes the toggle door as soon as it is fully open.
    let (mut world, door) = scripted(
        "Closer = {}
         function Closer:on_tick()
             local m = world.mover('door')
             if m.state == 'open' and m.progress == 1 then world.close('door') end
         end",
    );
    world.spawn(scripted_box("Closer", None, Vec3::X * 9.0));

    world.fire(DOOR);
    run(&mut world, 0.4, None);
    assert!(door_y(&world, door) > 1.0, "opening");
    run(&mut world, 1.5, None);
    assert_eq!(door_y(&world, door), 0.0, "closed by the script");
}

#[test]
fn scripts_drive_their_own_mover_through_self() {
    let mut world = World::new();
    world
        .load_script(
            "scripts/test.lua",
            "Lift = {} function Lift:on_use() world.open(self) end",
        )
        .unwrap();
    let lift = world.spawn(Entity {
        script: Some("Lift".into()),
        ..door(Close::OnFire)
    });

    assert!(world.use_from(Vec3::new(0.0, 1.0, 2.0), Vec3::NEG_Z));
    run(&mut world, 1.0, None);
    assert_eq!(door_y(&world, lift), 2.0);
}

#[test]
fn scripts_see_simulation_time() {
    let (mut world, door) = scripted(
        "Clock = {}
         function Clock:on_tick()
             if world.time() >= 0.5 then world.open('door') end
         end",
    );
    world.spawn(scripted_box("Clock", None, Vec3::X * 9.0));

    run(&mut world, 0.3, None);
    assert_eq!(door_y(&world, door), 0.0);
    run(&mut world, 1.0, None);
    assert_eq!(door_y(&world, door), 2.0);
}

#[test]
fn shape_center_reports_where_a_brush_entity_is() {
    // The door's origin is zero; its brush spans x −1..1, y 0..2.
    let (mut world, _) = scripted(
        "Probe = {}
         function Probe:on_spawn()
             local d = world.position('door')
             if d.x == 0 and d.y == 1 then world.open('door') end
         end",
    );
    let door = world.find(DOOR).unwrap();
    world.spawn(scripted_box("Probe", None, Vec3::X * 9.0));

    run(&mut world, 1.0, None);
    assert_eq!(door_y(&world, door), 2.0);
}

#[test]
fn firing_an_unknown_name_changes_nothing() {
    let mut world = World::new();
    let id = world.spawn(door(Close::OnFire));

    world.fire("nobody");
    run(&mut world, 1.0, None);
    assert_eq!(door_y(&world, id), 0.0);
}
