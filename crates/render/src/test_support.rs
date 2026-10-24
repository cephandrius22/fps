//! Helpers for GPU tests in any crate. Enabled by the `test-support`
//! feature, as a dev-dependency only.
//!
//! One policy for every GPU test: skip when the machine has no GPU,
//! fail on anything else, and never skip under $REQUIRE_GPU.
//!
//! GPU tests also run one at a time per test binary. Creating and
//! destroying several devices at once on parallel test threads crashed
//! the D3D12 software adapter on Windows CI (access violation, about 1
//! run in 80). The game only ever has one device, so tests match that.

use std::io::Write;
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard, Once, PoisonError};

use base::BoxError;

use crate::{RenderConfig, RenderError, Renderer, Shader};

/// Set to make a missing GPU fail tests instead of skipping them.
const REQUIRE_GPU_ENV: &str = "REQUIRE_GPU";
/// Set to "true" by GitHub Actions.
const CI_ENV: &str = "GITHUB_ACTIONS";

/// One CI warning per test binary, not one per skipped test.
static ANNOUNCE_SKIP: Once = Once::new();

/// Held for as long as a test has a GPU device.
static GPU: Mutex<()> = Mutex::new(());

/// Waits until no other test in this binary holds a GPU device. Hold the
/// guard for the device's whole lifetime.
pub fn lock_gpu() -> MutexGuard<'static, ()> {
    // A failed test poisons the lock while holding it; the GPU itself is
    // fine, so later tests carry on.
    GPU.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A headless renderer that holds the GPU lock. Use it like a
/// `Renderer`; dropping it releases the lock after the device is gone.
pub struct GpuTest {
    // Field order matters: the renderer drops before the lock.
    renderer: Renderer,
    _lock: MutexGuard<'static, ()>,
}

impl Deref for GpuTest {
    type Target = Renderer;

    fn deref(&self) -> &Renderer {
        &self.renderer
    }
}

impl DerefMut for GpuTest {
    fn deref_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }
}

/// Headless renderer, or None when the test should skip (no GPU).
/// Panics on any other failure.
pub fn headless_or_skip<E: Into<BoxError>>(
    config: RenderConfig,
    source: impl Fn(Shader) -> Result<String, E>,
) -> Option<GpuTest> {
    let lock = lock_gpu();

    match Renderer::headless(config, source) {
        Ok(renderer) => Some(GpuTest {
            renderer,
            _lock: lock,
        }),
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

    #[test]
    fn gpu_lock_survives_a_panicking_holder() {
        let _ = std::thread::spawn(|| {
            let _held = lock_gpu();
            panic!("test failed while holding the GPU");
        })
        .join();

        drop(lock_gpu());
    }
}
