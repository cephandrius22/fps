//! Asset registry: which asset path backs which engine resource. Hot
//! reload looks changed paths up here instead of hard-coding them.
//! Scripts are found by listing `scripts/*.lua` at startup.
//!
//!   watcher ─► "textures/crate.png" ─► Registry ─► Asset::Texture(id)
//!                                                      └─► reload(): decode,
//!                                                          replace in place

use std::collections::HashMap;

use base::{BoxError, Image, Vfs};
use render::{Renderer, Shader, TextureId};

use crate::world::World;

const SCRIPT_DIR: &str = "scripts";
const SCRIPT_EXTENSION: &str = "lua";

/// A reloadable engine resource. Maps join here later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asset {
    Shader(Shader),
    Texture(TextureId),
    /// The renderer's one quantization palette.
    Palette,
    /// A Lua script, run in the world's VM.
    Script,
}

/// Reads and decodes an image asset. PNG only for now.
pub(crate) fn load_image(vfs: &Vfs, path: &str) -> Result<Image, BoxError> {
    let bytes = vfs.read(path)?;
    Ok(base::image::read_png(&bytes)?)
}

/// Runs every `scripts/*.lua` in `world`, in name order, and watches
/// them. A broken script is logged and skipped; fixing the file reloads
/// it. Scripts added while running load on the next start.
pub(crate) fn load_scripts(vfs: &Vfs, world: &mut World, registry: &mut Registry) {
    let paths = match vfs.list(SCRIPT_DIR, SCRIPT_EXTENSION) {
        Ok(paths) => paths,
        Err(err) => {
            log::error!("list scripts: {}", base::chain(&err));
            return;
        }
    };

    for path in paths {
        registry.register(&path, Asset::Script);

        let loaded = vfs
            .read_text(&path)
            .map_err(BoxError::from)
            .and_then(|source| Ok(world.load_script(&path, &source)?));
        match loaded {
            Ok(()) => log::info!("loaded {path}"),
            Err(err) => log::error!("script {path} failed: {}", base::chain(&*err)),
        }
    }
}

/// Re-reads `path` and swaps it into the renderer or world. On error the
/// old version stays active.
pub(crate) fn reload(
    renderer: &mut Renderer,
    world: &mut World,
    vfs: &Vfs,
    path: &str,
    asset: Asset,
) -> Result<(), BoxError> {
    match asset {
        Asset::Shader(shader) => {
            let source = vfs.read_text(path)?;
            renderer.reload_shader(shader, &source)?;
        }
        Asset::Texture(id) => {
            let image = load_image(vfs, path)?;
            renderer.replace_texture(id, &image)?;
        }
        Asset::Palette => {
            let image = load_image(vfs, path)?;
            renderer.set_palette(&image)?;
        }
        Asset::Script => {
            let source = vfs.read_text(path)?;
            world.load_script(path, &source)?;
        }
    }

    Ok(())
}

/// Where each shader's source lives, relative to the asset root.
pub(crate) fn shader_path(shader: Shader) -> &'static str {
    match shader {
        Shader::Mesh => "shaders/mesh.wgsl",
        Shader::Post => "shaders/post.wgsl",
        Shader::Blit => "shaders/blit.wgsl",
    }
}

pub(crate) struct Registry {
    by_path: HashMap<String, Asset>,
}

impl Registry {
    /// Registry preloaded with every built-in shader.
    pub fn with_shaders() -> Self {
        let mut registry = Self {
            by_path: HashMap::new(),
        };

        for shader in Shader::ALL {
            registry.register(shader_path(shader), Asset::Shader(shader));
        }

        registry
    }

    /// Maps `path` to `asset`, replacing any previous mapping.
    pub fn register(&mut self, path: &str, asset: Asset) {
        self.by_path.insert(path.to_owned(), asset);
    }

    pub fn lookup(&self, path: &str) -> Option<Asset> {
        self.by_path.get(path).copied()
    }

