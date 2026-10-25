//! Shared test fixtures: the repository's real assets and a headless
//! renderer that loads its shaders.

use base::Vfs;
use render::test_support::GpuTest;

use crate::assets::shader_path;

/// The repository's `assets/` directory.
pub(crate) fn repo_assets() -> Vfs {
    Vfs::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")).unwrap()
}

/// Headless renderer with the shipped shaders, or None (the test skips)
/// without a GPU.
pub(crate) fn headless(vfs: &Vfs) -> Option<GpuTest> {
    let source = |shader| vfs.read_text(shader_path(shader));
    render::test_support::headless_or_skip(Default::default(), source)
}
