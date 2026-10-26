//! The whole compile: entities to models, brushes to solids, solids to
//! visible faces, all into one `Level`.
//!
//! ```text
//!   worldspawn + func_group + func_detail ─► model 0 (the world)
//!   every other entity with brushes       ─► model N, "model" "*N"
//!   point entities (lights, player start) ─► entities only
//!
//!   per model: brushes ─► solids ─► csg::visible ─► fragments
//!              (world only: drop those facing the void, outside.rs)
//!              ─► merge.rs ─► level faces
//! ```

use glam::DVec3;
use level::{Contents, Face, Level, Model};
use mapfile::Map;

use crate::csg::{self, Fragment};
use crate::merge::merge;
use crate::outside::Interior;
use crate::projection::Projection;
use crate::solid::Solid;
use crate::textures::{TextureSizes, TextureTable};

const WORLDSPAWN: &str = "worldspawn";
const MODEL_KEY: &str = "model";
/// Model index of the world.
const WORLD: usize = 0;

/// Compiles a parsed .map into a level, sizing textures with `sizes`.
/// Unlit: `bake` adds the lightmaps. Broken brushes are logged and left
/// out.
pub fn compile(map: &Map, sizes: &TextureSizes) -> Level {
    let (entities, models) = group(map);
    let seeds = seeds(map);
    let mut textures = TextureTable::new(sizes);
    let models = models
        .iter()
        .enumerate()
        .map(|(index, brushes)| {
            let void = match index {
                WORLD => Void::Drop(&seeds),
                _ => Void::Keep,
            };
            model(brushes, void, &mut textures)
        })
        .collect();

    Level {
        textures: textures.into_textures(),
        models,
        entities,
    }
}

/// Splits the map into its entities (worldspawn first, models keyed)
/// and each model's brushes.
fn group(map: &Map) -> (Vec<level::Entity>, Vec<Vec<&mapfile::Brush>>) {
    let world = map
        .entities
        .iter()
        .position(|e| e.fields.classname() == WORLDSPAWN);
    let mut entities = vec![match world {
        Some(index) => map.entities[index].fields.clone(),
        None => {
            log::warn!("map has no worldspawn: added an empty one");
            level::Entity::new(vec![("classname".into(), WORLDSPAWN.into())])
        }
    }];
    let mut models = vec![Vec::new()];

    for (index, entity) in map.entities.iter().enumerate() {
        let classname = entity.fields.classname();
        let merged = Some(index) == world || is_group(classname);
        if merged {
            models[0].extend(&entity.brushes);
            continue;
        }

        let mut fields = entity.fields.clone();
        if !entity.brushes.is_empty() {
            fields.set(MODEL_KEY, &format!("*{}", models.len()));
            models.push(entity.brushes.iter().collect());
        }
        entities.push(fields);
    }

    (entities, models)
}

/// Whether the catalog says `classname` only groups brushes (e.g.
/// func_group): merged into the world, and gone from the entity list.
fn is_group(classname: &str) -> bool {
    entities::class(classname).is_some_and(|c| c.kind == entities::Kind::Group)
}

/// Origins of point entities: where the map's open space is.
fn seeds(map: &Map) -> Vec<DVec3> {
    let points = map.entities.iter().filter(|e| e.brushes.is_empty());
    points
        .filter_map(|e| entities::Keys::of(&e.fields).vector("origin"))
        .collect()
}

/// What to do with faces that face the void outside the map.
enum Void<'a> {
    /// Find the outside by flooding from these points; drop its faces.
    Drop(&'a [DVec3]),
    /// Keep every face (brush entities, which move).
    Keep,
}

/// One model: its solids, their visible faces, and its bounds.
fn model(brushes: &[&mapfile::Brush], void: Void, textures: &mut TextureTable) -> Model {
    let solids: Vec<Solid> = brushes
        .iter()
        .filter_map(|brush| {
            Solid::build(brush)
                .inspect_err(|err| log::warn!("brush at line {} skipped: {err}", brush.line))
                .ok()
        })
        .collect();

    let mut fragments = csg::visible(&solids);
    if let Void::Drop(seeds) = void {
        drop_outside(&mut fragments, &solids, seeds);
    }

    let faces = merge(fragments, &solids)
        .iter()
        .flat_map(|fragment| faces(fragment, &solids, textures))
        .collect();

    let (mins, maxs) = match solids.is_empty() {
        true => (DVec3::ZERO, DVec3::ZERO),
        false => solids
            .iter()
            .fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), s| {
                (lo.min(s.mins), hi.max(s.maxs))
            }),
    };

    Model {
        mins: mins.as_vec3(),
        maxs: maxs.as_vec3(),
        faces,
        brushes: solids.iter().map(Solid::to_level).collect(),
    }
}

/// Removes fragments facing the void; keeps all, logged, when the map
/// leaks or has nothing inside to flood from.
fn drop_outside(fragments: &mut Vec<Fragment>, solids: &[Solid], seeds: &[DVec3]) {
    let interior = match Interior::flood(solids, seeds) {
        Ok(interior) => interior,
        Err(unsealed) => {
            log::warn!("{unsealed}: faces facing the void are kept");
            return;
        }
    };

    fragments.retain(|f| {
        let normal = solids[f.solid].sides[f.side].plane.normal;
        interior.sees(&f.winding, normal)
    });
}

