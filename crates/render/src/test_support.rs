//! Helpers for GPU tests in any crate. Enabled by the `test-support`
//! feature, as a dev-dependency only.
//!
//! One policy for every GPU test: skip when the machine has no GPU,
//! fail on anything else, and never skip under $REQUIRE_GPU.

use std::io::Write;
use std::sync::Once;

use base::BoxError;

use crate::{RenderConfig, RenderError, Renderer, Shader};

/// Set to make a missing GPU fail tests instead of skipping them.
const REQUIRE_GPU_ENV: &str = "REQUIRE_GPU";
/// Set to "true" by GitHub Actions.
const CI_ENV: &str = "GITHUB_ACTIONS";

/// One CI warning per test binary, not one per skipped test.
static ANNOUNCE_SKIP: Once = Once::new();

/// Headless renderer, or None when the test should skip (no GPU).
/// Panics on any other failure.
pub fn headless_or_skip<E: Into<BoxError>>(
    config: RenderConfig,
    source: impl Fn(Shader) -> Result<String, E>,
) -> Option<Renderer> {
    match Renderer::headless(config, source) {
        Ok(renderer) => Some(renderer),
        Err(err) => {
            skip_or_fail(err);
            None
        }
    }
}

/// Handles a failed renderer start. Only a missing GPU may skip a test,
/// and not under $REQUIRE_GPU; anything else (e.g. a broken shipped
/// shader) is a real failure.
pub fn skip_or_fail(err: RenderError) {
    let reason = base::chain(&err).to_string();

    if !matches!(err, RenderError::NoAdapter(_)) {
        panic!("renderer failed to start: {reason}");
    }
    if std::env::var_os(REQUIRE_GPU_ENV).is_some() {
        panic!("{REQUIRE_GPU_ENV} set but no GPU: {reason}");
    }

    eprintln!("skipping GPU test: {reason}");
    announce_skip(&reason);
}

/// Turns a skip into a warning on the GitHub Actions run page. libtest
/// swallows print!/eprintln! output from passing tests, but a direct
/// stdout write reaches the CI log, where `::warning` is picked up.
fn announce_skip(reason: &str) {
    if std::env::var(CI_ENV).as_deref() != Ok("true") {
        return;
    }

    ANNOUNCE_SKIP.call_once(|| {
        let os = std::env::consts::OS;
        let _ = writeln!(
            std::io::stdout(),
            "::warning title=GPU tests skipped::No GPU adapter on {os}; GPU tests did not run. {}",
            escape_command(reason)
        );
    });
}

/// Workflow-command escaping: %, CR and LF would end or corrupt the line.
fn escape_command(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_command_encodes_line_breaks_and_percent() {
        assert_eq!(escape_command("50%\r\nnext"), "50%25%0D%0Anext");
    }

    #[test]
    #[should_panic(expected = "renderer failed to start")]
    fn non_gpu_errors_fail() {
        skip_or_fail(RenderError::SurfaceUnsupported);
    }
}
