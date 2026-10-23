//! GPU → CPU readback of the low-res target, for screenshots and
//! automated image checks.
//!
//!   color texture ─copy─► staging buffer (rows padded to 256 B) ─map─► RGBA

use anyhow::{Context, Result};

use crate::target::LowRes;

const BYTES_PER_PIXEL: u32 = 4;

/// Tightly packed RGBA8 pixels, sRGB-encoded, top row first.
pub struct Capture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

impl Capture {
    /// RGBA at (x, y), or None outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let i = ((y * self.width + x) * BYTES_PER_PIXEL) as usize;
        self.rgba[i..i + 4].try_into().ok()
    }
}

pub(crate) fn read(device: &wgpu::Device, queue: &wgpu::Queue, target: &LowRes) -> Result<Capture> {
    let row_bytes = target.width * BYTES_PER_PIXEL;
    let padded_row = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture-staging"),
        size: u64::from(padded_row * target.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        target.color_texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(target.height),
            },
        },
        target.color_texture.size(),
    );
    queue.submit([encoder.finish()]);

    // Map and block until the copy lands.
    let (tx, rx) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    })?;
    rx.recv().context("map callback dropped")??;

    let mapped = staging.slice(..).get_mapped_range()?;
    let rgba = strip_padding(&mapped, row_bytes as usize, padded_row as usize);

    Ok(Capture {
        width: target.width,
        height: target.height,
        rgba,
    })
}

/// Drops the alignment bytes at the end of each row.
fn strip_padding(padded: &[u8], row_bytes: usize, padded_row: usize) -> Vec<u8> {
    padded
        .chunks_exact(padded_row)
        .flat_map(|row| &row[..row_bytes])
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_padding_keeps_row_prefixes() {
        // 2 rows of 3 bytes, padded to 4: [a a a _][b b b _].
        let padded = [1, 2, 3, 0, 4, 5, 6, 0];
        assert_eq!(strip_padding(&padded, 3, 4), [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn pixel_indexes_row_major_and_bounds_checks() {
        let capture = Capture {
            width: 2,
            height: 1,
            rgba: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };

        assert_eq!(capture.pixel(1, 0), Some([5, 6, 7, 8]));
        assert_eq!(capture.pixel(2, 0), None);
    }
}
