//! Hot-reload watcher. Reports changed files under a directory; the
//! main loop drains it once per frame and forwards paths to subsystems.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use anyhow::Result;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

pub struct Watcher {
    // Kept alive: dropping it stops the OS watch.
    _inner: RecommendedWatcher,
    rx: Receiver<PathBuf>,
}

impl Watcher {
    pub fn new(dir: &Path) -> Result<Self> {
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

    /// Changed files since the last call, deduplicated.
    pub fn drain(&self) -> Vec<PathBuf> {
        let unique: BTreeSet<_> = self.rx.try_iter().collect();
        unique.into_iter().collect()
    }
}
