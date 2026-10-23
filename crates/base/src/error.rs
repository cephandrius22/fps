//! Error helpers shared by every crate.
//!
//! Library crates return typed errors (one enum per module, via
//! `thiserror`) so callers can match on what went wrong. Each error's
//! `Display` describes only its own level; `chain` prints the causes.

use std::error::Error;
use std::fmt;

/// Type-erased error for boundaries where the cause is caller-defined,
/// e.g. a game's own `init` failure or a shader source loader.
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Displays an error and all its causes on one line:
/// "read shaders/a.wgsl: No such file or directory (os error 2)".
pub fn chain<'a>(err: &'a (dyn Error + 'static)) -> impl fmt::Display + 'a {
    Chain(err)
}

struct Chain<'a>(&'a (dyn Error + 'static));

impl fmt::Display for Chain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;

        let mut source = self.0.source();
        while let Some(err) = source {
            write!(f, ": {err}")?;
            source = err.source();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("outer")]
    struct Outer(#[source] std::io::Error);

    #[test]
    fn chain_joins_every_cause() {
        let err = Outer(std::io::Error::other("inner"));
        assert_eq!(chain(&err).to_string(), "outer: inner");
    }

    #[test]
    fn chain_of_leaf_is_just_its_message() {
        let err = std::io::Error::other("alone");
        assert_eq!(chain(&err).to_string(), "alone");
    }
}
