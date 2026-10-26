//! [`Prop`]s to .glb files: one self-contained binary glTF, for tools
//! and tests that make models in code.
//!
//! ```text
//!   ┌──────────────┬──────────────────────────┬────────────────────────┐
//!   │ header (12)  │ JSON chunk: the document │ BIN chunk: vertex data │
//!   │ "glTF" 2 len │ (meshes, materials, …)   │ and PNG textures       │
//!   └──────────────┴──────────────────────────┴────────────────────────┘
//! ```
//!
//! Each mesh becomes a node, a mesh and a material; textures are
//! sampled nearest-neighbor, as the engine draws them.

use std::fmt::Write;

use glam::Vec3;

use crate::model::Prop;
use crate::read::PNG_MIME;

const MAGIC: &[u8; 4] = b"glTF";
const VERSION: u32 = 2;
const JSON_CHUNK: &[u8; 4] = b"JSON";
const BIN_CHUNK: &[u8; 4] = b"BIN\0";
/// Chunks start on 4-byte boundaries; JSON pads with spaces, BIN with
/// zeros.
const ALIGN: usize = 4;
const JSON_PAD: u8 = b' ';
/// glTF component types and buffer targets.
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
/// Sampler filter: nearest.
const NEAREST: u32 = 9728;

/// The prop as a .glb file. Fails only if a texture cannot be encoded.
pub fn write(prop: &Prop) -> Result<Vec<u8>, base::ImageError> {
    let mut doc = Document::default();

    for (i, mesh) in prop.meshes.iter().enumerate() {
        let positions = doc.vec3s(&mesh.positions, Bounds::Write);
        let normals = doc.vec3s(&mesh.normals, Bounds::Skip);
        let uvs = doc.floats(
            mesh.uvs.iter().flat_map(|v| v.to_array()),
            "VEC2",
            mesh.uvs.len(),
        );
        let indices = doc.indices(&mesh.indices);

        let texture = mesh
            .texture
            .map(|t| format!(r#","baseColorTexture":{{"index":{t}}}"#))
            .unwrap_or_default();
        let [r, g, b, a] = mesh.color;
        doc.materials.push(format!(
            r#"{{"pbrMetallicRoughness":{{"baseColorFactor":[{r},{g},{b},{a}],"metallicFactor":0{texture}}}}}"#
        ));
        doc.meshes.push(format!(
            r#"{{"primitives":[{{"attributes":{{"POSITION":{positions},"NORMAL":{normals},"TEXCOORD_0":{uvs}}},"indices":{indices},"material":{i}}}]}}"#
        ));
        doc.nodes.push(format!(r#"{{"mesh":{i}}}"#));
    }

    for (i, image) in prop.textures.iter().enumerate() {
        let png = base::image::encode_png(image)?;
        let view = doc.view(&png, None);
        doc.images.push(format!(
            r#"{{"bufferView":{view},"mimeType":"{PNG_MIME}"}}"#
        ));
        doc.textures
            .push(format!(r#"{{"source":{i},"sampler":0}}"#));
    }

    Ok(doc.into_glb())
}

/// Whether an accessor records its bounds (glTF requires them for
/// positions).
#[derive(Clone, Copy, PartialEq)]
enum Bounds {
    Write,
    Skip,
}

/// The document's arrays as JSON fragments, and the binary buffer.
#[derive(Default)]
struct Document {
    nodes: Vec<String>,
    meshes: Vec<String>,
    materials: Vec<String>,
    textures: Vec<String>,
    images: Vec<String>,
    accessors: Vec<String>,
    views: Vec<String>,
    bin: Vec<u8>,
}

impl Document {
    /// Appends `bytes` as a buffer view; returns its index.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.bin.resize(self.bin.len().next_multiple_of(ALIGN), 0);

        let target = target
            .map(|t| format!(r#","target":{t}"#))
            .unwrap_or_default();
        self.views.push(format!(
            r#"{{"buffer":0,"byteOffset":{offset},"byteLength":{}{target}}}"#,
            bytes.len()
        ));
        self.views.len() - 1
    }

    fn vec3s(&mut self, values: &[Vec3], bounds: Bounds) -> usize {
        let accessor = self.floats(
            values.iter().flat_map(|v| v.to_array()),
            "VEC3",
            values.len(),
        );
        if bounds == Bounds::Skip || values.is_empty() {
            return accessor;
        }

        let (lo, hi) = values.iter().fold((values[0], values[0]), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
        let json = self.accessors.last_mut().expect("just added");
        json.pop();
        let _ = write!(
            json,
            r#","min":[{},{},{}],"max":[{},{},{}]}}"#,
            lo.x, lo.y, lo.z, hi.x, hi.y, hi.z
        );
        accessor
    }

    fn floats(&mut self, values: impl Iterator<Item = f32>, kind: &str, count: usize) -> usize {
        let bytes: Vec<u8> = values.flat_map(f32::to_le_bytes).collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        self.accessor(view, FLOAT, kind, count)
    }

    fn indices(&mut self, indices: &[u32]) -> usize {
        let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ELEMENT_ARRAY_BUFFER));
        self.accessor(view, UNSIGNED_INT, "SCALAR", indices.len())
    }

    fn accessor(&mut self, view: usize, component: u32, kind: &str, count: usize) -> usize {
        self.accessors.push(format!(
            r#"{{"bufferView":{view},"componentType":{component},"count":{count},"type":"{kind}"}}"#
        ));
        self.accessors.len() - 1
    }

    fn into_glb(self) -> Vec<u8> {
        let list = |items: &[String]| items.join(",");
        let scene_nodes: Vec<String> = (0..self.nodes.len()).map(|i| i.to_string()).collect();
        let samplers = format!(r#"[{{"magFilter":{NEAREST},"minFilter":{NEAREST}}}]"#);
        let json = format!(
            concat!(
                r#"{{"asset":{{"version":"2.0","generator":"props"}},"scene":0,"#,
                r#""scenes":[{{"nodes":[{}]}}],"nodes":[{}],"meshes":[{}],"materials":[{}],"#,
                r#""textures":[{}],"images":[{}],"samplers":{},"accessors":[{}],"#,
                r#""bufferViews":[{}],"buffers":[{{"byteLength":{}}}]}}"#
            ),
            list(&scene_nodes),
            list(&self.nodes),
            list(&self.meshes),
            list(&self.materials),
            list(&self.textures),
            list(&self.images),
            samplers,
            list(&self.accessors),
            list(&self.views),
            self.bin.len()
        );

        pack(&json, &self.bin)
    }
}

/// The .glb container around a JSON document and its binary buffer.
pub(crate) fn pack(json: &str, bin: &[u8]) -> Vec<u8> {
    const HEADER: usize = 12;
    const CHUNK_HEADER: usize = 8;

    let mut json = json.as_bytes().to_vec();
    json.resize(json.len().next_multiple_of(ALIGN), JSON_PAD);
    let total = HEADER + CHUNK_HEADER + json.len() + CHUNK_HEADER + bin.len();

    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(MAGIC);
    glb.extend(VERSION.to_le_bytes());
    glb.extend((total as u32).to_le_bytes());
    for (kind, data) in [(JSON_CHUNK, &json[..]), (BIN_CHUNK, bin)] {
        glb.extend((data.len() as u32).to_le_bytes());
        glb.extend_from_slice(kind);
        glb.extend_from_slice(data);
    }
    glb
}

/// The JSON and binary chunks of a .glb this module wrote.
#[cfg(test)]
pub(crate) fn unpack(glb: &[u8]) -> (String, Vec<u8>) {
    let len = |at: usize| u32::from_le_bytes(glb[at..at + 4].try_into().unwrap()) as usize;
    let json_len = len(12);
    let json = String::from_utf8(glb[20..20 + json_len].to_vec()).unwrap();
    let bin_at = 20 + json_len;
    let bin = glb[bin_at + 8..bin_at + 8 + len(bin_at)].to_vec();
    (json.trim_end().to_owned(), bin)
}
