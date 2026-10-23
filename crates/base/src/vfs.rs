//! Asset filesystem. Callers use asset paths ("shaders/mesh.wgsl"),
//! never OS paths, so packaging can change later without touching them.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Overrides asset root discovery.
const ROOT_ENV: &str = "ENGINE_ASSETS";
const ROOT_DIR: &str = "assets";

/// Read-only view of the asset directory.
#[derive(Clone, Debug)]
pub struct Vfs {
    root: PathBuf,
}

impl Vfs {
    /// Uses `root` as the asset directory. Fails if it does not exist.
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

    /// Absolute, canonical asset directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whole file as bytes. `path` is an asset path, e.g. "textures/a.png".
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let full = self.root.join(path);
        std::fs::read(&full).with_context(|| format!("read {}", full.display()))
    }

    /// Whole file as UTF-8 text. `path` is an asset path.
    pub fn read_text(&self, path: &str) -> Result<String> {
        let full = self.root.join(path);
        std::fs::read_to_string(&full).with_context(|| format!("read {}", full.display()))
    }

    /// OS path → asset path, e.g. "/…/assets/shaders/a.wgsl" → "shaders/a.wgsl".
    /// None for paths outside the root.
    pub fn asset_path(&self, os_path: &Path) -> Option<String> {
        let rel = os_path.strip_prefix(&self.root).ok()?;
        let parts: Vec<_> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect();

        Some(parts.join("/"))
    }
}

/// Nearest `assets/` directory at or above `start`.
fn find_upward(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(ROOT_DIR))
        .find(|candidate| candidate.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// temp/assets/shaders/a.wgsl
    fn fixture(name: &str) -> Vfs {
        let root = crate::test_dir(name).join(ROOT_DIR);
        std::fs::create_dir_all(root.join("shaders")).unwrap();
        std::fs::write(root.join("shaders").join("a.wgsl"), "// a").unwrap();

        Vfs::new(root).unwrap()
    }

    #[test]
    fn missing_root_is_an_error() {
        assert!(Vfs::new("/definitely/not/here").is_err());
    }

    #[test]
    fn reads_by_asset_path() {
        let vfs = fixture("vfs-read");

        assert_eq!(vfs.read_text("shaders/a.wgsl").unwrap(), "// a");
        assert_eq!(vfs.read("shaders/a.wgsl").unwrap(), b"// a");
    }

    #[test]
    fn missing_file_error_names_the_path() {
        let vfs = fixture("vfs-missing");
        let err = vfs.read_text("shaders/nope.wgsl").unwrap_err();

        assert!(format!("{err:#}").contains("nope.wgsl"));
    }

    #[test]
    fn asset_path_uses_forward_slashes() {
        let vfs = fixture("vfs-path");
        let os_path = vfs.root().join("shaders").join("a.wgsl");

        assert_eq!(vfs.asset_path(&os_path).as_deref(), Some("shaders/a.wgsl"));
        assert_eq!(vfs.asset_path(Path::new("/elsewhere/x")), None);
    }

    #[test]
    fn find_upward_checks_ancestors() {
        let vfs = fixture("vfs-upward");
        let project = vfs.root().parent().unwrap();
        let deep = project.join("target").join("debug");
        std::fs::create_dir_all(&deep).unwrap();

        assert_eq!(find_upward(&deep).as_deref(), Some(vfs.root()));
    }
}
