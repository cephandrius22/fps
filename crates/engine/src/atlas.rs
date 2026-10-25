//! Packs many small images (lightmaps) into one atlas, so a whole map's
//! baked light is a single texture and draws never switch lightmaps.
//!
//! Shelf packing: blocks are placed tallest first, left to right in rows
//! ("shelves") as tall as their first block.
//!
//! ```text
//!   ┌──────┬────┬───┬──┐
//!   │  A   │ B  │ C │D │   shelf 0: as tall as A
//!   │      ├────┘   └──┘
//!   ├────┬─┴─┬────┐       shelf 1: as tall as E
//!   │ E  │ F │ G  │
//!   └────┴───┴────┘
//! ```
//!
//! Lightmap coordinates sit on luxel centers, so bilinear sampling never
//! reaches a neighbor and blocks need no padding.

use base::Image;
use glam::UVec2;

/// Narrowest atlas; small maps get one row-friendly square-ish texture.
const MIN_WIDTH: u32 = 64;
const BYTES_PER_PIXEL: usize = 4;
const OPAQUE: u8 = u8::MAX;

/// A packed atlas: where each block went, and the image holding them.
pub(crate) struct Atlas {
    /// Top-left corner per block, in input order.
    pub positions: Vec<UVec2>,
    pub image: Image,
}

/// A gray block to pack: `width × height` luxels, row-major.
pub(crate) struct Block<'a> {
    pub width: u32,
    pub height: u32,
    pub luxels: &'a [u8],
}

/// Packs `blocks` into one opaque gray RGBA image. None for no blocks.
pub(crate) fn pack(blocks: &[Block]) -> Option<Atlas> {
    let widest = blocks.iter().map(|b| b.width).max()?;
    let area: u32 = blocks.iter().map(|b| b.width * b.height).sum();
    // About square: width near √area, never narrower than the widest
    // block. A power of two keeps it friendly to every GPU.
    let width = (area as f32).sqrt().ceil() as u32;
    let width = width.max(widest).max(MIN_WIDTH).next_power_of_two();

    let (positions, height) = place(blocks, width);
    let mut rgba = vec![0; width as usize * height as usize * BYTES_PER_PIXEL];
    for (block, at) in blocks.iter().zip(&positions) {
        blit(&mut rgba, width, block, *at);
    }

    let image = Image::new(width, height, rgba).expect("atlas buffer matches its size");
    Some(Atlas { positions, image })
}

/// Shelf placement within `width`: positions in input order, and the
/// total height used.
fn place(blocks: &[Block], width: u32) -> (Vec<UVec2>, u32) {
    // Tallest first keeps shelves full; ties keep input order.
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(blocks[i].height));

    let mut positions = vec![UVec2::ZERO; blocks.len()];
    let (mut x, mut shelf_top, mut shelf_height) = (0, 0, 0);
    for i in order {
        let block = &blocks[i];
        if x + block.width > width {
            shelf_top += shelf_height;
            x = 0;
            shelf_height = 0;
        }

        positions[i] = UVec2::new(x, shelf_top);
        x += block.width;
        shelf_height = shelf_height.max(block.height);
    }

    (positions, shelf_top + shelf_height)
}

/// Copies a block's luxels into the atlas as opaque gray pixels.
fn blit(rgba: &mut [u8], atlas_width: u32, block: &Block, at: UVec2) {
    for (row, luxels) in block.luxels.chunks_exact(block.width as usize).enumerate() {
        let start =
            ((at.y as usize + row) * atlas_width as usize + at.x as usize) * BYTES_PER_PIXEL;
        let pixels = rgba[start..start + luxels.len() * BYTES_PER_PIXEL]
            .as_chunks_mut::<4>()
            .0;
        for (pixel, &v) in pixels.iter_mut().zip(luxels) {
            *pixel = [v, v, v, OPAQUE];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(width: u32, height: u32, value: u8) -> (u32, u32, Vec<u8>) {
        (width, height, vec![value; (width * height) as usize])
    }

    fn pack_owned(owned: &[(u32, u32, Vec<u8>)]) -> Atlas {
        let blocks: Vec<_> = owned
            .iter()
            .map(|(w, h, l)| Block {
                width: *w,
                height: *h,
                luxels: l,
            })
            .collect();
        pack(&blocks).unwrap()
    }

    #[test]
    fn nothing_to_pack_is_none() {
        assert!(pack(&[]).is_none());
    }

    #[test]
    fn blocks_never_overlap_and_stay_inside() {
        let owned: Vec<_> = (1..40)
            .map(|i| block(1 + i % 7, 1 + i % 5, i as u8))
            .collect();
        let atlas = pack_owned(&owned);
        let (w, h) = (atlas.image.width(), atlas.image.height());

        let rects: Vec<_> = owned
            .iter()
            .zip(&atlas.positions)
            .map(|((bw, bh, _), at)| (at.x, at.y, at.x + bw, at.y + bh))
            .collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(a.2 <= w && a.3 <= h, "block {i} outside {w}×{h}");
            for b in &rects[i + 1..] {
                let apart = a.2 <= b.0 || b.2 <= a.0 || a.3 <= b.1 || b.3 <= a.1;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn luxels_land_where_their_block_went() {
        let owned = [block(3, 2, 10), block(2, 4, 200)];
        let atlas = pack_owned(&owned);

        for ((bw, bh, luxels), at) in owned.iter().zip(&atlas.positions) {
            let corner = atlas.image.pixel(at.x + bw - 1, at.y + bh - 1).unwrap();
            assert_eq!(corner, [luxels[0], luxels[0], luxels[0], OPAQUE]);
        }
    }

    #[test]
    fn width_is_a_power_of_two_holding_the_widest_block() {
        let atlas = pack_owned(&[block(100, 1, 0), block(5, 5, 0)]);

        assert_eq!(atlas.image.width(), 128);
        assert!(atlas.image.width().is_power_of_two());
    }
}
