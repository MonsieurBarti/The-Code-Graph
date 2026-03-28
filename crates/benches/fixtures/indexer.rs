use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub path: PathBuf,
    pub symbols: Vec<String>,
    pub imports: Vec<String>,
    pub checksum: u64,
    pub line_count: usize,
}

#[derive(Debug, Default)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub symbols_found: usize,
    pub errors: usize,
    pub elapsed_ms: u64,
}

pub struct Indexer {
    root: PathBuf,
    index: Arc<Mutex<HashMap<PathBuf, IndexEntry>>>,
}

impl Indexer {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), index: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn index_all(&self, extensions: &[&str]) -> IndexStats {
        let start = std::time::Instant::now();
        let paths: Vec<PathBuf> = WalkDir::new(&self.root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| extensions.iter().any(|ext| e.path().extension().and_then(|x| x.to_str()) == Some(ext)))
            .map(|e| e.into_path())
            .collect();

        let results: Vec<Option<IndexEntry>> = paths.par_iter()
            .map(|p| self.index_file(p))
            .collect();

        let mut stats = IndexStats::default();
        let mut guard = self.index.lock().unwrap();
        for (path, result) in paths.into_iter().zip(results) {
            match result {
                Some(entry) => {
                    stats.files_indexed += 1;
                    stats.symbols_found += entry.symbols.len();
                    guard.insert(path, entry);
                }
                None => stats.errors += 1,
            }
        }
        stats.elapsed_ms = start.elapsed().as_millis() as u64;
        stats
    }

    fn index_file(&self, path: &Path) -> Option<IndexEntry> {
        let content = std::fs::read_to_string(path).ok()?;
        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") || trimmed.starts_with("struct ") {
                let name = trimmed.split('(').next().unwrap_or("").split_whitespace().last().unwrap_or("").to_string();
                if !name.is_empty() { symbols.push(name); }
            }
            if trimmed.starts_with("use ") { imports.push(trimmed.trim_end_matches(';').to_string()); }
        }
        let checksum = content.bytes().fold(0u64, |acc, b| acc.wrapping_add(b as u64));
        Some(IndexEntry { path: path.to_path_buf(), symbols, imports, checksum, line_count: content.lines().count() })
    }

    pub fn lookup(&self, symbol: &str) -> Vec<PathBuf> {
        self.index.lock().unwrap().iter()
            .filter(|(_, e)| e.symbols.iter().any(|s| s == symbol))
            .map(|(p, _)| p.clone())
            .collect()
    }

    pub fn invalidate(&self, path: &Path) -> bool {
        self.index.lock().unwrap().remove(path).is_some()
    }
}
