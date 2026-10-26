//! Why a prop could not be read.

/// What was wrong with the file.
#[derive(Debug, thiserror::Error)]
pub enum PropError {
    /// Not valid glTF.
    #[error("not a valid glTF file")]
    Gltf(#[from] gltf::Error),
    /// Data outside the file: only self-contained .glb is read.
    #[error("{0} refers to a separate file (only self-contained .glb is read)")]
    External(&'static str),
    /// A texture is not a PNG.
    #[error("texture {index} is {mime}, not PNG")]
    TextureFormat {
        /// Image index in the file.
        index: usize,
        /// Its media type.
        mime: String,
    },
    /// A PNG texture does not decode.
    #[error("texture {index} does not decode")]
    Texture {
        /// Image index in the file.
        index: usize,
        /// The decoder's error.
        #[source]
        source: base::ImageError,
    },
    /// A mesh has no positions, or indices past its vertices.
    #[error("mesh {mesh} is malformed: {problem}")]
    Mesh {
        /// Mesh index in the file.
        mesh: usize,
        /// What is wrong.
        problem: &'static str,
    },
}
