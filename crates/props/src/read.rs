//! .glb files to [`Prop`]s: the default scene's meshes, placed by their
//! nodes' transforms, with each material's base color and texture.
//!
//! ```text
//!   scene ─► nodes (recursive, transforms multiply)
//!          └─► mesh ─► primitives (triangles only)
//!                       ├─ POSITION, NORMAL, TEXCOORD_0, indices ─► Mesh
//!                       └─ material: base color factor + texture ─► textures
//! ```
//!
//! Only what a static prop needs: no skins, animation, morph targets,
//! cameras or lights. Missing normals are made flat; missing UVs are 0.

use std::collections::HashMap;

use glam::{Mat3, Mat4, Vec2, Vec3};
use gltf::buffer::Source as BufferSource;
use gltf::image::Source as ImageSource;
use gltf::mesh::Mode;

use crate::error::PropError;
use crate::model::{Mesh, Prop};

/// The only texture format read.
pub(crate) const PNG_MIME: &str = "image/png";

/// Reads a .glb file.
pub fn read(bytes: &[u8]) -> Result<Prop, PropError> {
    let gltf::Gltf { document, blob } = gltf::Gltf::from_slice(bytes)?;

    let buffers = document
        .buffers()
        .map(|buffer| match buffer.source() {
            BufferSource::Bin => blob.as_deref().ok_or(PropError::External("a buffer")),
            BufferSource::Uri(_) => Err(PropError::External("a buffer")),
        })
        .collect::<Result<Vec<&[u8]>, _>>()?;

    let mut reader = Reader {
        buffers: &buffers,
        prop: Prop::default(),
        textures: HashMap::new(),
    };
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next());
    for node in scene.iter().flat_map(|s| s.nodes()) {
        reader.node(&node, Mat4::IDENTITY)?;
    }

    Ok(reader.prop)
}

/// Builds the prop while walking the scene.
struct Reader<'a> {
    buffers: &'a [&'a [u8]],
    prop: Prop,
    /// glTF image index → index in `prop.textures`.
    textures: HashMap<usize, usize>,
}

impl Reader<'_> {
    fn node(&mut self, node: &gltf::Node, parent: Mat4) -> Result<(), PropError> {
        let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());

        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives().filter(|p| p.mode() == Mode::Triangles) {
                let read = self.primitive(&primitive, mesh.index(), transform)?;
                self.prop.meshes.push(read);
            }
        }
        for child in node.children() {
            self.node(&child, transform)?;
        }
        Ok(())
    }

    fn primitive(
        &mut self,
        primitive: &gltf::Primitive,
        mesh: usize,
        transform: Mat4,
    ) -> Result<Mesh, PropError> {
        let malformed = |problem| PropError::Mesh { mesh, problem };
        let buffers = self.buffers;
        let reader = primitive.reader(|buffer| buffers.get(buffer.index()).copied());

        let positions: Vec<Vec3> = reader
            .read_positions()
            .ok_or_else(|| malformed("no positions"))?
            .map(|p| transform.transform_point3(Vec3::from(p)))
            .collect();
        let count = positions.len();
        let mut indices: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().collect(),
            None => (0..count as u32).collect(),
        };
        if !indices.len().is_multiple_of(3) || indices.iter().any(|&i| i as usize >= count) {
            return Err(malformed(
                "indices do not make whole triangles of its vertices",
            ));
        }

        // A mirroring transform turns triangles inside out; swap two
        // corners of each to keep them counter-clockwise.
        if transform.determinant() < 0.0 {
            indices
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .for_each(|t| t.swap(1, 2));
        }

        let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
        let normals = match reader.read_normals() {
            Some(normals) => normals
                .map(|n| (normal_matrix * Vec3::from(n)).normalize_or_zero())
                .collect(),
            None => flat_normals(&positions, &indices),
        };

        let material = primitive.material().pbr_metallic_roughness();
        let base = material.base_color_texture();
        let uvs = match reader.read_tex_coords(base.as_ref().map_or(0, |t| t.tex_coord())) {
            Some(uvs) => uvs.into_f32().map(Vec2::from).collect(),
            None => vec![Vec2::ZERO; count],
        };
        let texture = match base {
            Some(info) => Some(self.texture(&info.texture().source())?),
            None => None,
        };

        Ok(Mesh {
            positions,
            normals,
            uvs,
            indices,
            texture,
            color: material.base_color_factor(),
        })
    }

    /// The prop texture for a glTF image, decoding it on first use.
    fn texture(&mut self, image: &gltf::Image) -> Result<usize, PropError> {
        if let Some(&index) = self.textures.get(&image.index()) {
            return Ok(index);
        }

        let (view, mime) = match image.source() {
            ImageSource::View { view, mime_type } => (view, mime_type),
            ImageSource::Uri { .. } => return Err(PropError::External("a texture")),
        };
        if mime != PNG_MIME {
            return Err(PropError::TextureFormat {
                index: image.index(),
                mime: mime.to_owned(),
            });
        }

        let buffer = self.buffers[view.buffer().index()];
        let bytes = buffer
            .get(view.offset()..view.offset() + view.length())
            .ok_or(PropError::External("a texture"))?;
        let decoded = base::image::read_png(bytes).map_err(|source| PropError::Texture {
            index: image.index(),
            source,
        })?;

        let index = self.prop.textures.len();
        self.prop.textures.push(decoded);
        self.textures.insert(image.index(), index);
        Ok(index)
    }
}

