//! Foundation layer shared by every other crate.

pub mod image;
pub mod log;
pub mod vfs;
pub mod watch;

pub use vfs::Vfs;
pub use watch::Watcher;

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
