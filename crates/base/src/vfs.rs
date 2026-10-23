//! Asset filesystem. Callers use asset paths ("shaders/mesh.wgsl"),
//! never OS paths, so packaging can change later without touching them.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Overrides asset root discovery.
const ROOT_ENV: &str = "ENGINE_ASSETS";
const ROOT_DIR: &str = "assets";

#[derive(Clone, Debug)]
pub struct Vfs {
    root: PathBuf,
}

impl Vfs {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let root = root
            .canonicalize()
            .with_context(|| format!("asset root {}", root.display()))?;

        Ok(Self { root })
    }

    /// Finds `assets/` via $ENGINE_ASSETS, then upward from the exe and cwd.
    pub fn discover() -> Result<Self> {
        if let Ok(dir) = std::env::var(ROOT_ENV) {
            return Self::new(dir);
        }

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let starts = [exe_dir, std::env::current_dir().ok()];

        for start in starts.into_iter().flatten() {
            if let Some(found) = find_upward(&start) {
                return Self::new(found);
            }
        }

        bail!("no `{ROOT_DIR}` directory found; set {ROOT_ENV}")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let full = self.root.join(path);
        std::fs::read(&full).with_context(|| format!("read {}", full.display()))
    }

    pub fn read_text(&self, path: &str) -> Result<String> {
        let full = self.root.join(path);
        std::fs::read_to_string(&full).with_context(|| format!("read {}", full.display()))
    }

    /// OS path → asset path, e.g. "/…/assets/shaders/a.wgsl" → "shaders/a.wgsl".
    pub fn asset_path(&self, os_path: &Path) -> Option<String> {
        let rel = os_path.strip_prefix(&self.root).ok()?;
        let parts: Vec<_> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect();

        Some(parts.join("/"))
    }
}

fn find_upward(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(ROOT_DIR))
        .find(|candidate| candidate.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_path_uses_forward_slashes() {
        let vfs = Vfs::new(env!("CARGO_MANIFEST_DIR")).unwrap();
        let os_path = vfs.root().join("src").join("vfs.rs");

        assert_eq!(vfs.asset_path(&os_path).as_deref(), Some("src/vfs.rs"));
        assert_eq!(vfs.asset_path(Path::new("/elsewhere/x")), None);
    }
}
