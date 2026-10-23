//! Engine failures, by category. Lower-layer errors stay opaque causes
//! (see the source chain), so games never depend on render or platform
//! types.

use base::{BoxError, VfsError};
use render::RenderError;

/// Why the engine could not start or keep running.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The asset directory was not found.
    #[error("find assets")]
    Assets(#[from] VfsError),
    /// The window or OS event loop failed.
    #[error("window system")]
    Window(#[source] BoxError),
    /// No usable GPU. Usually fatal; worth a friendly message.
    #[error("no compatible GPU")]
    NoGpu(#[source] BoxError),
    /// The renderer failed to start, e.g. a shipped shader is broken.
    #[error("start renderer")]
    Renderer(#[source] BoxError),
    /// The game's own `init` failed.
    #[error("game init failed")]
    Game(#[source] BoxError),
}

impl From<RenderError> for EngineError {
    fn from(err: RenderError) -> Self {
        match err {
            RenderError::NoAdapter(_) => Self::NoGpu(err.into()),
            other => Self::Renderer(other.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync + 'static>() {}

    #[test]
    fn error_is_thread_safe() {
        assert_send_sync::<EngineError>();
    }

    #[test]
    fn render_errors_map_to_categories() {
        let surface = EngineError::from(RenderError::SurfaceUnsupported);
        assert!(matches!(surface, EngineError::Renderer(_)));

        // Cause survives the conversion.
        assert_eq!(
            base::chain(&surface).to_string(),
            "start renderer: surface unsupported by adapter"
        );
    }

    #[test]
    fn game_error_keeps_its_cause() {
        let err = EngineError::Game("level missing".into());
        assert_eq!(
            base::chain(&err).to_string(),
            "game init failed: level missing"
        );
    }
}
