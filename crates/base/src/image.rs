//! RGBA8 images and PNG file I/O. Pixels are sRGB-encoded, row-major,
//! top row first: the layout textures and screenshots both use.

use std::fs::File;
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};

const BYTES_PER_PIXEL: usize = 4;
const CHANNEL_MAX: f32 = u8::MAX as f32;
/// Rec. 709 luminance weights for linear red, green and blue.
const LUMA_R: f32 = 0.2126;
const LUMA_G: f32 = 0.7152;
const LUMA_B: f32 = 0.0722;

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

    /// Next mip level: half size (rounded down, at least 1), each pixel
    /// the average of up to 2×2 source pixels. Color is averaged in
    /// linear light, so a black/white checker becomes mid-gray (188),
    /// not the too-dark 128. Alpha is averaged as is.
    pub fn half(&self) -> Image {
        let width = (self.width / 2).max(1);
        let height = (self.height / 2).max(1);
        let mut rgba = Vec::with_capacity(width as usize * height as usize * BYTES_PER_PIXEL);

        for y in 0..height {
            for x in 0..width {
                rgba.extend(self.box_average(x * 2, y * 2));
            }
        }

        Self {
            width,
            height,
            rgba,
        }
    }

    /// Mean brightness in linear light, 0 (black) to 1 (white): how
    /// much light a surface of this texture reflects. Rec. 709 luminance
    /// weights; alpha ignored.
    pub fn luminance(&self) -> f32 {
        let pixels = self.rgba.as_chunks::<BYTES_PER_PIXEL>().0;
        let total: f32 = pixels
            .iter()
            .map(|&[r, g, b, _]| {
                let [r, g, b] = [r, g, b].map(srgb_to_linear);
                LUMA_R * r + LUMA_G * g + LUMA_B * b
            })
            .sum();
        total / pixels.len() as f32
    }

    /// Average of the 2×2 block at (x, y), clipped to the image: a 1-wide
    /// image averages vertical pairs only.
    fn box_average(&self, x: u32, y: u32) -> [u8; 4] {
        let mut sum = [0.0_f32; 4];
        let mut count = 0.0;

        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let Some(px) = self.pixel(x + dx, y + dy) else {
                continue;
            };

            for c in 0..3 {
                sum[c] += srgb_to_linear(px[c]);
            }
            sum[3] += f32::from(px[3]) / CHANNEL_MAX;
            count += 1.0;
        }

        let [r, g, b, a] = sum.map(|total| total / count);
        [
            linear_to_srgb(r),
            linear_to_srgb(g),
            linear_to_srgb(b),
            unorm_to_u8(a),
        ]
    }
}

/// sRGB-encoded byte → linear 0..1 (IEC 61966-2-1 transfer curve).
fn srgb_to_linear(c: u8) -> f32 {
    let c = f32::from(c) / CHANNEL_MAX;

    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear 0..1 → sRGB-encoded byte; inverse of `srgb_to_linear`.
fn linear_to_srgb(c: f32) -> u8 {
    let encoded = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };

    unorm_to_u8(encoded)
}

fn unorm_to_u8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * CHANNEL_MAX).round() as u8
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
    encode(BufWriter::new(file), image)
}

/// `image` as PNG bytes, e.g. to embed in another file.
pub fn encode_png(image: &Image) -> Result<Vec<u8>, ImageError> {
    let mut bytes = Vec::new();
    encode(&mut bytes, image)?;
    Ok(bytes)
}

fn encode(out: impl std::io::Write, image: &Image) -> Result<(), ImageError> {
    let mut encoder = png::Encoder::new(out, image.width, image.height);
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
    fn encoded_png_decodes_to_the_same_image() {
        let image = Image::new(2, 1, [[1, 2, 3, 4], [250, 251, 252, 253]].concat()).unwrap();
        let bytes = encode_png(&image).unwrap();

        assert_eq!(read_png(&bytes).unwrap(), image);
    }

    #[test]
    fn luminance_is_linear_mean_brightness() {
        let white = Image::solid([255; 4]);
        let black = Image::solid([0, 0, 0, 255]);
        assert!((white.luminance() - 1.0).abs() < 1e-6);
        assert_eq!(black.luminance(), 0.0);

        // Half black, half white: half the light, not sRGB 128's 22%.
        let checker = Image::new(2, 1, [[0, 0, 0, 255], [255; 4]].concat()).unwrap();
        assert!((checker.luminance() - 0.5).abs() < 1e-6);

        let green = Image::solid([0, 255, 0, 255]);
        assert!((green.luminance() - LUMA_G).abs() < 1e-6);
    }

    #[test]
    fn half_averages_in_linear_light() {
        // Black/white checker: linear mean 0.5, which sRGB encodes as 188.
        #[rustfmt::skip]
        let checker = Image::new(2, 2, vec![
            0, 0, 0, 255,          255, 255, 255, 255,
            255, 255, 255, 255,    0, 0, 0, 255,
        ]).unwrap();

        let half = checker.half();

        assert_eq!((half.width(), half.height()), (1, 1));
        assert_eq!(half.pixel(0, 0), Some([188, 188, 188, 255]));
    }

    #[test]
    fn half_keeps_uniform_colors_exact() {
        let image = Image::new(4, 2, [7, 99, 200, 128].repeat(8)).unwrap();
        let half = image.half();

        assert_eq!((half.width(), half.height()), (2, 1));
        assert!(
            half.rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|px| *px == [7, 99, 200, 128])
        );
    }

    #[test]
    fn half_of_thin_image_stops_at_one_pixel() {
        let column = gradient(1, 4);
        let half = column.half();
        assert_eq!((half.width(), half.height()), (1, 2));

        let dot = Image::solid([1, 2, 3, 4]);
        assert_eq!(dot.half(), dot);
    }

    #[test]
    fn srgb_round_trips_every_byte() {
        for c in 0..=u8::MAX {
            assert_eq!(linear_to_srgb(srgb_to_linear(c)), c);
        }
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
