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

/// Scale fixtures by duplicating them with unique names.
pub fn scale_fixtures(base: &[(PathBuf, Vec<u8>)], target_count: usize) -> Vec<(PathBuf, Vec<u8>)> {
    let mut scaled = Vec::with_capacity(target_count);
    for (i, (path, content)) in base.iter().cycle().enumerate().take(target_count) {
        let stem = path.file_stem().unwrap().to_string_lossy();
        let ext = path.extension().unwrap().to_string_lossy();
        let new_path = path.with_file_name(format!("{stem}_{i}.{ext}"));
        scaled.push((new_path, content.clone()));
    }
    scaled
}
