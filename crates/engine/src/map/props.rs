//! Props: "prop_static" entities, each drawing a glTF model where the
//! map places it.
//!
//! ```text
//!   "model" "models/barrel.glb" ─► props::read ─► meshes + textures
//!                                   (once per file and scale)
//!   "origin", "angle", "scale"  ─► position, turn about up, size
//!   "_light" (written by bake)  ─► a 1×1 lightmap: the light where it
//!                                   stands, so it matches the room
//!   "solid" "1"                 ─► a turned box around the model
//! ```
//!
//! Model files are in meters, Y up: engine space already. Not yet: hot
//! reload of model files, shadows cast by props.

use std::collections::HashMap;

use base::Image;
use entities::{BAKED_LIGHT, Keys};
use glam::{Quat, Vec3};

use super::space::to_engine_point;
use crate::collision::Brush;
use crate::entity::{Entity, Model};
use crate::gfx::{Gfx, MeshData, MeshHandle, TextureHandle, Vertex};

const MODEL_KEY: &str = "model";
/// "solid" "0" makes a prop walk-through.
const NOT_SOLID: f64 = 0.0;
/// Every vertex reads the 1×1 lightmap's only luxel.
const LUXEL_CENTER: [f32; 2] = [0.5, 0.5];

/// Models loaded so far in this map, so placing a model twice loads it
/// once.
#[derive(Default)]
pub(super) struct Props {
    /// By (path, scale bits); None when the file failed (logged once).
    models: HashMap<(String, u32), Option<Loaded>>,
    /// 1×1 lightmaps by light level.
    lights: HashMap<u8, TextureHandle>,
}

/// A model file's draws and bounds, at one scale.
#[derive(Clone)]
struct Loaded {
    draws: Vec<(MeshHandle, Option<TextureHandle>)>,
    /// Meters, in model space.
    bounds: Option<(Vec3, Vec3)>,
}

/// Where a prop stands.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    position: Vec3,
    rotation: Quat,
    scale: f32,
}

impl Props {
    /// The engine entity for a prop; None (logged) when it names no
    /// model or the model does not load.
    pub(super) fn spawn(&mut self, gfx: &mut Gfx, entity: &level::Entity) -> Option<Entity> {
        let keys = Keys::of(entity);
        let Some(path) = keys.text(MODEL_KEY) else {
            log::warn!("{} without a \"model\" key skipped", entity.classname());
            return None;
        };

        let placement = placement(&keys);
        let loaded = self.load(gfx, path, placement.scale)?;
        let lightmap = keys
            .number(BAKED_LIGHT)
            .and_then(|level| self.light(gfx, level.clamp(0.0, 255.0) as u8));

        let solid = keys.number("solid").is_none_or(|s| s != NOT_SOLID);
        let colliders = match (solid, loaded.bounds) {
            (true, Some(bounds)) => collider(bounds, placement.rotation).into_iter().collect(),
            _ => Vec::new(),
        };

        Some(Entity {
            name: keys.text("targetname").map(str::to_owned),
            script: keys.text("script").map(str::to_owned),
            position: placement.position,
            rotation: placement.rotation,
            models: loaded
                .draws
                .iter()
                .map(|&(mesh, texture)| Model {
                    mesh,
                    texture,
                    lightmap,
                })
                .collect(),
            colliders,
            ..Default::default()
        })
    }

    fn load(&mut self, gfx: &mut Gfx, path: &str, scale: f32) -> Option<Loaded> {
        let key = (path.to_owned(), scale.to_bits());
        if let Some(loaded) = self.models.get(&key) {
            return loaded.clone();
        }

        let loaded = read(gfx, path)
            .inspect_err(|err| log::warn!("prop {path} skipped: {}", base::chain(err.as_ref())))
            .ok()
            .map(|prop| upload(gfx, path, &prop, scale));
        self.models.insert(key, loaded.clone());
        loaded
    }

    /// A 1×1 lightmap of `level` (luxel units), shared by equal levels.
    fn light(&mut self, gfx: &mut Gfx, level: u8) -> Option<TextureHandle> {
        if let Some(&texture) = self.lights.get(&level) {
            return Some(texture);
        }

        let texture = gfx
            .create_lightmap(&Image::solid([level, level, level, u8::MAX]))
            .inspect_err(|err| log::warn!("prop light dropped: {}", base::chain(err)))
            .ok()?;
        self.lights.insert(level, texture);
        Some(texture)
    }
}

fn read(gfx: &Gfx, path: &str) -> Result<props::Prop, Box<dyn std::error::Error + Send + Sync>> {
    let bytes = gfx.vfs.read(path)?;
    Ok(props::read(&bytes)?)
}

