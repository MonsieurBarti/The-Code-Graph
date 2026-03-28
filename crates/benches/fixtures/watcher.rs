use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug, Clone)]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    pub kind: ChangeKind,
}

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<FileChange>,
    debounce_ms: u64,
}

impl FileWatcher {
    pub fn new(dirs: &[&Path], debounce_ms: u64) -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let tx_clone = tx.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                let kind = match event.kind {
                    EventKind::Create(_) => ChangeKind::Created,
                    EventKind::Modify(_) => ChangeKind::Modified,
                    EventKind::Remove(_) => ChangeKind::Deleted,
                    _ => return,
                };
                for path in event.paths {
                    let _ = tx_clone.send(FileChange { path, kind: kind.clone() });
                }
            }
        })?;
        for dir in dirs {
            watcher.watch(dir, RecursiveMode::Recursive)?;
        }
        Ok(Self { _watcher: watcher, rx, debounce_ms })
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Option<FileChange> {
        self.rx.recv_timeout(timeout).ok()
    }

    pub fn drain_batch(&self) -> Vec<FileChange> {
        let mut changes = Vec::new();
        while let Ok(change) = self.rx.try_recv() {
            changes.push(change);
        }
        changes
    }

    pub fn debounced_drain(&self) -> Vec<FileChange> {
        std::thread::sleep(Duration::from_millis(self.debounce_ms));
        self.drain_batch()
    }
}
