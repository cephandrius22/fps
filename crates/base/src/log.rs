//! Logger setup. Override with `RUST_LOG`, e.g. `RUST_LOG=debug`.

/// Engine at info; noisy GPU internals at warn.
const DEFAULT_FILTER: &str = "info,wgpu_core=warn,wgpu_hal=warn,naga=warn";

pub fn init() {
    let env = env_logger::Env::default().default_filter_or(DEFAULT_FILTER);

    // Ignore "already initialized" so tests and tools can call this freely.
    let _ = env_logger::Builder::from_env(env)
        .format_timestamp_millis()
        .try_init();
}
