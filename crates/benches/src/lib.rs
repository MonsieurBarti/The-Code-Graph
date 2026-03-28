//! Shared benchmark helpers for code-graph benchmarks.

use std::path::{Path, PathBuf};

/// Load all fixture files of a given extension from the fixtures directory.
pub fn load_fixtures(ext: &str) -> Vec<(PathBuf, Vec<u8>)> {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&fixtures_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                if let Ok(content) = std::fs::read(&path) {
                    files.push((path, content));
                }
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}
