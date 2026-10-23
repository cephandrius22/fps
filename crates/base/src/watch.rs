//! Hot-reload watcher. Reports changed files under a directory; the
//! main loop drains it once per frame and forwards paths to subsystems.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

/// The OS file watch could not be started.
#[derive(Debug, thiserror::Error)]
#[error("start file watcher")]
pub struct WatchError(#[from] notify::Error);

/// Recursive file watcher with a non-blocking, per-frame drain.
pub struct Watcher {
    // Kept alive: dropping it stops the OS watch.
    _inner: RecommendedWatcher,
    rx: Receiver<PathBuf>,
}

impl Watcher {
    /// Starts watching `dir` and everything below it.
    pub fn new(dir: &Path) -> Result<Self, WatchError> {
        let (tx, rx) = channel();

        let mut inner = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else {
                return;
            };

            // Editors save by write or by rename-over; both count.
            if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                return;
            }

            for path in event.paths {
                let _ = tx.send(path);
            }
        })?;
        inner.watch(dir, RecursiveMode::Recursive)?;

        Ok(Self { _inner: inner, rx })
    }

    /// Changed files since the last call, deduplicated. Never blocks.
    pub fn drain(&self) -> Vec<PathBuf> {
        let unique: BTreeSet<_> = self.rx.try_iter().collect();
        unique.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    /// OS watchers deliver asynchronously (FSEvents batches up to ~1 s).
    const DEADLINE: Duration = Duration::from_secs(10);
    const POLL: Duration = Duration::from_millis(50);

    #[test]
    fn reports_written_file() {
        let dir = crate::test_dir("watch");
        let watcher = Watcher::new(&dir).unwrap();
        let file = dir.join("shader.wgsl");

        let start = Instant::now();
        let mut seen = false;

        // Rewrite until seen: an event can be lost if the OS watch is
        // still starting when the first write lands.
        while !seen && start.elapsed() < DEADLINE {
            std::fs::write(&file, "// edit").unwrap();
            std::thread::sleep(POLL);

            seen = watcher
                .drain()
                .iter()
                .any(|path| path.file_name() == file.file_name());
        }

        assert!(seen, "no event for {}", file.display());
    }

    #[test]
    fn missing_dir_is_an_error() {
        assert!(Watcher::new(Path::new("/definitely/not/here")).is_err());
    }

    #[test]
    fn drain_is_empty_without_changes() {
        let dir = crate::test_dir("watch-quiet");
        let watcher = Watcher::new(&dir).unwrap();

        assert!(watcher.drain().is_empty());
    }
}
