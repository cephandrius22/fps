//! A prop as plain data: triangle meshes and their textures, in the
//! model's own space. glTF space is the engine's: meters, Y up,
//! right-handed; a model's front faces +Z.

use base::Image;
use glam::{Vec2, Vec3};

/// A whole prop model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prop {
    /// One per material: a mesh draws with one texture.
    pub meshes: Vec<Mesh>,
    /// Textures, by index, as meshes refer to them.
    pub textures: Vec<Image>,
}

/// Triangles with one material.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    /// Per vertex, meters.
    pub positions: Vec<Vec3>,
    /// Per vertex, unit length.
    pub normals: Vec<Vec3>,
    /// Per vertex, texture coordinates: (0, 0) top-left, repeating.
    pub uvs: Vec<Vec2>,
    /// Three per triangle, counter-clockwise seen from outside.
    pub indices: Vec<u32>,
    /// Index into [`Prop::textures`]; None draws `color` alone.
    pub texture: Option<usize>,
    /// Linear RGBA multiplying the texture.
    pub color: [f32; 4],
}

impl Prop {
    /// Bounds of every vertex: (min, max). None for no vertices.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut points = self.meshes.iter().flat_map(|m| &m.positions);
        let first = *points.next()?;
        Some(points.fold((first, first), |(lo, hi), &p| (lo.min(p), hi.max(p))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_cover_every_mesh() {
        let mesh = |positions: Vec<Vec3>| Mesh {
            normals: vec![Vec3::Y; positions.len()],
            uvs: vec![Vec2::ZERO; positions.len()],
            indices: Vec::new(),
            positions,
            texture: None,
            color: [1.0; 4],
        };
        let prop = Prop {
            meshes: vec![
                mesh(vec![Vec3::ZERO, Vec3::ONE]),
                mesh(vec![Vec3::new(-1.0, 0.5, 2.0)]),
            ],
            textures: Vec::new(),
        };

        assert_eq!(
            prop.bounds(),
            Some((Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 2.0)))
        );
        assert_eq!(Prop::default().bounds(), None);
    }
}
