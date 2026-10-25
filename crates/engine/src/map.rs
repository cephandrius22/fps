//! Quake maps (.bsp, from ericw-tools) into the world: geometry becomes
//! meshes, BSPX brushes become collision, and Quake entity classes become
//! engine entities. The only place engine and `bsp` types meet.
//!
//! ```text
//!   Quake (units, Z up)        engine (meters, Y up)
//!     x  ───────────────────►    x
//!     y  ───────────────────►   −z        all ÷ UNITS_PER_METER
//!     z  ───────────────────►    y
//!
//!   worldspawn          → static meshes + solid brushes
//!   func_door           → mover (+ trigger field around it if untargeted)
//!   func_button         → usable, fires `target`
//!   trigger_once/_multiple → trigger, fires `target`
//!   func_illusionary    → meshes only
//!   other brush entities (func_wall, …) → meshes + solid brushes
//!   info_player_start   → MapInfo::start
//! ```
//!
//! Every entity also takes `targetname` (its name) and `script` (its Lua
//! class). Textures are `textures/<name>.png`; tool textures (trigger,
//! clip, skip, …) are never drawn. Not yet: lightmaps, button travel,
//! linked doors, door spawnflags.

use std::collections::BTreeMap;

use glam::{Vec2, Vec3};

use crate::collision::{Aabb, Brush, Plane};
use crate::entity::{Close, Entity, Model, Mover, Repeat, Trigger, Usable};
use crate::error::EngineError;
use crate::gfx::{Gfx, MeshData, TextureHandle, Vertex};
use crate::world::World;

/// Quake units per meter: a 56-unit player is 1.75 m, a 64-unit door
/// 2 m wide.
pub const UNITS_PER_METER: f32 = 32.0;
/// Textures that mark brushes for the compiler or the game, never drawn.
const TOOL_TEXTURES: [&str; 6] = ["trigger", "clip", "skip", "hint", "origin", "null"];
/// Texture size assumed when neither the map nor a PNG gives one.
const DEFAULT_TEXELS: f32 = 64.0;
const WHITE: [f32; 4] = [1.0; 4];

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

/// Where the map puts the player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerStart {
    /// Feet position, meters.
    pub feet: Vec3,
    /// Heading, radians (0 faces −Z, positive turns left).
    pub yaw: f32,
}

/// What a loaded map tells the game, beyond the entities it spawned.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapInfo {
    /// From `info_player_start`; None if the map has none.
    pub start: Option<PlayerStart>,
    /// The worldspawn `message`: the map's title.
    pub message: Option<String>,
}

/// How faces with one texture are drawn.
#[derive(Clone, Copy, Debug)]
enum Surface {
    /// A tool texture: not drawn.
    Hidden,
    /// Drawn with `texture` (None: untextured), sized `texels`.
    Drawn {
        texture: Option<TextureHandle>,
        texels: Vec2,
    },
}

/// Loads a .bsp from the asset directory and spawns its entities into
/// `world`.
pub(crate) fn load(gfx: &mut Gfx, world: &mut World, path: &str) -> Result<MapInfo, EngineError> {
    let load_err = |source| EngineError::Load {
        path: path.to_owned(),
        source,
    };

    let bytes = gfx.vfs.read(path).map_err(|err| load_err(err.into()))?;
    let map = bsp::parse(&bytes).map_err(|err| load_err(err.into()))?;
    if map.models.iter().all(|m| m.brushes.is_empty()) {
        log::warn!("{path} has no brush list (compile with qbsp -wrbrushes): nothing is solid");
    }

    let surfaces: Vec<Surface> = map.textures.iter().map(|t| surface(gfx, t)).collect();
    let mut info = MapInfo::default();

    for entity in &map.entities {
        match entity.classname() {
            "info_player_start" => info.start = Some(player_start(entity)),
            "worldspawn" => info.message = entity.get("message").map(str::to_owned),
            _ => {}
        }

        let Some(index) = entity.model() else {
            continue;
        };
        let Some(model) = map.models.get(index) else {
            log::warn!(
                "{path}: {} names missing model *{index}",
                entity.classname()
            );
            continue;
        };
        let models = meshes(gfx, model, &surfaces);
        for spawned in brush_entity(entity, model, models) {
            match spawned {
                Spawn::Entity(e) => {
                    world.spawn(*e);
                }
                Spawn::Static(brush) => world.add_brush(brush),
            }
        }
    }

    Ok(info)
}

/// What one map entity turns into.
enum Spawn {
    /// Boxed: an `Entity` is much bigger than a `Brush`.
    Entity(Box<Entity>),
    /// World geometry: solid, no entity.
    Static(Brush),
}

