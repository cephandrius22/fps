//! Why .map text could not be read.

/// A syntax error, with where it was found.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct MapError {
    /// 1-based line.
    pub line: usize,
    /// What was expected, e.g. "expected '('".
    pub message: &'static str,
}

pub(crate) type Result<T, E = MapError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_names_the_line() {
        let err = MapError {
            line: 3,
            message: "expected '('",
        };

        assert_eq!(err.to_string(), "line 3: expected '('");
    }
}
