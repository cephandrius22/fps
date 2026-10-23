//! Asset registry: which asset path backs which engine resource. Hot
//! reload looks changed paths up here instead of hard-coding them.
//!
//!   watcher ─► "shaders/mesh.wgsl" ─► Registry ─► Asset::Shader(Mesh)
//!                                                     └─► renderer reload

use std::collections::HashMap;

use render::Shader;

/// A reloadable engine resource. Textures, scripts and maps join here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asset {
    Shader(Shader),
}

/// Where each shader's source lives, relative to the asset root.
pub(crate) fn shader_path(shader: Shader) -> &'static str {
    match shader {
        Shader::Mesh => "shaders/mesh.wgsl",
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
    fn unknown_paths_miss() {
        let registry = Registry::with_shaders();
        assert_eq!(registry.lookup("textures/brick.png"), None);
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
