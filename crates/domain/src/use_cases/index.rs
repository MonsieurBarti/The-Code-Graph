use std::path::Path;
use std::time::Instant;
use crate::error::Result;
use crate::model::IndexStats;
use crate::ports::{FileSystem, GitProvider, GraphStore, ParseProvider};

const SUPPORTED_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "rs", "py", "go"];

pub struct IndexUseCase<S, P, F, G> {
    store: S,
    parser: P,
    fs: F,
    #[allow(dead_code)] // used by incremental_index in S07
    git: G,
}

impl<S: GraphStore, P: ParseProvider, F: FileSystem, G: GitProvider> IndexUseCase<S, P, F, G> {
    pub fn new(store: S, parser: P, fs: F, git: G) -> Self {
        Self { store, parser, fs, git }
    }

    pub fn full_index(&self, root: &Path) -> Result<IndexStats> {
        let start = Instant::now();

        let file_paths = self.fs.list_files(root, SUPPORTED_EXTENSIONS)?;

        let mut files_with_content = Vec::new();
        for path in &file_paths {
            let abs_path = root.join(path);
            match self.fs.read_file(&abs_path) {
                Ok(content) => {
                    files_with_content.push((path.clone(), content.into_bytes()));
                }
                Err(e) => {
                    tracing::warn!("skipping {}: {e}", path.display());
                }
            }
        }

        let file_data = self.parser.parse_and_resolve(&files_with_content, root)?;

        let mut files_indexed = 0;
        let mut symbols_extracted = 0;
        let mut edges_created = 0;

        for fd in &file_data {
            self.store.store_file_data(&fd.file, &fd.symbols, &fd.edges)?;
            files_indexed += 1;
            symbols_extracted += fd.symbols.len();
            edges_created += fd.edges.len();
        }

        Ok(IndexStats {
            files_indexed,
            symbols_extracted,
            edges_created,
            duration: start.elapsed(),
        })
    }

    pub fn incremental_index(&self, _root: &Path) -> Result<IndexStats> {
        todo!("Implemented in incremental updates slice")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::ports::FileData;
    use crate::test_support::*;
    use std::path::PathBuf;

    fn make_file_data(path: &str, num_symbols: usize, num_edges: usize) -> FileData {
        let symbols: Vec<SymbolNode> = (0..num_symbols).map(|i| SymbolNode {
            name: format!("sym{i}"),
            qualified_name: format!("{path}::sym{i}"),
            kind: SymbolKind::Function,
            location: Location {
                file: path.into(),
                line_start: i + 1,
                line_end: i + 2,
                col_start: 0,
                col_end: 10,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }).collect();

        let edges: Vec<Edge> = (0..num_edges).map(|i| Edge {
            kind: EdgeKind::Contains,
            source: path.to_string(),
            target: format!("{path}::sym{i}"),
            metadata: None,
        }).collect();

        FileData {
            file: FileNode {
                path: path.into(),
                language: Language::TypeScript,
                hash: "abc123".into(),
            },
            symbols,
            edges,
        }
    }

    #[test]
    fn full_index_with_two_files_returns_correct_stats() {
        let fd1 = make_file_data("src/a.ts", 3, 3);
        let fd2 = make_file_data("src/b.ts", 2, 1);
        let store = InMemoryGraphStore::new();
        let parser = MockParseProvider::new(vec![fd1, fd2]);
        let fs = MockFileSystem::new(vec![
            (PathBuf::from("src/a.ts"), "content a".into()),
            (PathBuf::from("src/b.ts"), "content b".into()),
        ]);
        let git = MockGitProvider;
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.full_index(Path::new("/project")).unwrap();
        assert_eq!(stats.files_indexed, 2);
        assert_eq!(stats.symbols_extracted, 5);
        assert_eq!(stats.edges_created, 4);
    }

    #[test]
    fn full_index_empty_file_list_returns_zeros() {
        let store = InMemoryGraphStore::new();
        let parser = MockParseProvider::new(vec![]);
        let fs = MockFileSystem::new(vec![]);
        let git = MockGitProvider;
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.full_index(Path::new("/project")).unwrap();
        assert_eq!(stats.files_indexed, 0);
        assert_eq!(stats.symbols_extracted, 0);
        assert_eq!(stats.edges_created, 0);
    }

    #[test]
    fn full_index_duration_is_non_zero() {
        let store = InMemoryGraphStore::new();
        let parser = MockParseProvider::new(vec![make_file_data("src/a.ts", 1, 1)]);
        let fs = MockFileSystem::new(vec![
            (PathBuf::from("src/a.ts"), "content".into()),
        ]);
        let git = MockGitProvider;
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.full_index(Path::new("/project")).unwrap();
        assert!(stats.duration.as_nanos() > 0);
    }

    #[test]
    fn full_index_with_three_files_reads_all() {
        let fds = vec![
            make_file_data("a.rs", 1, 1),
            make_file_data("b.rs", 1, 1),
            make_file_data("c.rs", 1, 1),
        ];
        let store = InMemoryGraphStore::new();
        let parser = MockParseProvider::new(fds);
        let fs = MockFileSystem::new(vec![
            (PathBuf::from("a.rs"), "fn a(){}".into()),
            (PathBuf::from("b.rs"), "fn b(){}".into()),
            (PathBuf::from("c.rs"), "fn c(){}".into()),
        ]);
        let git = MockGitProvider;
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.full_index(Path::new("/project")).unwrap();
        assert_eq!(stats.files_indexed, 3);
    }
}
