//! RGBA8 images and PNG file I/O. Pixels are sRGB-encoded, row-major,
//! top row first: the layout textures and screenshots both use.

use std::fs::File;
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};

const BYTES_PER_PIXEL: usize = 4;

/// Why an image could not be built, read or written.
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    /// Zero size, or pixel bytes do not match width × height × 4.
    #[error("invalid image size {width}×{height} for {len} bytes")]
    Size {
        /// Claimed width.
        width: u32,
        /// Claimed height.
        height: u32,
        /// Bytes supplied.
        len: usize,
    },
    /// The file or its directory could not be created.
    #[error("create {path}")]
    Create {
        /// Target path.
        path: PathBuf,
        /// OS error.
        #[source]
        source: std::io::Error,
    },
    /// Encoding failed.
    #[error("encode png")]
    Encode(#[from] png::EncodingError),
    /// The bytes are not a readable PNG.
    #[error("decode png")]
    Decode(#[from] png::DecodingError),
}

/// RGBA8 image whose pixel buffer always matches its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    /// Wraps `rgba` (width × height × 4 bytes). Fails on zero size or a
    /// length mismatch.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, ImageError> {
        let expected = width as usize * height as usize * BYTES_PER_PIXEL;
        if width == 0 || height == 0 || rgba.len() != expected {
            return Err(ImageError::Size {
                width,
                height,
                len: rgba.len(),
            });
        }

        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// 1×1 image of one color, e.g. a neutral white texture.
    pub fn solid(rgba: [u8; 4]) -> Self {
        Self {
            width: 1,
            height: 1,
            rgba: rgba.to_vec(),
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Pixel bytes, RGBA, row-major, top row first.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// RGBA at (x, y), or None outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let i = (y as usize * self.width as usize + x as usize) * BYTES_PER_PIXEL;
        self.rgba[i..i + BYTES_PER_PIXEL].try_into().ok()
    }
}

/// Decodes a PNG of any color type and bit depth to RGBA8. Palette,
/// grayscale and RGB images are expanded; 16-bit channels are reduced.
/// Animated PNGs yield their first frame.
pub fn read_png(bytes: &[u8]) -> Result<Image, ImageError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());

    let mut reader = decoder.read_info()?;
    let size = reader
        .output_buffer_size()
        .ok_or(png::DecodingError::LimitsExceeded)?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());

    let rgba = to_rgba(buf, info.color_type);
    Image::new(info.width, info.height, rgba)
}

/// Expands 8-bit pixels of `color` to RGBA. Opaque where alpha is absent.
fn to_rgba(pixels: Vec<u8>, color: png::ColorType) -> Vec<u8> {
    const OPAQUE: u8 = u8::MAX;

    match color {
        png::ColorType::Rgba => pixels,
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, OPAQUE])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, OPAQUE]).collect(),
        // normalize_to_color8 expands palettes; never reached.
        png::ColorType::Indexed => pixels,
    }
}

/// Writes `image` as PNG. Creates parent dirs.
pub fn write_png(path: &Path, image: &Image) -> Result<(), ImageError> {
    let create = |source| ImageError::Create {
        path: path.to_path_buf(),
        source,
    };

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(create)?;
    }

    let file = File::create(path).map_err(create)?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    encoder.write_header()?.write_image_data(&image.rgba)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Image {
        let rgba = (0..width * height * 4).map(|i| (i * 7) as u8).collect();
        Image::new(width, height, rgba).unwrap()
    }

    /// Encodes raw 8-bit pixels of `color` with the png crate directly.
    fn encode(width: u32, height: u32, color: png::ColorType, pixels: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
        out
    }

    #[test]
    fn new_rejects_zero_size_and_wrong_length() {
        assert!(matches!(
            Image::new(0, 1, vec![]),
            Err(ImageError::Size { .. })
        ));
        assert!(matches!(
            Image::new(2, 2, vec![0; 3]),
            Err(ImageError::Size { .. })
        ));
        assert!(Image::new(2, 2, vec![0; 16]).is_ok());
    }

    #[test]
    fn pixel_indexes_row_major_and_bounds_checks() {
        let image = Image::new(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();

        assert_eq!(image.pixel(1, 0), Some([5, 6, 7, 8]));
        assert_eq!(image.pixel(0, 1), None);
        assert_eq!(Image::solid([9, 9, 9, 9]).pixel(0, 0), Some([9, 9, 9, 9]));
    }

    #[test]
    fn png_round_trips_and_creates_dirs() {
        let path = crate::test_dir("png").join("nested").join("out.png");
        let image = gradient(2, 3);

        write_png(&path, &image).unwrap();
        let back = read_png(&std::fs::read(&path).unwrap()).unwrap();

        assert_eq!(back, image);
    }

    #[test]
    fn rgb_png_gains_opaque_alpha() {
        let bytes = encode(1, 1, png::ColorType::Rgb, &[10, 20, 30]);
        let image = read_png(&bytes).unwrap();

        assert_eq!(image.pixel(0, 0), Some([10, 20, 30, 255]));
    }

    #[test]
    fn grayscale_pngs_expand_to_rgba() {
        let gray = read_png(&encode(1, 1, png::ColorType::Grayscale, &[77])).unwrap();
        assert_eq!(gray.pixel(0, 0), Some([77, 77, 77, 255]));

        let gray_alpha = read_png(&encode(1, 1, png::ColorType::GrayscaleAlpha, &[77, 9])).unwrap();
        assert_eq!(gray_alpha.pixel(0, 0), Some([77, 77, 77, 9]));
    }

    #[test]
    fn garbage_is_a_decode_error() {
        assert!(matches!(read_png(b"not a png"), Err(ImageError::Decode(_))));
    }

    #[test]
    fn unwritable_path_is_a_create_error() {
        // A regular file cannot be a parent directory.
        let blocker = crate::test_dir("png-blocked").join("file");
        std::fs::write(&blocker, "").unwrap();

        let err = write_png(&blocker.join("out.png"), &Image::solid([0; 4])).unwrap_err();
        assert!(matches!(err, ImageError::Create { .. }));
    }
}
