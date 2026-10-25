//! Why a BSP file could not be read. The reader checks every size and
//! index, so a corrupt file is an error, never a panic.

/// What was wrong with the file.
#[derive(Debug, thiserror::Error)]
pub enum BspError {
    /// Too short for its header, or a structure runs past the end.
    #[error("{0} is truncated")]
    Truncated(&'static str),
    /// Neither Quake's BSP29 nor BSP2.
    #[error("unsupported BSP version {0:#010x}")]
    Version(u32),
    /// A lump's offset or length points outside the file.
    #[error("{0} lump lies outside the file")]
    LumpBounds(&'static str),
    /// A lump's length is not a whole number of records.
    #[error("{lump} lump is {len} bytes, not a multiple of {record}")]
    LumpSize {
        /// Lump name.
        lump: &'static str,
        /// Its length in bytes.
        len: usize,
        /// Bytes per record.
        record: usize,
    },
    /// A record refers to something that does not exist.
    #[error("{what} index {index} out of range")]
    Index {
        /// What kind of index, e.g. "vertex".
        what: &'static str,
        /// The bad value.
        index: i64,
    },
    /// A BSPX brush list of a version this reader does not know.
    #[error("BRUSHLIST version {0}, expected 1")]
    BrushListVersion(i32),
    /// The entity text does not parse.
    #[error("entities, line {line}: {message}")]
    Entities {
        /// 1-based line in the entity text.
        line: usize,
        /// What was expected.
        message: &'static str,
    },
}

pub(crate) type Result<T, E = BspError> = std::result::Result<T, E>;
