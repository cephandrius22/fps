//! Why a .lvl file could not be read. Every length is checked, so a
//! corrupt file is an error, never a panic or a huge allocation.

/// What was wrong with the file.
#[derive(Debug, thiserror::Error)]
pub enum LevelError {
    /// The file does not start with the .lvl magic bytes.
    #[error("not a level file")]
    Magic,
    /// A format version this reader does not know.
    #[error("level format version {found}, this build reads {supported}")]
    Version {
        /// The file's version.
        found: u32,
        /// The version this build reads and writes.
        supported: u32,
    },
    /// The file ends inside a structure.
    #[error("{0} is truncated")]
    Truncated(&'static str),
    /// A name or entity string is not UTF-8.
    #[error("{0} is not valid UTF-8")]
    Utf8(&'static str),
    /// A field holds a value no writer produces.
    #[error("{0} is corrupt")]
    Corrupt(&'static str),
    /// Bytes follow the last structure.
    #[error("{0} unexpected bytes after the level")]
    Trailing(usize),
}

pub(crate) type Result<T, E = LevelError> = std::result::Result<T, E>;
