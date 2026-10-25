//! Foundation layer: small, dependency-light utilities and vocabulary
//! types usable by every other crate, like an extension of `std`.
//! Nothing here knows about windows, GPUs, or game rules.
//!
//! Modules:
//!
//! ```text
//!   image.rs   Image (RGBA8), PNG read / write, mip halving
//!   slots.rs   Slots, SlotId: generational handles (meshes, textures, entities)
//!   vfs.rs     Vfs: asset paths ("textures/a.png") → files under assets/
//!   watch.rs   Watcher: changed files, for hot reload
//!   keys.rs    Key, Button: input vocabulary shared by platform and engine
//!   error.rs   BoxError, chain(): error plumbing for every crate
//!   log.rs     logger setup (RUST_LOG)
//!   lib.rs     verify!: an assert that logs, and panics only in debug builds
//! ```

pub mod error;
pub mod image;
pub mod keys;
pub mod log;
pub mod slots;
pub mod vfs;
pub mod watch;

pub use error::{BoxError, chain};
pub use image::{Image, ImageError};
pub use keys::{Button, Key};
pub use slots::{SlotId, Slots};
pub use vfs::{Vfs, VfsError};
pub use watch::{WatchError, Watcher};

#[doc(hidden)]
pub use ::log as __log;

/// Checks an invariant. Logs on failure; panics only in debug builds.
///
/// `verify!(id < len, "mesh {id} out of range");`
#[macro_export]
macro_rules! verify {
    ($cond:expr, $($arg:tt)+) => {
        if !$cond {
            $crate::__log::error!(
                "verify failed: `{}` at {}:{}: {}",
                stringify!($cond),
                file!(),
                line!(),
                format_args!($($arg)+)
            );

            if cfg!(debug_assertions) {
                panic!("verify failed: {}", stringify!($cond));
            }
        }
    };
}

/// Fresh, empty scratch directory for tests that touch the filesystem.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join("engine-base-tests")
        .join(format!("{name}-{}", std::process::id()));

    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    #[test]
    fn verify_passes_when_true() {
        verify!(1 + 1 == 2, "math broke");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "verify failed")]
    fn verify_panics_in_debug_when_false() {
        verify!(1 + 1 == 3, "expected");
    }
}
