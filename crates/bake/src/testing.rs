//! Test fixtures: levels built in code.

use glam::{DVec3, Vec2, Vec3};
use level::{Brush, Contents, Entity, Face, Level, Model, Plane};

/// A model holding one solid box brush and no faces.
pub(crate) fn cuboid_model(min: DVec3, max: DVec3) -> Model {
    let (min, max) = (min.as_vec3(), max.as_vec3());
    let planes = [Vec3::X, Vec3::Y, Vec3::Z]
        .into_iter()
        .flat_map(|axis| {
            [
                Plane {
                    normal: axis,
                    dist: axis.dot(max),
                },
                Plane {
                    normal: -axis,
                    dist: -axis.dot(min),
                },
            ]
        })
        .collect();

    Model {
        mins: min,
        maxs: max,
        faces: Vec::new(),
        brushes: vec![Brush {
            contents: Contents::Solid,
            mins: min,
            maxs: max,
            planes,
        }],
    }
}

/// A level of these models: model 0 is the world, with a worldspawn.
pub(crate) fn level_with(models: Vec<Model>) -> Level {
    Level {
        textures: Vec::new(),
        models,
        entities: vec![Entity::new(vec![("classname".into(), "worldspawn".into())])],
    }
}

/// A `size`-unit square floor at height `z`, facing up, texels (x, −y)
/// as Quake maps floors.
pub(crate) fn floor(size: f32, z: f32) -> Face {
    let points: Vec<Vec3> = [(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)]
        .iter()
        .map(|&(x, y)| Vec3::new(x, y, z))
        .collect();

    Face {
        texture: 0,
        normal: Vec3::Z,
        texels: points.iter().map(|p| Vec2::new(p.x, -p.y)).collect(),
        points,
        lightmap: None,
    }
}

/// A light entity.
pub(crate) fn light(origin: DVec3, brightness: f64) -> Entity {
    Entity::new(vec![
        ("classname".into(), "light".into()),
        (
            "origin".into(),
            format!("{} {} {}", origin.x, origin.y, origin.z),
        ),
        ("light".into(), brightness.to_string()),
    ])
}