/// The engine pieces for a brush entity with drawable `models`.
fn brush_entity(entity: &bsp::Entity, model: &bsp::Model, models: Vec<Model>) -> Vec<Spawn> {
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
fn door(entity: &bsp::Entity, model: &bsp::Model, door: Entity) -> Vec<Entity> {
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
fn door_mover(entity: &bsp::Entity, model: &bsp::Model) -> Mover {
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

fn player_start(entity: &bsp::Entity) -> PlayerStart {
    let origin = vector(entity, "origin");
    let feet = origin - Vec3::Z * PLAYER_ORIGIN_HEIGHT;

    PlayerStart {
        feet: to_engine_point(feet),
        yaw: to_engine_yaw(number(entity, "angle", 0.0)),
    }
}

/// How a texture's faces are drawn: loads `textures/<name>.png`.
fn surface(gfx: &mut Gfx, texture: &bsp::Texture) -> Surface {
    let name = texture.name.to_lowercase();
    if TOOL_TEXTURES.contains(&name.as_str()) {
        return Surface::Hidden;
    }

    let size = Vec2::new(texture.width as f32, texture.height as f32);
    let texels = if size.min_element() > 0.0 {
        size
    } else {
        Vec2::splat(DEFAULT_TEXELS)
    };
    let loaded = gfx
        .load_texture(&format!("textures/{name}.png"))
        .inspect_err(|err| {
            log::warn!(
                "map texture {name:?} drawn untextured: {}",
                base::chain(err)
            )
        })
        .ok();

    Surface::Drawn {
        texture: loaded,
        texels,
    }
}

/// One mesh per texture for the model's visible faces.
fn meshes(gfx: &mut Gfx, model: &bsp::Model, surfaces: &[Surface]) -> Vec<Model> {
    // BTreeMap: the same draw order on every run.
    let mut by_texture: BTreeMap<usize, MeshData> = BTreeMap::new();

    for face in &model.faces {
        let Some(&Surface::Drawn { texels, .. }) = surfaces.get(face.texture) else {
            continue;
        };
        append_face(by_texture.entry(face.texture).or_default(), face, texels);
    }

    by_texture
        .into_iter()
        .map(|(index, data)| {
            let texture = match surfaces[index] {
                Surface::Drawn { texture, .. } => texture,
                Surface::Hidden => None,
            };
            Model {
                mesh: gfx.create_mesh(&data),
                texture,
            }
        })
        .collect()
}

/// Adds a convex face as a triangle fan. Collinear corners (common in
/// Quake faces) only add zero-area triangles.
fn append_face(data: &mut MeshData, face: &bsp::Face, texels: Vec2) {
    let base = data.vertices.len() as u32;
    let normal = to_engine_direction(face.normal).to_array();

    data.vertices
        .extend(face.points.iter().zip(&face.texels).map(|(&p, &t)| Vertex {
            pos: to_engine_point(p).to_array(),
            normal,
            uv: (t / texels).to_array(),
            color: WHITE,
            ..Default::default()
        }));

    let count = face.points.len() as u32;
    data.indices
        .extend((1..count - 1).flat_map(|i| [base, base + i, base + i + 1]));
}

/// Solid brushes of a model, in engine space. Liquids are not solid;
/// a degenerate brush is logged and skipped.
fn colliders(model: &bsp::Model) -> Vec<Brush> {
    model
        .brushes
        .iter()
        .filter(|b| b.contents.is_solid())
        .filter_map(|b| {
            let planes: Vec<_> = b.planes.iter().map(|p| to_engine_plane(*p)).collect();
            let brush = Brush::from_planes(&planes);
            if brush.is_none() {
                log::warn!("skipped a degenerate map brush at {:?}", b.mins);
            }
            brush
        })
        .collect()
}

fn to_engine_point(q: Vec3) -> Vec3 {
    to_engine_direction(q) / UNITS_PER_METER
}

fn to_engine_direction(q: Vec3) -> Vec3 {
    Vec3::new(q.x, q.z, -q.y)
}

/// The axis swap is a rotation, so only the distance scales.
fn to_engine_plane(p: bsp::Plane) -> Plane {
    Plane {
        normal: to_engine_direction(p.normal),
        dist: p.dist / UNITS_PER_METER,
    }
}

/// Quake angle (degrees, 0 = +X, 90 = +Y) → engine yaw (radians,
/// 0 = −Z, which is Quake +Y).
fn to_engine_yaw(angle: f32) -> f32 {
    (angle - 90.0).to_radians()
}

/// Engine box for Quake bounds; the swap reverses one axis.
fn bounds(mins: Vec3, maxs: Vec3) -> Aabb {
    let (a, b) = (to_engine_point(mins), to_engine_point(maxs));
    Aabb {
        min: a.min(b),
        max: a.max(b),
    }
}

/// A numeric key, or `default` when missing or unparsable.
fn number(entity: &bsp::Entity, key: &str, default: f32) -> f32 {
    entity
        .get(key)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// A "x y z" key; zero when missing or malformed.
fn vector(entity: &bsp::Entity, key: &str) -> Vec3 {
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

    /// Entity from Quake key/value text.
    fn entity(pairs: &str) -> bsp::Entity {
        let text = format!("{{\n{pairs}\n}}");
        let bytes = test_bsp(&text);
        bsp::parse(&bytes).unwrap().entities.remove(0)
    }

    /// Smallest valid BSP29 holding only `entities` (every other lump
    /// empty).
    fn test_bsp(entities: &str) -> Vec<u8> {
        const LUMPS: u32 = 15;
        let header = 4 + LUMPS * 8;
        let mut bytes = 29u32.to_le_bytes().to_vec();
        for lump in 0..LUMPS {
            let len = if lump == 0 { entities.len() as u32 } else { 0 };
            bytes.extend(header.to_le_bytes());
            bytes.extend(len.to_le_bytes());
        }
        bytes.extend(entities.as_bytes());
        bytes
    }

    /// A 64 × 16 × 112 unit door model.
    fn door_model() -> bsp::Model {
        bsp::Model {
            mins: Vec3::new(-32.0, 0.0, 0.0),
            maxs: Vec3::new(32.0, 16.0, 112.0),
            faces: Vec::new(),
            brushes: Vec::new(),
        }
    }

    #[test]
    fn axes_swap_to_y_up_and_scale_to_meters() {
        // Quake north (+Y) is engine −Z; Quake up is engine +Y.
        assert_eq!(to_engine_direction(Vec3::Y), Vec3::NEG_Z);
        assert_eq!(to_engine_direction(Vec3::Z), Vec3::Y);
        assert_eq!(
            to_engine_point(Vec3::new(32.0, 64.0, 96.0)),
            Vec3::new(1.0, 3.0, -2.0)
        );

        // Still right-handed: X × Y = Z holds after the swap.
        let [x, y, z] = [Vec3::X, Vec3::Y, Vec3::Z].map(to_engine_direction);
        assert!((x.cross(y) - z).length() < EPS);
    }

    #[test]
    fn planes_keep_the_same_points() {
        let quake = bsp::Plane {
            normal: Vec3::Z,
            dist: 64.0,
        };
        let plane = to_engine_plane(quake);
        let on_plane = to_engine_point(Vec3::new(5.0, 7.0, 64.0));

        assert!((plane.normal.dot(on_plane) - plane.dist).abs() < EPS);
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
    fn bounds_stay_ordered_after_the_swap() {
        let b = bounds(Vec3::new(0.0, 0.0, 0.0), Vec3::new(32.0, 64.0, 96.0));

        assert_eq!(b.min, Vec3::new(0.0, 0.0, -2.0));
        assert_eq!(b.max, Vec3::new(1.0, 3.0, 0.0));
    }

    #[test]
    fn door_moves_its_size_less_lip_along_its_angle() {
        let up = door_mover(&entity("\"angle\" \"-1\""), &door_model());
        // 112 tall − 8 lip = 104 units up.
        assert!((up.travel - Vec3::Y * 104.0 / UNITS_PER_METER).length() < EPS);
        assert_eq!(up.speed, DOOR_SPEED / UNITS_PER_METER);
        assert_eq!(up.close, Close::After(DOOR_WAIT));

        let east = door_mover(
            &entity("\"angle\" \"0\" \"lip\" \"4\" \"wait\" \"-1\""),
            &door_model(),
        );
        assert!((east.travel - Vec3::X * 60.0 / UNITS_PER_METER).length() < EPS);
        assert_eq!(east.close, Close::OnFire);
    }

    #[test]
    fn untargeted_door_gets_a_name_and_a_trigger_field() {
        let quake = entity("\"classname\" \"func_door\" \"model\" \"*5\"");
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
        let quake = entity("\"classname\" \"func_door\" \"targetname\" \"door1\"");
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
        let start = player_start(&entity("\"origin\" \"0 -192 24\" \"angle\" \"90\""));

        assert!((start.feet - Vec3::new(0.0, 0.0, 6.0)).length() < EPS);
        assert!(start.yaw.abs() < EPS);
    }

    #[test]
    fn malformed_numbers_fall_back_to_defaults() {
        let quake = entity("\"speed\" \"fast\" \"origin\" \"1 2\"");

        assert_eq!(number(&quake, "speed", DOOR_SPEED), DOOR_SPEED);
        assert_eq!(vector(&quake, "origin"), Vec3::ZERO);
    }
}

/// The committed test map (maps/test.map) loaded on a headless GPU.
#[cfg(test)]
mod test_map {
    use base::Key;
    use platform::{Event, KeyState};

    use super::*;
    use crate::assets::Registry;
    use crate::input::Input;
    use crate::look::MouseLook;
    use crate::test_util::{headless, repo_assets};
    use crate::walk::{WalkSettings, Walker};

    const PATH: &str = "maps/test.bsp";
    const DT: f32 = 1.0 / 60.0;
    const EYE: f32 = 1.6;
    const HALF: Vec3 = Vec3::new(0.3, 0.9, 0.3);

    /// Runs `check` on a world holding the test map; skips without a GPU.
    fn with_map(check: impl FnOnce(&mut World, &MapInfo)) {
        let vfs = repo_assets();
        let Some(mut renderer) = headless(&vfs) else {
            return;
        };
        let mut registry = Registry::with_shaders();
        let mut gfx = Gfx {
            renderer: &mut renderer,
            vfs: &vfs,
            registry: &mut registry,
        };
        let mut world = World::new();

        let info = gfx.load_map(PATH, &mut world).unwrap();
        check(&mut world, &info);
    }

    fn run(world: &mut World, seconds: f32, player: Option<Aabb>) {
        for _ in 0..(seconds / DT).round() as u32 {
            world.tick(DT, player);
        }
    }

    /// How far (meters) a ray goes from `from` along −Z before hitting.
    fn reach_north(world: &World, from: Vec3) -> f32 {
        let trace = world.trace(from, from + Vec3::NEG_Z * 30.0, Vec3::ZERO);
        from.z - trace.end.z
    }

    #[test]
    fn start_and_title_come_from_the_map() {
        with_map(|_, info| {
            let start = info.start.unwrap();
            assert!((start.feet - Vec3::new(0.0, 0.0, 6.0)).length() < 1e-4);
            assert!(start.yaw.abs() < 1e-4, "faces north (−Z)");
            assert_eq!(info.message.as_deref(), Some("Engine test map"));
        });
    }

    #[test]
    fn floor_under_the_start_is_solid_at_zero() {
        with_map(|world, info| {
            let feet = info.start.unwrap().feet;
            let trace = world.trace(feet + Vec3::Y, feet - Vec3::Y, Vec3::ZERO);

            assert!(trace.hit() && trace.end.y.abs() < 0.01, "{trace:?}");
            assert_eq!(trace.normal, Some(Vec3::Y));
        });
    }

    #[test]
    fn button_opens_the_targeted_door() {
        with_map(|world, _| {
            let door = world.find("door1").unwrap();
            let eye = Vec3::new(0.0, EYE, 0.0);
            // Door 1 sits 8 m north (Quake y 258).
            assert!(reach_north(world, eye) < 8.1, "door 1 starts closed");

            // The button: Quake (96, 252, 64) → engine (3, 2, −7.9).
            assert!(world.use_from(Vec3::new(3.0, 2.0, -6.5), Vec3::NEG_Z));
            run(world, 2.0, None);

            assert!(world.entity(door).unwrap().position.y > 3.0, "slid up");
            assert!(reach_north(world, eye) > 19.0, "through room B to door 2");
        });
    }

    #[test]
    fn untargeted_door_opens_on_approach() {
        with_map(|world, _| {
            let door = world.find("*door5").unwrap();
            // Quake y 608: inside door 2's field, short of the door.
            let player = Aabb::from_center(Vec3::new(0.0, 0.9, -19.0), HALF);

            run(world, 1.0, Some(player));
            assert!(world.entity(door).unwrap().position.x > 1.0, "slid east");
        });
    }

    #[test]
    fn walker_climbs_the_stairs_to_the_ledge() {
        with_map(|world, _| {
            // Facing east (+X) toward 37.5 cm steps up to a 1.125 m ledge.
            let east = -std::f32::consts::FRAC_PI_2;
            let mut walker =
                Walker::new(Vec3::new(2.0, 0.0, 0.0), east, 0.0, WalkSettings::arena());
            let mut input = Input::default();
            input.apply(Event::Key {
                key: Key::W,
                state: KeyState::Pressed,
            });

            for _ in 0..120 {
                walker.update(&input, MouseLook::Disabled, world, DT);
                input.end_tick();
            }

            let feet = walker.feet();
            assert!(
                (feet.y - 36.0 / UNITS_PER_METER).abs() < 0.02,
                "on the ledge: {feet:?}"
            );
            assert!(feet.x < 8.0, "stopped at the east wall: {feet:?}");
        });
    }
}
