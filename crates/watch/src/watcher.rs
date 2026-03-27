use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify_debouncer_full::{new_debouncer, DebounceEventResult};

use domain::error::{CodeGraphError, Result};

pub struct CodeGraphWatcher {
    root: PathBuf,
    debounce_ms: u64,
}

impl CodeGraphWatcher {
    pub fn new(root: PathBuf, debounce_ms: u64) -> Self {
        Self { root, debounce_ms }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn debounce_ms(&self) -> u64 {
        self.debounce_ms
    }

    pub fn watch(&self, tx: mpsc::Sender<Vec<PathBuf>>) -> Result<()> {
        let root = self.root.clone();
        let (notify_tx, notify_rx) = mpsc::channel();

        let mut debouncer = new_debouncer(
            Duration::from_millis(self.debounce_ms),
            None,
            move |result: DebounceEventResult| {
                if let Ok(events) = result {
                    let _ = notify_tx.send(events);
                }
            },
        )
        .map_err(|e| CodeGraphError::Other(format!("failed to create watcher: {e}")))?;

        debouncer
            .watch(&self.root, notify::RecursiveMode::Recursive)
            .map_err(|e| CodeGraphError::Other(format!("failed to watch: {e}")))?;

        // Process events
        while let Ok(events) = notify_rx.recv() {
            let mut paths = Vec::new();
            for event in events {
                for path in &event.paths {
                    if should_ignore(path) || !has_supported_extension(path) {
                        continue;
                    }
                    if let Ok(rel) = path.strip_prefix(&root) {
                        let rel_path = rel.to_path_buf();
                        if !paths.contains(&rel_path) {
                            paths.push(rel_path);
                        }
                    }
                }
            }
            if !paths.is_empty() && tx.send(paths).is_err() {
                break; // receiver dropped
            }
        }

        Ok(())
    }
}

pub fn should_ignore(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some(".git" | "target" | "node_modules" | ".code-graph")
        )
    })
}

pub fn has_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["ts", "tsx", "js", "jsx", "rs", "py", "go"].contains(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_ignore_git_dir() {
        assert!(should_ignore(Path::new("/project/.git/config")));
        assert!(should_ignore(Path::new("/project/.git/HEAD")));
    }

    #[test]
    fn should_ignore_target_dir() {
        assert!(should_ignore(Path::new("/project/target/debug/main")));
    }

    #[test]
    fn should_ignore_node_modules() {
        assert!(should_ignore(Path::new("/project/node_modules/foo/index.js")));
    }

    #[test]
    fn should_ignore_code_graph_dir() {
        assert!(should_ignore(Path::new("/project/.code-graph/graph.db")));
    }

    #[test]
    fn should_not_ignore_normal_source() {
        assert!(!should_ignore(Path::new("/project/src/main.rs")));
        assert!(!should_ignore(Path::new("/project/src/app.ts")));
    }

    #[test]
    fn has_supported_extension_passes_source_files() {
        assert!(has_supported_extension(Path::new("src/main.rs")));
        assert!(has_supported_extension(Path::new("src/app.ts")));
        assert!(has_supported_extension(Path::new("src/app.tsx")));
        assert!(has_supported_extension(Path::new("src/app.js")));
        assert!(has_supported_extension(Path::new("src/app.jsx")));
        assert!(has_supported_extension(Path::new("src/main.py")));
        assert!(has_supported_extension(Path::new("src/main.go")));
    }

    #[test]
    fn has_supported_extension_rejects_non_source() {
        assert!(!has_supported_extension(Path::new("readme.md")));
        assert!(!has_supported_extension(Path::new("config.json")));
        assert!(!has_supported_extension(Path::new("Cargo.toml")));
        assert!(!has_supported_extension(Path::new("data.csv")));
    }

    #[test]
    fn has_supported_extension_rejects_no_extension() {
        assert!(!has_supported_extension(Path::new("Makefile")));
    }
}
