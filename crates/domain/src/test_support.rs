use crate::error::Result;
use crate::model::*;
use crate::ports::{FileData, GraphStore, ParseProvider, SearchIndex};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// In-memory implementation of GraphStore + SearchIndex for testing.
pub struct InMemoryGraphStore {
    pub files: Vec<FileNode>,
    pub symbols: Vec<SymbolNode>,
    pub edges: Vec<Edge>,
    pub symbols_for_files_calls: AtomicUsize,
    pub edges_streaming_calls: AtomicUsize,
}

impl Clone for InMemoryGraphStore {
    fn clone(&self) -> Self {
        Self {
            files: self.files.clone(),
            symbols: self.symbols.clone(),
            edges: self.edges.clone(),
            symbols_for_files_calls: AtomicUsize::new(
                self.symbols_for_files_calls.load(Ordering::Relaxed),
            ),
            edges_streaming_calls: AtomicUsize::new(
                self.edges_streaming_calls.load(Ordering::Relaxed),
            ),
        }
    }
}

impl Default for InMemoryGraphStore {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            symbols: Vec::new(),
            edges: Vec::new(),
            symbols_for_files_calls: AtomicUsize::new(0),
            edges_streaming_calls: AtomicUsize::new(0),
        }
    }
}

impl InMemoryGraphStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_file(&mut self, file: FileNode) {
        self.files.push(file);
    }

    pub fn insert_symbol(&mut self, symbol: SymbolNode) {
        self.symbols.push(symbol);
    }

    pub fn insert_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }
}

