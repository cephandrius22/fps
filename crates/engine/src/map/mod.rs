//! Quake maps (.bsp, from ericw-tools) into the world: geometry becomes
//! meshes, BSPX brushes become collision, lightmaps become one atlas, and
//! Quake entity classes become engine entities. The only place engine and
//! `bsp` types meet.
//!
//! ```text
//!   .bsp ─► bsp::parse ─┬─ textures ──► geometry.rs: textures/<name>.png
//!                       ├─ lightmaps ─► lighting.rs: one atlas texture
//!                       └─ entities ──► entities.rs ─┬─ meshes (geometry.rs)
//!                                                    ├─ brushes (geometry.rs)
//!                                                    └─ World::spawn
//!   everything passes through space.rs: Quake units, Z up → meters, Y up
//! ```
//!
//! Not yet: colored or switchable lights, button travel, linked doors,
//! door spawnflags, map hot reload.

mod entities;
mod geometry;
mod lighting;
mod space;
#[cfg(test)]
mod test_map;

use glam::Vec3;

use crate::error::EngineError;
use crate::gfx::Gfx;
use crate::world::World;

use entities::{Spawn, brush_entity, player_start};
use geometry::{Surface, meshes, surface};
use lighting::lighting;

/// Quake units per meter: a 56-unit player is 1.75 m, a 64-unit door
/// 2 m wide.
pub const UNITS_PER_METER: f32 = 32.0;

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
    let lighting = lighting(gfx, &map, path);
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
        let models = meshes(gfx, index, model, &surfaces, lighting.as_ref());
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