/// Meshes and textures on the GPU, vertices scaled.
fn upload(gfx: &mut Gfx, path: &str, prop: &props::Prop, scale: f32) -> Loaded {
    let textures: Vec<Option<TextureHandle>> = prop
        .textures
        .iter()
        .map(|image| {
            gfx.create_texture(image)
                .inspect_err(|err| log::warn!("{path}: texture dropped: {}", base::chain(err)))
                .ok()
        })
        .collect();

    let draws = prop
        .meshes
        .iter()
        .map(|mesh| {
            let texture = mesh
                .texture
                .and_then(|t| textures.get(t).copied().flatten());
            (gfx.create_mesh(&mesh_data(mesh, scale)), texture)
        })
        .collect();

    Loaded {
        draws,
        bounds: prop.bounds().map(|(lo, hi)| (lo * scale, hi * scale)),
    }
}

fn mesh_data(mesh: &props::Mesh, scale: f32) -> MeshData {
    let vertices = mesh
        .positions
        .iter()
        .zip(&mesh.normals)
        .zip(&mesh.uvs)
        .map(|((&pos, &normal), &uv)| Vertex {
            pos: (pos * scale).to_array(),
            normal: normal.to_array(),
            uv: uv.to_array(),
            lightmap_uv: LUXEL_CENTER,
            color: mesh.color,
        })
        .collect();

    MeshData {
        vertices,
        indices: mesh.indices.clone(),
    }
}

/// Position (Quake units to meters), turn about up by "angle" (Quake's
/// counter-clockwise from above), and "scale".
fn placement(keys: &Keys) -> Placement {
    let origin = keys.vector("origin").unwrap_or_default().as_vec3();
    let angle = keys.number("angle").unwrap_or_default() as f32;

    Placement {
        position: to_engine_point(origin),
        rotation: Quat::from_rotation_y(angle.to_radians()),
        scale: keys.number("scale").unwrap_or(1.0) as f32,
    }
}

/// A box around model-space `bounds`, turned with the model, relative
/// to the entity's position.
fn collider((lo, hi): (Vec3, Vec3), rotation: Quat) -> Option<Brush> {
    let center = rotation * ((lo + hi) / 2.0);
    Brush::oriented_box(center, (hi - lo) / 2.0, rotation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::UNITS_PER_METER;

    const EPS: f32 = 1e-5;

    fn entity(pairs: &[(&str, &str)]) -> level::Entity {
        let fields = pairs.iter().map(|&(k, v)| (k.into(), v.into()));
        level::Entity::new(fields.collect())
    }

    #[test]
    fn placement_converts_origin_angle_and_scale() {
        let prop = entity(&[
            ("classname", "prop_static"),
            ("origin", "64 32 0"),
            ("angle", "90"),
            ("scale", "2"),
        ]);
        let placed = placement(&Keys::of(&prop));

        let expected = Vec3::new(64.0, 0.0, -32.0) / UNITS_PER_METER;
        assert!(placed.position.abs_diff_eq(expected, EPS));
        // Quake +X turned 90° counter-clockwise is Quake +Y: engine −Z.
        assert!((placed.rotation * Vec3::X).abs_diff_eq(Vec3::NEG_Z, EPS));
        assert_eq!(placed.scale, 2.0);

        let plain = placement(&Keys::of(&entity(&[("classname", "prop_static")])));
        assert_eq!((plain.rotation, plain.scale), (Quat::IDENTITY, 1.0));
    }

    #[test]
    fn collider_boxes_the_turned_model() {
        // A 2 × 1 × 1 model standing on its origin, turned 90°.
        let bounds = (Vec3::new(0.0, 0.0, -0.5), Vec3::new(2.0, 1.0, 0.5));
        let brush = collider(bounds, Quat::from_rotation_y(90f32.to_radians())).unwrap();
        let aabb = brush.bounds();

        assert!(
            aabb.min.abs_diff_eq(Vec3::new(-0.5, 0.0, -2.0), EPS),
            "{aabb:?}"
        );
        assert!(
            aabb.max.abs_diff_eq(Vec3::new(0.5, 1.0, 0.0), EPS),
            "{aabb:?}"
        );
    }

    #[test]
    fn mesh_data_scales_and_reads_the_lone_luxel() {
        let mesh = props::Mesh {
            positions: vec![Vec3::ONE],
            normals: vec![Vec3::Y],
            uvs: vec![glam::Vec2::X],
            indices: vec![0, 0, 0],
            texture: None,
            color: [0.5; 4],
        };
        let data = mesh_data(&mesh, 3.0);

        assert_eq!(data.vertices[0].pos, [3.0; 3]);
        assert_eq!(data.vertices[0].lightmap_uv, LUXEL_CENTER);
        assert_eq!(data.vertices[0].color, [0.5; 4]);
        assert_eq!(data.indices, [0, 0, 0]);
    }
}