/// One normal per vertex, from the (last) triangle using it.
fn flat_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::Y; positions.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let [a, b, c] = triangle.map(|i| positions[i as usize]);
        let normal = (b - a).cross(c - a).normalize_or_zero();
        for &i in triangle {
            normals[i as usize] = normal;
        }
    }
    normals
}

#[cfg(test)]
mod tests {
    use base::Image;

    use super::*;
    use crate::write::{pack, unpack, write};

    /// A unit quad on the floor (y = 0), facing up, with `texture`.
    fn quad(texture: Option<usize>) -> Mesh {
        Mesh {
            positions: vec![
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, 1.0),
                Vec3::new(1.0, 0.0, 1.0),
                Vec3::X,
            ],
            normals: vec![Vec3::Y; 4],
            uvs: vec![Vec2::ZERO, Vec2::Y, Vec2::ONE, Vec2::X],
            indices: vec![0, 1, 2, 0, 2, 3],
            texture,
            color: [1.0, 0.5, 0.25, 1.0],
        }
    }

    fn checker() -> Image {
        let rgba = [[255, 0, 0, 255], [0, 0, 255, 255]].concat();
        Image::new(2, 1, rgba).unwrap()
    }

    /// The .glb of `prop` with its JSON edited by `edit`.
    fn edited(prop: &Prop, edit: impl Fn(String) -> String) -> Vec<u8> {
        let (json, bin) = unpack(&write(prop).unwrap());
        pack(&edit(json), &bin)
    }

    #[test]
    fn written_props_read_back_the_same() {
        let prop = Prop {
            meshes: vec![quad(Some(0)), quad(None), quad(Some(0))],
            textures: vec![checker()],
        };

        assert_eq!(read(&write(&prop).unwrap()).unwrap(), prop);
    }

    #[test]
    fn node_transforms_apply_and_mirrors_keep_winding() {
        let prop = Prop {
            meshes: vec![quad(None)],
            textures: Vec::new(),
        };
        let glb = edited(&prop, |json| {
            json.replace(
                r#""nodes":[{"mesh":0}]"#,
                r#""nodes":[{"mesh":0,"scale":[-2,1,1],"translation":[0,3,0]}]"#,
            )
        });
        let mesh = &read(&glb).unwrap().meshes[0];

        assert_eq!(mesh.positions[3], Vec3::new(-2.0, 3.0, 0.0));
        assert_eq!(
            mesh.normals[0],
            Vec3::Y,
            "normals follow the inverse transpose"
        );
        assert_eq!(
            &mesh.indices[..3],
            &[0, 2, 1],
            "swapped to stay counter-clockwise"
        );
    }

    #[test]
    fn missing_normals_are_made_flat() {
        let prop = Prop {
            meshes: vec![quad(None)],
            textures: Vec::new(),
        };
        let glb = edited(&prop, |json| json.replace(r#""NORMAL":1,"#, ""));

        let normals = &read(&glb).unwrap().meshes[0].normals;
        assert!(
            normals.iter().all(|n| n.abs_diff_eq(Vec3::Y, 1e-6)),
            "{normals:?}"
        );
    }

    #[test]
    fn broken_files_are_errors() {
        assert!(matches!(read(b"not a model"), Err(PropError::Gltf(_))));

        let mut bad = quad(None);
        bad.indices = vec![0, 1, 9];
        let prop = Prop {
            meshes: vec![bad],
            textures: Vec::new(),
        };
        let err = read(&write(&prop).unwrap()).unwrap_err();
        assert!(matches!(err, PropError::Mesh { mesh: 0, .. }), "{err}");

        let textured = Prop {
            meshes: vec![quad(Some(0))],
            textures: vec![checker()],
        };
        let jpeg = edited(&textured, |json| json.replace("image/png", "image/jpeg"));
        assert!(matches!(read(&jpeg), Err(PropError::TextureFormat { .. })));
    }
}