impl GraphStore for InMemoryGraphStore {
    fn upsert_file(&self, _file: &FileNode) -> Result<()> {
        Ok(())
    }
    fn upsert_symbol(&self, _symbol: &SymbolNode) -> Result<()> {
        Ok(())
    }
    fn upsert_edge(&self, _edge: &Edge) -> Result<()> {
        Ok(())
    }
    fn get_file(&self, path: &Path) -> Result<Option<FileNode>> {
        Ok(self.files.iter().find(|f| f.path == path).cloned())
    }
    fn get_symbol(&self, qualified_name: &str) -> Result<Option<SymbolNode>> {
        Ok(self
            .symbols
            .iter()
            .find(|s| s.qualified_name == qualified_name)
            .cloned())
    }
    fn get_edges_from(&self, source: &str) -> Result<Vec<Edge>> {
        Ok(self
            .edges
            .iter()
            .filter(|e| e.source == source)
            .cloned()
            .collect())
    }
    fn get_edges_to(&self, target: &str) -> Result<Vec<Edge>> {
        Ok(self
            .edges
            .iter()
            .filter(|e| e.target == target)
            .cloned()
            .collect())
    }
    fn all_files(&self) -> Result<Vec<FileNode>> {
        Ok(self.files.clone())
    }
    fn all_symbols(&self) -> Result<Vec<SymbolNode>> {
        Ok(self.symbols.clone())
    }
    fn all_edges(&self) -> Result<Vec<Edge>> {
        Ok(self.edges.clone())
    }
    fn remove_file(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
    fn remove_symbols_in_file(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
    fn stats(&self) -> Result<GraphStats> {
        Ok(GraphStats {
            files: self.files.len(),
            symbols: self.symbols.len(),
            edges: self.edges.len(),
            entry_point_count: None,
            avg_criticality: None,
            clone_clusters: None,
            duplication_pct: None,
            most_duplicated: None,
            avg_risk: None,
            p90_risk: None,
        })
    }
    fn find_by_name(&self, pattern: &str) -> Result<Vec<SymbolNode>> {
        let exact: Vec<SymbolNode> = self
            .symbols
            .iter()
            .filter(|s| s.name == pattern)
            .cloned()
            .collect();
        if !exact.is_empty() {
            return Ok(exact);
        }
        Ok(self
            .symbols
            .iter()
            .filter(|s| s.name.starts_with(pattern))
            .cloned()
            .collect())
    }

    fn symbols_for_files(&self, paths: &[&Path]) -> Result<Vec<SymbolNode>> {
        self.symbols_for_files_calls.fetch_add(1, Ordering::Relaxed);
        Ok(self
            .symbols
            .iter()
            .filter(|s| paths.contains(&&*s.location.file))
            .cloned()
            .collect())
    }

    fn edges_streaming(&self, callback: &mut dyn FnMut(Edge) -> Result<()>) -> Result<()> {
        self.edges_streaming_calls.fetch_add(1, Ordering::Relaxed);
        for edge in &self.edges {
            callback(edge.clone())?;
        }
        Ok(())
    }

    fn store_file_data(
        &self,
        _file: &FileNode,
        _symbols: &[SymbolNode],
        _edges: &[Edge],
    ) -> Result<()> {
        Ok(())
    }

    fn remove_file_data(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
}

impl SearchIndex for InMemoryGraphStore {
    fn index_symbol(&self, _symbol: &SymbolNode) -> Result<()> {
        Ok(())
    }
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let results: Vec<SearchResult> = self
            .symbols
            .iter()
            .filter(|s| s.name.contains(query) || s.qualified_name.contains(query))
            .take(limit)
            .map(|s| SearchResult {
                qualified_name: s.qualified_name.clone(),
                name: s.name.clone(),
                kind: s.kind,
                file_path: s.location.file.clone(),
                score: 1.0,
            })
            .collect();
        Ok(results)
    }
    fn rebuild(&self) -> Result<()> {
        Ok(())
    }
}

/// Mock ParseProvider that returns canned FileData for testing.
pub struct MockParseProvider {
    pub results: Vec<FileData>,
}

impl MockParseProvider {
    pub fn new(results: Vec<FileData>) -> Self {
        Self { results }
    }
}

impl ParseProvider for MockParseProvider {
    fn parse_and_resolve(
        &self,
        _files: &[(PathBuf, Vec<u8>)],
        _project_root: &Path,
    ) -> crate::error::Result<Vec<FileData>> {
        Ok(self.results.clone())
    }
}

/// Mock FileSystem for testing.
pub struct MockFileSystem {
    pub files: Vec<(PathBuf, String)>,
    pub hashes: Vec<(PathBuf, String)>,
}

impl MockFileSystem {
    pub fn new(files: Vec<(PathBuf, String)>) -> Self {
        Self {
            files,
            hashes: vec![],
        }
    }

    pub fn with_hashes(mut self, hashes: Vec<(PathBuf, String)>) -> Self {
        self.hashes = hashes;
        self
    }
}

impl crate::ports::FileSystem for MockFileSystem {
    fn list_files(&self, _root: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
        Ok(self
            .files
            .iter()
            .filter(|(p, _)| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| extensions.contains(&e))
            })
            .map(|(p, _)| p.clone())
            .collect())
    }

    fn read_file(&self, path: &Path) -> Result<String> {
        self.files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, content)| content.clone())
            .ok_or_else(|| {
                crate::error::CodeGraphError::Other(format!("file not found: {}", path.display()))
            })
    }

    fn file_hash(&self, path: &Path) -> Result<String> {
        if !self.hashes.is_empty() {
            return self
                .hashes
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, h)| h.clone())
                .ok_or_else(|| {
                    crate::error::CodeGraphError::Other(format!(
                        "file not found: {}",
                        path.display()
                    ))
                });
        }
        Ok("mock_hash".to_string())
    }
}

/// Mock GitProvider for testing.
pub struct MockGitProvider {
    pub modified: Vec<PathBuf>,
}

impl Default for MockGitProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockGitProvider {
    pub fn new() -> Self {
        Self { modified: vec![] }
    }

    pub fn with_modified(files: Vec<PathBuf>) -> Self {
        Self { modified: files }
    }
}

impl crate::ports::GitProvider for MockGitProvider {
    fn current_head(&self) -> Result<String> {
        Ok("abcd1234".to_string())
    }

    fn changed_files(&self, _from: &str, _to: &str) -> Result<Vec<PathBuf>> {
        Ok(vec![])
    }

    fn diff_hunks(&self, _from: &str, _to: Option<&str>) -> Result<Vec<DiffHunk>> {
        Ok(vec![])
    }

    fn modified_files(&self) -> Result<Vec<PathBuf>> {
        Ok(self.modified.clone())
    }
}