/// The level faces for one visible fragment: none for tool textures,
/// two for liquids (seen from above and below the surface), else one.
fn faces(fragment: &Fragment, solids: &[Solid], textures: &mut TextureTable) -> Vec<Face> {
    let solid = &solids[fragment.solid];
    let side = &solid.sides[fragment.side];
    if level::is_tool_texture(&side.face.texture) {
        return Vec::new();
    }

    let texture = textures.index(&side.face.texture);
    let projection = Projection::new(side.face, side.plane.normal);
    let face = |winding: &crate::winding::Winding, normal: DVec3| Face {
        texture,
        normal: normal.as_vec3(),
        points: winding.points().iter().map(|p| p.as_vec3()).collect(),
        texels: winding
            .points()
            .iter()
            .map(|&p| projection.texel(p).as_vec2())
            .collect(),
        lightmap: None,
    };

    let front = face(&fragment.winding, side.plane.normal);
    if !is_liquid(solid.contents) {
        return vec![front];
    }
    vec![
        front,
        face(&fragment.winding.reversed(), -side.plane.normal),
    ]
}

fn is_liquid(contents: Contents) -> bool {
    matches!(contents, Contents::Water | Contents::Slime | Contents::Lava)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::cuboid;

    fn entity(classname: &str, brushes: Vec<mapfile::Brush>) -> mapfile::Entity {
        mapfile::Entity {
            fields: level::Entity::new(vec![("classname".into(), classname.into())]),
            brushes,
            line: 0,
        }
    }

    fn unit(texture: &str) -> mapfile::Brush {
        cuboid(DVec3::ZERO, DVec3::splat(64.0), texture)
    }

    fn sized(_: &str) -> Option<[u32; 2]> {
        Some([64, 64])
    }

    #[test]
    fn groups_merge_into_the_world_and_brush_entities_get_models() {
        let far = cuboid(DVec3::splat(128.0), DVec3::splat(192.0), "floor");
        let map = Map {
            entities: vec![
                entity("light", vec![]),
                entity(WORLDSPAWN, vec![unit("floor")]),
                entity("func_door", vec![unit("crate")]),
                entity("func_group", vec![far]),
                entity("trigger_once", vec![unit("trigger")]),
            ],
        };
        let level = compile(&map, &sized);

        let classes: Vec<_> = level.entities.iter().map(|e| e.classname()).collect();
        assert_eq!(classes, [WORLDSPAWN, "light", "func_door", "trigger_once"]);
        let models: Vec<_> = level.entities.iter().map(|e| e.model()).collect();
        assert_eq!(models, [Some(0), None, Some(1), Some(2)]);

        assert_eq!(level.models.len(), 3);
        assert_eq!(level.models[0].brushes.len(), 2, "world and group");
        assert_eq!(level.models[0].maxs, glam::Vec3::splat(192.0));
    }

    #[test]
    fn tool_faces_are_dropped_but_keep_their_brushes() {
        let map = Map {
            entities: vec![
                entity(WORLDSPAWN, vec![]),
                entity("trigger_multiple", vec![unit("trigger")]),
            ],
        };
        let level = compile(&map, &sized);
        let trigger = &level.models[1];

        assert!(trigger.faces.is_empty() && level.textures.is_empty());
        assert_eq!(trigger.brushes.len(), 1);
        assert_eq!(
            trigger.maxs,
            glam::Vec3::splat(64.0),
            "bounds for the volume"
        );
    }

    #[test]
    fn faces_carry_texture_and_texels() {
        let map = Map {
            entities: vec![entity(WORLDSPAWN, vec![unit("floor")])],
        };
        let level = compile(&map, &sized);
        let world = &level.models[0];

        assert_eq!(level.textures[0].name, "floor");
        assert_eq!(
            (level.textures[0].width, level.textures[0].height),
            (64, 64)
        );
        assert_eq!(world.faces.len(), 6);

        // The top: texels are (x, −y), as Quake maps floors.
        let top = world
            .faces
            .iter()
            .find(|f| f.normal == glam::Vec3::Z)
            .unwrap();
        for (p, t) in top.points.iter().zip(&top.texels) {
            assert_eq!(*t, glam::Vec2::new(p.x, -p.y));
        }
        assert!(top.lightmap.is_none());
    }

    #[test]
    fn liquid_faces_show_from_both_sides_and_bad_brushes_are_skipped() {
        let mut broken = unit("floor");
        broken.faces.truncate(3);
        let map = Map {
            entities: vec![entity(WORLDSPAWN, vec![unit("*water1"), broken])],
        };
        let level = compile(&map, &sized);
        let world = &level.models[0];

        assert_eq!(world.faces.len(), 12);
        assert_eq!(world.brushes.len(), 1);
        assert_eq!(world.brushes[0].contents, Contents::Water);
        let ups = world
            .faces
            .iter()
            .filter(|f| f.normal == glam::Vec3::Z)
            .count();
        assert_eq!(ups, 2, "the top, and the bottom seen from inside");
    }

    #[test]
    fn a_map_without_worldspawn_gets_one() {
        let level = compile(&Map::default(), &sized);

        assert_eq!(level.entities[0].classname(), WORLDSPAWN);
        assert_eq!(level.models.len(), 1);
        assert_eq!(level.models[0].mins, glam::Vec3::ZERO);
    }
}
