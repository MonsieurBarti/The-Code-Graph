use std::path::{Path, PathBuf};
use crate::error::Result;
use crate::model::*;

/// Primary storage for the code graph — files, symbols, edges.
pub trait GraphStore: Send + Sync {
    fn upsert_file(&self, file: &FileNode) -> Result<()>;
    fn upsert_symbol(&self, symbol: &SymbolNode) -> Result<()>;
    fn upsert_edge(&self, edge: &Edge) -> Result<()>;
    fn get_file(&self, path: &Path) -> Result<Option<FileNode>>;
    fn get_symbol(&self, qualified_name: &str) -> Result<Option<SymbolNode>>;
    fn get_edges_from(&self, source: &str) -> Result<Vec<Edge>>;
    fn get_edges_to(&self, target: &str) -> Result<Vec<Edge>>;
    fn all_files(&self) -> Result<Vec<FileNode>>;
    fn all_symbols(&self) -> Result<Vec<SymbolNode>>;
    fn all_edges(&self) -> Result<Vec<Edge>>;
    fn remove_file(&self, path: &Path) -> Result<()>;
    fn remove_symbols_in_file(&self, path: &Path) -> Result<()>;
    fn stats(&self) -> Result<GraphStats>;
    fn find_by_name(&self, pattern: &str) -> Result<Vec<SymbolNode>>;

    /// Store a file and all its symbols and edges atomically.
    fn store_file_data(
        &self,
        file: &FileNode,
        symbols: &[SymbolNode],
        edges: &[Edge],
    ) -> Result<()>;

    /// Remove all data associated with a file: file row, symbols, and related edges.
    fn remove_file_data(&self, path: &Path) -> Result<()>;
}

/// Full-text search over symbols.
pub trait SearchIndex: Send + Sync {
    fn index_symbol(&self, symbol: &SymbolNode) -> Result<()>;
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;
    fn rebuild(&self) -> Result<()>;
}

/// Git operations (diff, log, etc.).
pub trait GitProvider: Send + Sync {
    fn diff_hunks(&self, from: &str, to: Option<&str>) -> Result<Vec<DiffHunk>>;
    fn changed_files(&self, from: &str, to: &str) -> Result<Vec<PathBuf>>;
    fn current_head(&self) -> Result<String>;
}

/// Filesystem abstraction for reading source files.
pub trait FileSystem: Send + Sync {
    fn read_file(&self, path: &Path) -> Result<String>;
    fn list_files(&self, root: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>>;
    fn file_hash(&self, path: &Path) -> Result<String>;
}

/// Data ready for storage: one file's worth of graph data.
#[derive(Debug, Clone)]
pub struct FileData {
    pub file: FileNode,
    pub symbols: Vec<SymbolNode>,
    pub edges: Vec<Edge>,
}

/// Outbound port: parse and resolve a batch of source files.
pub trait ParseProvider: Send + Sync {
    fn parse_and_resolve(
        &self,
        files: &[(PathBuf, Vec<u8>)],
        project_root: &Path,
    ) -> Result<Vec<FileData>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn graph_store_is_send_sync() {
        assert_send_sync::<Box<dyn GraphStore>>();
    }

    #[test]
    fn search_index_is_send_sync() {
        assert_send_sync::<Box<dyn SearchIndex>>();
    }

    #[test]
    fn git_provider_is_send_sync() {
        assert_send_sync::<Box<dyn GitProvider>>();
    }

    #[test]
    fn file_system_is_send_sync() {
        assert_send_sync::<Box<dyn FileSystem>>();
    }

    #[test]
    fn parse_provider_is_send_sync() {
        assert_send_sync::<Box<dyn ParseProvider>>();
    }

    #[test]
    fn file_data_construction() {
        let fd = FileData {
            file: FileNode {
                path: "src/main.rs".into(),
                language: Language::Rust,
                hash: "abc123".into(),
            },
            symbols: vec![],
            edges: vec![],
        };
        assert_eq!(fd.file.path.to_str().unwrap(), "src/main.rs");
        assert!(fd.symbols.is_empty());
        assert!(fd.edges.is_empty());
    }
}
