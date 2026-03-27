use std::path::Path;
use crate::error::Result;
use crate::model::*;
use crate::ports::{GraphStore, SearchIndex};

/// In-memory implementation of GraphStore + SearchIndex for testing.
pub struct InMemoryGraphStore {
    files: Vec<FileNode>,
    symbols: Vec<SymbolNode>,
    edges: Vec<Edge>,
}

impl InMemoryGraphStore {
    pub fn new() -> Self {
        Self { files: vec![], symbols: vec![], edges: vec![] }
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
    fn upsert_file(&self, _file: &FileNode) -> Result<()> { Ok(()) }
    fn upsert_symbol(&self, _symbol: &SymbolNode) -> Result<()> { Ok(()) }
    fn upsert_edge(&self, _edge: &Edge) -> Result<()> { Ok(()) }
    fn get_file(&self, path: &Path) -> Result<Option<FileNode>> {
        Ok(self.files.iter().find(|f| f.path == path).cloned())
    }
    fn get_symbol(&self, qualified_name: &str) -> Result<Option<SymbolNode>> {
        Ok(self.symbols.iter().find(|s| s.qualified_name == qualified_name).cloned())
    }
    fn get_edges_from(&self, source: &str) -> Result<Vec<Edge>> {
        Ok(self.edges.iter().filter(|e| e.source == source).cloned().collect())
    }
    fn get_edges_to(&self, target: &str) -> Result<Vec<Edge>> {
        Ok(self.edges.iter().filter(|e| e.target == target).cloned().collect())
    }
    fn all_files(&self) -> Result<Vec<FileNode>> { Ok(self.files.clone()) }
    fn all_symbols(&self) -> Result<Vec<SymbolNode>> { Ok(self.symbols.clone()) }
    fn all_edges(&self) -> Result<Vec<Edge>> { Ok(self.edges.clone()) }
    fn remove_file(&self, _path: &Path) -> Result<()> { Ok(()) }
    fn remove_symbols_in_file(&self, _path: &Path) -> Result<()> { Ok(()) }
    fn stats(&self) -> Result<GraphStats> {
        Ok(GraphStats { files: self.files.len(), symbols: self.symbols.len(), edges: self.edges.len() })
    }

    fn store_file_data(
        &self,
        _file: &FileNode,
        _symbols: &[SymbolNode],
        _edges: &[Edge],
    ) -> Result<()> {
        Ok(())
    }

    fn remove_file_data(&self, path: &Path) -> Result<()> {
        Ok(())
    }
}

impl SearchIndex for InMemoryGraphStore {
    fn index_symbol(&self, _symbol: &SymbolNode) -> Result<()> { Ok(()) }
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let results: Vec<SearchResult> = self.symbols.iter()
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
    fn rebuild(&self) -> Result<()> { Ok(()) }
}