    /// Drops every path mapped to `asset`, e.g. after it is destroyed.
    pub fn forget(&mut self, asset: Asset) {
        self.by_path.retain(|_, mapped| *mapped != asset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shader_is_registered_under_its_path() {
        let registry = Registry::with_shaders();

        for shader in Shader::ALL {
            assert_eq!(
                registry.lookup(shader_path(shader)),
                Some(Asset::Shader(shader))
            );
        }
    }

    #[test]
    fn forget_removes_only_that_asset() {
        let mut registry = Registry::with_shaders();
        registry.forget(Asset::Shader(Shader::Mesh));

        assert_eq!(registry.lookup(shader_path(Shader::Mesh)), None);
        assert!(registry.lookup(shader_path(Shader::Blit)).is_some());
    }

    #[test]
    fn unknown_paths_miss() {
        let registry = Registry::with_shaders();
        assert_eq!(registry.lookup("textures/brick.png"), None);
    }

    fn repo_assets() -> Vfs {
        Vfs::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")).unwrap()
    }

    #[test]
    fn load_image_decodes_shipped_texture() {
        let image = load_image(&repo_assets(), "textures/crate.png").unwrap();
        assert_eq!((image.width(), image.height()), (64, 64));
    }

    #[test]
    fn load_image_reports_missing_and_garbage_files() {
        let vfs = repo_assets();

        let missing = load_image(&vfs, "textures/nope.png").unwrap_err();
        assert!(missing.to_string().contains("nope.png"), "{missing}");

        // A shader is not a PNG.
        assert!(load_image(&vfs, "shaders/mesh.wgsl").is_err());
    }

    #[test]
    fn reload_replaces_texture_and_keeps_old_on_error() {
        let vfs = repo_assets();
        let source = |shader| vfs.read_text(shader_path(shader));
        let Some(mut renderer) = render::test_support::headless_or_skip(Default::default(), source)
        else {
            return;
        };
        let id = renderer.create_texture(&Image::solid([0; 4])).unwrap();
        let asset = Asset::Texture(id);

        reload(
            &mut renderer,
            &mut World::new(),
            &vfs,
            "textures/crate.png",
            asset,
        )
        .unwrap();
        assert!(
            reload(
                &mut renderer,
                &mut World::new(),
                &vfs,
                "textures/nope.png",
                asset
            )
            .is_err()
        );
        // Handle survives both: the failed reload left the texture in place.
        assert!(renderer.destroy_texture(id));
    }

    #[test]
    fn reload_swaps_palette_and_rejects_oversized_one() {
        let vfs = repo_assets();
        let source = |shader| vfs.read_text(shader_path(shader));
        let Some(mut renderer) = render::test_support::headless_or_skip(Default::default(), source)
        else {
            return;
        };

        reload(
            &mut renderer,
            &mut World::new(),
            &vfs,
            "palettes/default.png",
            Asset::Palette,
        )
        .unwrap();
        // 64×64 = 4096 colors, over the 256 limit.
        assert!(
            reload(
                &mut renderer,
                &mut World::new(),
                &vfs,
                "textures/crate.png",
                Asset::Palette
            )
            .is_err()
        );
    }

    #[test]
    fn reload_rejects_broken_shader() {
        let vfs = repo_assets();
        let source = |shader| vfs.read_text(shader_path(shader));
        let Some(mut renderer) = render::test_support::headless_or_skip(Default::default(), source)
        else {
            return;
        };

        // A PNG is not WGSL: read_text fails on invalid UTF-8.
        let asset = Asset::Shader(Shader::Mesh);
        assert!(
            reload(
                &mut renderer,
                &mut World::new(),
                &vfs,
                "textures/crate.png",
                asset
            )
            .is_err()
        );
        reload(
            &mut renderer,
            &mut World::new(),
            &vfs,
            shader_path(Shader::Mesh),
            asset,
        )
        .unwrap();
    }

    #[test]
    fn shipped_scripts_load_and_register() {
        let vfs = repo_assets();
        let mut world = World::new();
        let mut registry = Registry::with_shaders();

        let scripts = vfs.list(SCRIPT_DIR, SCRIPT_EXTENSION).unwrap();
        assert!(!scripts.is_empty(), "no scripts under assets/{SCRIPT_DIR}");
        for path in &scripts {
            let source = vfs.read_text(path).unwrap();
            world
                .load_script(path, &source)
                .unwrap_or_else(|err| panic!("{}", base::chain(&err)));
        }

        load_scripts(&vfs, &mut world, &mut registry);
        assert_eq!(registry.lookup(&scripts[0]), Some(Asset::Script));
    }

    #[test]
    fn script_reload_reports_lua_errors() {
        let source = |shader| repo_assets().read_text(shader_path(shader));
        let Some(mut renderer) = render::test_support::headless_or_skip(Default::default(), source)
        else {
            return;
        };

        // A shader is not Lua.
        let mut world = World::new();
        let broken = reload(
            &mut renderer,
            &mut world,
            &repo_assets(),
            shader_path(Shader::Mesh),
            Asset::Script,
        );
        assert!(broken.is_err());
    }

    #[test]
    fn shader_paths_exist_on_disk() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");

        for shader in Shader::ALL {
            let path = root.join(shader_path(shader));
            assert!(path.is_file(), "missing {}", path.display());
        }
    }
}
