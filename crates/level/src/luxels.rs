//! The luxel grid: how a face's lightmap lies over its texels. Every
//! map reader and light baker must size grids exactly alike, or faces
//! read each other's luxels; this is Quake's rule, as ericw-tools'
//! `light` applies it.
//!
//! ```text
//!   texels (s)   0      16      32      48
//!   luxels       ●───────●───────●───────●     a face spanning texels
//!                0       1       2       3     8..40 gets luxels 0..3:
//!                    ├────face────┤            bounds snap outward, and
//!                                              both edges get a sample
//! ```

use glam::{DVec2, Vec2};

/// Texels per luxel side: Quake's fixed lightmap scale.
pub const TEXELS_PER_LUXEL: f64 = 16.0;

/// A face's lightmap grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LuxelGrid {
    /// Luxel (0, 0)'s position in whole luxels from texel 0.
    pub origin: DVec2,
    /// Luxels across and down.
    pub size: [u32; 2],
}

impl LuxelGrid {
    /// The grid over a face with these corner texels. Texels are taken
    /// in double precision, as `light` computes them.
    pub fn new(texels: &[Vec2]) -> Self {
        let start = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
        let (lo, hi) = texels.iter().fold(start, |(lo, hi), t| {
            let t = t.as_dvec2();
            (lo.min(t), hi.max(t))
        });

        let origin = (lo / TEXELS_PER_LUXEL).floor();
        let end = (hi / TEXELS_PER_LUXEL).ceil();
        let size = (end - origin).as_uvec2() + 1;
        Self {
            origin,
            size: size.to_array(),
        }
    }

    /// Luxels in the grid.
    pub fn count(&self) -> usize {
        self.size[0] as usize * self.size[1] as usize
    }

    /// Where each texel falls in the grid, in luxels: luxel i's center
    /// is at i + 0.5. These are a [`Lightmap`](crate::Lightmap)'s
    /// `coords`.
    pub fn coords(&self, texels: &[Vec2]) -> Vec<Vec2> {
        texels
            .iter()
            .map(|t| (t.as_dvec2() / TEXELS_PER_LUXEL - self.origin + 0.5).as_vec2())
            .collect()
    }

    /// The texel luxel (x, y) samples.
    pub fn texel(&self, x: u32, y: u32) -> DVec2 {
        (self.origin + DVec2::new(f64::from(x), f64::from(y))) * TEXELS_PER_LUXEL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_outward_and_adds_an_edge() {
        // Texels 8..40 across, 0..16 down: luxels 0..3 and 0..1.
        let grid = LuxelGrid::new(&[Vec2::new(8.0, 0.0), Vec2::new(40.0, 16.0)]);

        assert_eq!(grid.origin, DVec2::ZERO);
        assert_eq!(grid.size, [4, 2]);
        assert_eq!(grid.count(), 8);
    }

    #[test]
    fn handles_negative_texels() {
        let grid = LuxelGrid::new(&[Vec2::new(-20.0, -16.0), Vec2::new(-4.0, 0.0)]);

        assert_eq!(grid.origin, DVec2::new(-2.0, -1.0));
        assert_eq!(grid.size, [3, 2]);
    }

    #[test]
    fn coords_and_samples_agree() {
        let texels = [Vec2::new(8.0, 0.0), Vec2::new(40.0, 16.0)];
        let grid = LuxelGrid::new(&texels);

        // Texel 8 is half a luxel in: center of luxel 0 plus a half.
        assert_eq!(
            grid.coords(&texels),
            [Vec2::new(1.0, 0.5), Vec2::new(3.0, 1.5)]
        );
        // Luxel 2 samples texel 32, whose coord is luxel 2's center.
        assert_eq!(grid.texel(2, 1), DVec2::new(32.0, 16.0));
        let center = grid.coords(&[grid.texel(2, 1).as_vec2()])[0];
        assert_eq!(center, Vec2::new(2.5, 1.5));
    }
}
