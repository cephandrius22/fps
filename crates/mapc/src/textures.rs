//! The level's texture table, and what the compiler needs to know of
//! each texture's PNG.
//!
//! Faces store texel coordinates; the engine divides by the texture's
//! size, so the compiler records each size alongside the name. The
//! baker needs each texture's albedo (how much light it reflects).
//! Both come from `<dir>/<name>.png`, the same files the engine draws.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base::Image;
use level::Texture;

const PNG_EXTENSION: &str = "png";
/// Albedo of a texture with no PNG: a mid-gray surface.
const DEFAULT_ALBEDO: f64 = 0.5;

/// A texture's width and height in texels, by name; None when unknown.
pub type TextureSizes<'a> = dyn Fn(&str) -> Option<[u32; 2]> + 'a;

/// Sizes of `<dir>/<lowercase name>.png`.
pub fn png_sizes(dir: &Path) -> impl Fn(&str) -> Option<[u32; 2]> + use<> {
    let dir = dir.to_path_buf();
    move |name| png(&dir, name).map(|image| [image.width(), image.height()])
}

/// Albedos of `<dir>/<lowercase name>.png`: mean brightness in linear
/// light, mid-gray without a PNG. For `bake::bake`.
pub fn png_albedos(dir: &Path) -> impl Fn(&str) -> f64 + use<> {
    let dir = dir.to_path_buf();
    move |name| png(&dir, name).map_or(DEFAULT_ALBEDO, |image| f64::from(image.luminance()))
}

fn png(dir: &Path, name: &str) -> Option<Image> {
    let path: PathBuf = dir.join(name.to_lowercase()).with_extension(PNG_EXTENSION);
    let bytes = std::fs::read(&path).ok()?;
    base::image::read_png(&bytes)
        .inspect_err(|err| log::warn!("{}: {}", path.display(), base::chain(err)))
        .ok()
}

/// Textures in first-use order, each listed once.
pub(crate) struct TextureTable<'a> {
    sizes: &'a TextureSizes<'a>,
    textures: Vec<Texture>,
    index: HashMap<String, usize>,
}

impl<'a> TextureTable<'a> {
    pub(crate) fn new(sizes: &'a TextureSizes<'a>) -> Self {
        Self {
            sizes,
            textures: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// The index of `name`, adding it on first use. An unknown size is
    /// recorded as 0 × 0, which the engine replaces with a default.
    pub(crate) fn index(&mut self, name: &str) -> usize {
        if let Some(&index) = self.index.get(name) {
            return index;
        }

        let [width, height] = (self.sizes)(name).unwrap_or_else(|| {
            log::warn!("texture {name:?} has no size (no PNG?): engine default used");
            [0, 0]
        });
        let index = self.textures.len();
        self.textures.push(Texture {
            name: name.to_owned(),
            width,
            height,
        });
        self.index.insert(name.to_owned(), index);
        index
    }

    pub(crate) fn into_textures(self) -> Vec<Texture> {
        self.textures
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_listed_once_in_first_use_order() {
        let sizes = |name: &str| (name == "floor").then_some([64, 32]);
        let mut table = TextureTable::new(&sizes);

        assert_eq!(table.index("crate"), 0);
        assert_eq!(table.index("floor"), 1);
        assert_eq!(table.index("crate"), 0);

        let textures = table.into_textures();
        assert_eq!((textures[0].width, textures[0].height), (0, 0), "unknown");
        assert_eq!((textures[1].width, textures[1].height), (64, 32));
    }

    #[test]
    fn sizes_and_albedos_come_from_the_asset_textures() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
        let sizes = png_sizes(&dir);

        let [width, height] = sizes("FLOOR").expect("assets/textures/floor.png");
        assert!(width > 0 && height > 0);
        assert_eq!(sizes("no-such-texture"), None);

        let albedos = png_albedos(&dir);
        assert!((0.0..1.0).contains(&albedos("floor")) && albedos("floor") > 0.0);
        assert_eq!(albedos("no-such-texture"), DEFAULT_ALBEDO);
    }
}
