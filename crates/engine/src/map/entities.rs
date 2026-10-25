//! Quake entity classes → engine entities. Doors, buttons and triggers
//! keep Quake's keys and defaults, so maps built for Quake behave as
//! their authors expect.
//!
//! ```text
//!   worldspawn             → static meshes + solid brushes
//!   func_door              → mover (+ trigger field around it if untargeted)
//!   func_button            → usable, fires "target"
//!   trigger_once/_multiple → trigger, fires "target"
//!   func_illusionary       → meshes only
//!   other brush entities   → meshes + solid brushes (func_wall, …)
//!   info_player_start      → MapInfo::start
//!   any                    → "targetname" is the name, "script" the Lua class
//! ```

use glam::Vec3;

use super::geometry::colliders;
use super::space::{bounds, to_engine_direction, to_engine_point, to_engine_yaw};
use super::{PlayerStart, UNITS_PER_METER};
use crate::collision::Brush;
use crate::entity::{Close, Entity, Model, Mover, Repeat, Trigger, Usable};

/// Quake's player box reaches 24 units below its origin.
const PLAYER_ORIGIN_HEIGHT: f32 = 24.0;
/// Quake's func_door defaults: units/s, seconds, units.
const DOOR_SPEED: f32 = 100.0;
const DOOR_WAIT: f32 = 3.0;
const DOOR_LIP: f32 = 8.0;
/// A door wait of -1 means "stay open".
const STAY_OPEN: f32 = -1.0;
/// Untargeted doors open when the player comes this close (Quake's
/// door trigger field), units.
const DOOR_FIELD: Vec3 = Vec3::new(60.0, 60.0, 8.0);
/// Quake "angle" values meaning straight up / down.
const ANGLE_UP: f32 = -1.0;
const ANGLE_DOWN: f32 = -2.0;

/// What one map entity turns into.
pub(super) enum Spawn {
    /// Boxed: an `Entity` is much bigger than a `Brush`.
    Entity(Box<Entity>),
    /// World geometry: solid, no entity.
    Static(Brush),
}

/// The engine pieces for a brush entity with drawable `models`.
pub(super) fn brush_entity(
    entity: &level::Entity,
    model: &level::Model,
    models: Vec<Model>,
) -> Vec<Spawn> {
    let base = Entity {
        name: entity.get("targetname").map(str::to_owned),
        script: entity.get("script").map(str::to_owned),
        ..Default::default()
    };
    let target = entity.get("target").map(str::to_owned);
    let spawn = |e: Entity| Spawn::Entity(Box::new(e));

    match entity.classname() {
        "worldspawn" => {
            let mut out = vec![spawn(Entity { models, ..base })];
            out.extend(colliders(model).into_iter().map(Spawn::Static));
            out
        }
        "trigger_once" | "trigger_multiple" => {
            let repeat = match entity.classname() {
                "trigger_once" => Repeat::Once,
                _ => Repeat::EveryEntry,
            };
            vec![spawn(Entity {
                trigger: Some(Trigger {
                    volume: bounds(model.mins, model.maxs),
                    target,
                    repeat,
                }),
                ..base
            })]
        }
        "func_door" => door(
            entity,
            model,
            Entity {
                models,
                colliders: colliders(model),
                ..base
            },
        )
        .into_iter()
        .map(spawn)
        .collect(),
        "func_button" => vec![spawn(Entity {
            models,
            colliders: colliders(model),
            usable: Some(Usable { target }),
            ..base
        })],
        "func_illusionary" => vec![spawn(Entity { models, ..base })],
        _ => vec![spawn(Entity {
            models,
            colliders: colliders(model),
            ..base
        })],
    }
}

/// A func_door: slides along its "angle" by its own size less "lip".
/// Without a "targetname" Quake opens doors on approach, so it gets a
/// name and a trigger field around it.
fn door(entity: &level::Entity, model: &level::Model, door: Entity) -> Vec<Entity> {
    let mover = door_mover(entity, model);
    if door.name.is_some() {
        return vec![Entity {
            mover: Some(mover),
            ..door
        }];
    }

    let name = format!("*door{}", entity.model().unwrap_or_default());
    let field = Entity {
        trigger: Some(Trigger {
            volume: bounds(model.mins - DOOR_FIELD, model.maxs + DOOR_FIELD),
            target: Some(name.clone()),
            repeat: Repeat::EveryEntry,
        }),
        ..Default::default()
    };
    let door = Entity {
        name: Some(name),
        mover: Some(mover),
        ..door
    };
    vec![door, field]
}

/// Quake func_door travel: `|direction · size| − lip` along "angle".
fn door_mover(entity: &level::Entity, model: &level::Model) -> Mover {
    let direction = move_direction(number(entity, "angle", 0.0));
    let size = model.maxs - model.mins;
    let distance = direction.dot(size).abs() - number(entity, "lip", DOOR_LIP);
    let wait = number(entity, "wait", DOOR_WAIT);

    Mover {
        travel: to_engine_direction(direction) * distance / UNITS_PER_METER,
        speed: number(entity, "speed", DOOR_SPEED) / UNITS_PER_METER,
        close: if wait == STAY_OPEN {
            Close::OnFire
        } else {
            Close::After(wait)
        },
    }
}

