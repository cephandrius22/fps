//! Image file output.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// Why an image could not be written.
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    /// The file or its directory could not be created.
    #[error("create {path}")]
    Create {
        /// Target path.
        path: PathBuf,
        /// OS error.
        #[source]
        source: std::io::Error,
    },
    /// Encoding failed, e.g. pixel count does not match the size.
    #[error("encode png")]
    Encode(#[from] png::EncodingError),
}

/// Writes tightly packed RGBA8 (sRGB) pixels as PNG. Creates parent dirs.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), ImageError> {
    let create = |source| ImageError::Create {
        path: path.to_path_buf(),
        source,
    };

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(create)?;
    }

    let file = File::create(path).map_err(create)?;
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
    fn wrong_buffer_size_is_an_encode_error() {
        let path = crate::test_dir("png-bad").join("out.png");
        let err = write_png(&path, 4, 4, &[0; 3]).unwrap_err();

        assert!(matches!(err, ImageError::Encode(_)));
    }

    #[test]
    fn unwritable_path_is_a_create_error() {
        // A regular file cannot be a parent directory.
        let blocker = crate::test_dir("png-blocked").join("file");
        std::fs::write(&blocker, "").unwrap();

        let err = write_png(&blocker.join("out.png"), 1, 1, &[0; 4]).unwrap_err();
        assert!(matches!(err, ImageError::Create { .. }));
    }
}
