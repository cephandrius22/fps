//! GPU → CPU readback of the low-res target, for screenshots and
//! automated image checks.
//!
//!   color texture ─copy─► staging buffer (rows padded to 256 B) ─map─► RGBA

use anyhow::{Context, Result};

use crate::target::LowRes;

const BYTES_PER_PIXEL: u32 = 4;

/// Tightly packed RGBA8 pixels, sRGB-encoded, top row first.
pub struct Capture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
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

    // Strip row padding.
    let mapped = staging.slice(..).get_mapped_range()?;
    let mut rgba = Vec::with_capacity((row_bytes * target.height) as usize);
    for row in mapped.chunks_exact(padded_row as usize) {
        rgba.extend_from_slice(&row[..row_bytes as usize]);
    }

    Ok(Capture {
        width: target.width,
        height: target.height,
        rgba,
    })
}