/// Quake movement direction for an "angle": −1 up, −2 down, otherwise
/// degrees counter-clockwise from +X in the ground plane.
fn move_direction(angle: f32) -> Vec3 {
    if angle == ANGLE_UP {
        return Vec3::Z;
    }
    if angle == ANGLE_DOWN {
        return Vec3::NEG_Z;
    }

    let (sin, cos) = angle.to_radians().sin_cos();
    Vec3::new(cos, sin, 0.0)
}

pub(super) fn player_start(entity: &level::Entity) -> PlayerStart {
    let origin = vector(entity, "origin");
    let feet = origin - Vec3::Z * PLAYER_ORIGIN_HEIGHT;

    PlayerStart {
        feet: to_engine_point(feet),
        yaw: to_engine_yaw(number(entity, "angle", 0.0)),
    }
}

/// A numeric key, or `default` when missing or unparsable.
fn number(entity: &level::Entity, key: &str, default: f32) -> f32 {
    entity
        .get(key)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// A "x y z" key; zero when missing or malformed.
fn vector(entity: &level::Entity, key: &str) -> Vec3 {
    let parts: Vec<f32> = entity
        .get(key)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();

    match parts[..] {
        [x, y, z] => Vec3::new(x, y, z),
        _ => Vec3::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look::Look;

    const EPS: f32 = 1e-5;

    /// Entity from key/value pairs.
    fn entity(pairs: &[(&str, &str)]) -> level::Entity {
        let fields = pairs.iter().map(|&(k, v)| (k.into(), v.into()));
        level::Entity::new(fields.collect())
    }

    /// A 64 × 16 × 112 unit door model.
    fn door_model() -> level::Model {
        level::Model {
            mins: Vec3::new(-32.0, 0.0, 0.0),
            maxs: Vec3::new(32.0, 16.0, 112.0),
            faces: Vec::new(),
            brushes: Vec::new(),
        }
    }

    #[test]
    fn quake_angles_face_the_same_way() {
        // Angle 90 faces Quake +Y, which is engine −Z: yaw 0.
        for angle in [0.0, 90.0, 180.0, 270.0] {
            let forward = Look::new(to_engine_yaw(angle), 0.0).forward();
            let quake = move_direction(angle);
            assert!(
                (forward - to_engine_direction(quake)).length() < EPS,
                "{angle}"
            );
        }
    }

    #[test]
    fn door_moves_its_size_less_lip_along_its_angle() {
        let up = door_mover(&entity(&[("angle", "-1")]), &door_model());
        // 112 tall − 8 lip = 104 units up.
        assert!((up.travel - Vec3::Y * 104.0 / UNITS_PER_METER).length() < EPS);
        assert_eq!(up.speed, DOOR_SPEED / UNITS_PER_METER);
        assert_eq!(up.close, Close::After(DOOR_WAIT));

        let east = door_mover(
            &entity(&[("angle", "0"), ("lip", "4"), ("wait", "-1")]),
            &door_model(),
        );
        assert!((east.travel - Vec3::X * 60.0 / UNITS_PER_METER).length() < EPS);
        assert_eq!(east.close, Close::OnFire);
    }

    #[test]
    fn untargeted_door_gets_a_name_and_a_trigger_field() {
        let quake = entity(&[("classname", "func_door"), ("model", "*5")]);
        let spawned = door(&quake, &door_model(), Entity::default());

        let [door, field] = &spawned[..] else {
            panic!("expected door + field");
        };
        assert_eq!(door.name.as_deref(), Some("*door5"));
        let trigger = field.trigger.as_ref().unwrap();
        assert_eq!(trigger.target, door.name);
        // The field reaches 60 units past the door horizontally.
        assert!((trigger.volume.max.x - 92.0 / UNITS_PER_METER).abs() < EPS);
    }

    #[test]
    fn targeted_door_waits_to_be_fired() {
        let quake = entity(&[("classname", "func_door"), ("targetname", "door1")]);
        let named = Entity {
            name: Some("door1".into()),
            ..Default::default()
        };

        let spawned = door(&quake, &door_model(), named);
        assert_eq!(spawned.len(), 1);
        assert!(spawned[0].mover.is_some());
    }

    #[test]
    fn player_start_puts_feet_on_the_floor() {
        let start = player_start(&entity(&[("origin", "0 -192 24"), ("angle", "90")]));

        assert!((start.feet - Vec3::new(0.0, 0.0, 6.0)).length() < EPS);
        assert!(start.yaw.abs() < EPS);
    }

    #[test]
    fn malformed_numbers_fall_back_to_defaults() {
        let quake = entity(&[("speed", "fast"), ("origin", "1 2")]);

        assert_eq!(number(&quake, "speed", DOOR_SPEED), DOOR_SPEED);
        assert_eq!(vector(&quake, "origin"), Vec3::ZERO);
    }
}
