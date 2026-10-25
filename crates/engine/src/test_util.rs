//! Shared test fixtures: the repository's real assets, a headless
//! renderer that loads its shaders, and reference-image comparison.

use std::path::{Path, PathBuf};

use base::{Image, Vfs};
use render::RenderConfig;
use render::test_support::GpuTest;

use crate::assets::shader_path;

/// Set to rewrite reference images from the current render.
const UPDATE_ENV: &str = "UPDATE_REFERENCE";
/// A channel this far off counts the pixel as different. GPUs disagree
/// by a few levels on filtering and blending.
const PIXEL_TOLERANCE: u8 = 24;
/// Share of pixels allowed to differ: triangle edges rasterize slightly
/// differently between GPUs.
const MAX_DIFFERENT: f32 = 0.01;
/// Mean channel difference allowed over the whole image.
const MAX_MEAN_DIFFERENCE: f32 = 3.0;

/// The repository's `assets/` directory.
pub(crate) fn repo_assets() -> Vfs {
    Vfs::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")).unwrap()
}

/// Headless renderer with the shipped shaders, or None (the test skips)
/// without a GPU.
pub(crate) fn headless(vfs: &Vfs) -> Option<GpuTest> {
    headless_with(vfs, RenderConfig::default())
}

/// As `headless`, with a chosen config (e.g. a smaller target).
pub(crate) fn headless_with(vfs: &Vfs, config: RenderConfig) -> Option<GpuTest> {
    let source = |shader| vfs.read_text(shader_path(shader));
    render::test_support::headless_or_skip(config, source)
}

/// Compares a render against the reference PNG at `reference`, within
/// GPU-to-GPU tolerances. With $UPDATE_REFERENCE set, rewrites the
/// reference instead. On a mismatch, saves the render under
/// target/reference/ and panics with the numbers.
pub(crate) fn assert_matches_reference(actual: &Image, reference: &Path) {
    if std::env::var_os(UPDATE_ENV).is_some() {
        base::image::write_png(reference, actual).unwrap();
        return;
    }

    let bytes = std::fs::read(reference).unwrap_or_else(|_| {
        panic!(
            "no reference image at {}; create it with {UPDATE_ENV}=1",
            reference.display()
        )
    });
    let expected = base::image::read_png(&bytes).unwrap();
    let diff = difference(actual, &expected);
    if diff.passes() {
        return;
    }

    let saved = failure_path(reference);
    base::image::write_png(&saved, actual).unwrap();
    panic!(
        "render differs from {}: {diff:?}; actual saved to {}. If the change is \
         intended, rerun with {UPDATE_ENV}=1.",
        reference.display(),
        saved.display()
    );
}

/// How far two images are apart.
#[derive(Debug)]
struct Difference {
    /// Share of pixels with a channel off by more than PIXEL_TOLERANCE;
    /// 1.0 when the sizes differ.
    different: f32,
    /// Mean absolute channel difference.
    mean: f32,
}

impl Difference {
    fn passes(&self) -> bool {
        self.different <= MAX_DIFFERENT && self.mean <= MAX_MEAN_DIFFERENCE
    }
}

fn difference(a: &Image, b: &Image) -> Difference {
    if (a.width(), a.height()) != (b.width(), b.height()) {
        return Difference {
            different: 1.0,
            mean: f32::INFINITY,
        };
    }

    let pairs = a
        .rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.rgba().as_chunks::<4>().0);
    let (mut different, mut total) = (0usize, 0u64);
    for (pa, pb) in pairs {
        let deltas = [0, 1, 2].map(|c| pa[c].abs_diff(pb[c]));
        total += deltas.iter().map(|&d| u64::from(d)).sum::<u64>();
        different += usize::from(deltas.iter().any(|&d| d > PIXEL_TOLERANCE));
    }

    let pixels = (a.width() * a.height()) as f32;
    Difference {
        different: different as f32 / pixels,
        mean: total as f32 / (pixels * 3.0),
    }
}

/// target/reference/<name>.actual.png, beside the build output.
fn failure_path(reference: &Path) -> PathBuf {
    let name = reference.file_stem().unwrap_or_default().to_string_lossy();
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/reference")
        .join(format!("{name}.actual.png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(value: u8, size: u32) -> Image {
        Image::new(
            size,
            size,
            [value, value, value, 255].repeat((size * size) as usize),
        )
        .unwrap()
    }

    #[test]
    fn small_noise_passes_and_big_changes_fail() {
        assert!(difference(&gray(100, 8), &gray(102, 8)).passes());
        assert!(!difference(&gray(100, 8), &gray(160, 8)).passes());
    }

    #[test]
    fn a_few_odd_pixels_pass_but_not_many() {
        let base = gray(100, 20);
        let with_odd = |count: usize| {
            let mut rgba = base.rgba().to_vec();
            for px in rgba.as_chunks_mut::<4>().0.iter_mut().take(count) {
                *px = [255, 255, 255, 255];
            }
            Image::new(20, 20, rgba).unwrap()
        };

        // 400 pixels: 4 is 1%, 40 is 10%.
        assert!(difference(&base, &with_odd(2)).passes());
        assert!(!difference(&base, &with_odd(40)).passes());
    }

    #[test]
    fn size_mismatch_fails() {
        assert!(!difference(&gray(0, 4), &gray(0, 8)).passes());
    }
}
