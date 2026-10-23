//! Image file output.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};

/// Writes tightly packed RGBA8 (sRGB) pixels as PNG. Creates parent dirs.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trips_and_creates_dirs() {
        let path = crate::test_dir("png").join("nested").join("out.png");
        let rgba: Vec<u8> = (0..2 * 3 * 4).map(|i| i as u8 * 10).collect();

        write_png(&path, 2, 3, &rgba).unwrap();

        let decoder = png::Decoder::new(std::io::BufReader::new(File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();

        assert_eq!((info.width, info.height), (2, 3));
        assert_eq!(&pixels[..info.buffer_size()], &rgba[..]);
    }

    #[test]
    fn wrong_buffer_size_is_an_error() {
        let path = crate::test_dir("png-bad").join("out.png");
        assert!(write_png(&path, 4, 4, &[0; 3]).is_err());
    }
}
