use std::path::{Path, PathBuf};
use std::time::Instant;
use crate::error::Result;
use crate::model::IndexStats;
use crate::ports::{FileSystem, GitProvider, GraphStore, ParseProvider};

const SUPPORTED_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "rs", "py", "go"];

pub struct IndexUseCase<S, P, F, G> {
    store: S,
    parser: P,
    fs: F,
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

    pub fn incremental_index(&self, root: &Path) -> Result<IndexStats> {
        let modified = self.git.modified_files()?;
        self.run_incremental(root, modified)
    }

    pub fn incremental_files(&self, root: &Path, files: Vec<PathBuf>) -> Result<IndexStats> {
        self.run_incremental(root, files)
    }

    fn run_incremental(&self, root: &Path, changed_paths: Vec<PathBuf>) -> Result<IndexStats> {
        run_incremental_pipeline(&self.store, &self.parser, &self.fs, root, changed_paths)
    }
}

/// Core incremental pipeline: hash-check, 1-hop dependent discovery, re-parse, atomic store update.
///
/// Extracted as a free function so both `IndexUseCase` and `ensure_fresh` can reuse it
/// without ownership issues.
pub fn run_incremental_pipeline<S: GraphStore, P: ParseProvider, F: FileSystem>(
    store: &S,
    parser: &P,
    fs: &F,
    root: &Path,
    changed_paths: Vec<PathBuf>,
) -> Result<IndexStats> {
    let start = Instant::now();
    let mut reparse_set = Vec::new();

    // Phase 1: Hash check — filter to actually-changed files
    for path in &changed_paths {
        let abs_path = root.join(path);
        let current_hash = match fs.file_hash(&abs_path) {
            Ok(h) => h,
            Err(_) => {
                // File deleted — remove from store
                store.remove_file_data(path)?;
                continue;
            }
        };
        let stored = store.get_file(path)?;
        if stored.as_ref().is_some_and(|f| f.hash == current_hash) {
            continue; // unchanged
        }
        reparse_set.push(path.clone());
    }

    // Phase 2: Find 1-hop dependents
    let mut dependent_set = Vec::new();
    let all_symbols = store.all_symbols()?;
    for path in &reparse_set {
        let file_symbols: Vec<_> = all_symbols.iter()
            .filter(|s| s.location.file == *path)
            .collect();
        for sym in file_symbols {
            let incoming = store.get_edges_to(&sym.qualified_name)?;
            for edge in incoming {
                if let Some(source_sym) = store.get_symbol(&edge.source)? {
                    let dep_path = source_sym.location.file.clone();
                    if !reparse_set.contains(&dep_path) && !dependent_set.contains(&dep_path) {
                        dependent_set.push(dep_path);
                    }
                }
            }
        }
    }
    reparse_set.extend(dependent_set);
    reparse_set.sort();
    reparse_set.dedup();

    if reparse_set.is_empty() {
        return Ok(IndexStats {
            files_indexed: 0,
            symbols_extracted: 0,
            edges_created: 0,
            duration: start.elapsed(),
        });
    }

    // Phase 3: Read + parse + store
    let mut files_with_content = Vec::new();
    for path in &reparse_set {
        let abs_path = root.join(path);
        match fs.read_file(&abs_path) {
            Ok(content) => files_with_content.push((path.clone(), content.into_bytes())),
            Err(e) => tracing::warn!("skipping {}: {e}", path.display()),
        }
    }

    let file_data = parser.parse_and_resolve(&files_with_content, root)?;
    let mut stats = IndexStats {
        files_indexed: 0,
        symbols_extracted: 0,
        edges_created: 0,
        duration: start.elapsed(),
    };
    for fd in &file_data {
        store.remove_file_data(&fd.file.path)?;
        store.store_file_data(&fd.file, &fd.symbols, &fd.edges)?;
        stats.files_indexed += 1;
        stats.symbols_extracted += fd.symbols.len();
        stats.edges_created += fd.edges.len();
    }
    stats.duration = start.elapsed();
    Ok(stats)
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
        let git = MockGitProvider::new();
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
        let git = MockGitProvider::new();
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
        let git = MockGitProvider::new();
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
        let git = MockGitProvider::new();
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.full_index(Path::new("/project")).unwrap();
        assert_eq!(stats.files_indexed, 3);
    }

    // --- Incremental pipeline tests ---

    #[test]
    fn incremental_index_skips_unchanged_files() {
        // Store has file with hash "abc123", fs returns same hash → no re-parse
        let mut store = InMemoryGraphStore::new();
        store.insert_file(FileNode {
            path: "src/a.ts".into(),
            language: Language::TypeScript,
            hash: "abc123".into(),
        });
        let parser = MockParseProvider::new(vec![]);
        let fs = MockFileSystem::new(vec![])
            .with_hashes(vec![
                (PathBuf::from("/project/src/a.ts"), "abc123".into()),
            ]);
        let git = MockGitProvider::with_modified(vec![PathBuf::from("src/a.ts")]);
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.incremental_index(Path::new("/project")).unwrap();
        assert_eq!(stats.files_indexed, 0);
    }

    #[test]
    fn incremental_index_reparses_changed_files() {
        // Store has file with hash "old", fs returns "new" → re-parse
        let mut store = InMemoryGraphStore::new();
        store.insert_file(FileNode {
            path: "src/a.ts".into(),
            language: Language::TypeScript,
            hash: "old_hash".into(),
        });
        let fd = make_file_data("src/a.ts", 2, 1);
        let parser = MockParseProvider::new(vec![fd]);
        let fs = MockFileSystem::new(vec![
                (PathBuf::from("/project/src/a.ts"), "content".into()),
            ])
            .with_hashes(vec![
                (PathBuf::from("/project/src/a.ts"), "new_hash".into()),
            ]);
        let git = MockGitProvider::with_modified(vec![PathBuf::from("src/a.ts")]);
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.incremental_index(Path::new("/project")).unwrap();
        assert_eq!(stats.files_indexed, 1);
        assert_eq!(stats.symbols_extracted, 2);
    }

    #[test]
    fn incremental_index_reparses_one_hop_dependents() {
        // File A changed, File B has edge targeting a symbol in A → both re-parsed
        let mut store = InMemoryGraphStore::new();
        store.insert_file(FileNode {
            path: "src/a.ts".into(),
            language: Language::TypeScript,
            hash: "old_hash".into(),
        });
        store.insert_file(FileNode {
            path: "src/b.ts".into(),
            language: Language::TypeScript,
            hash: "b_hash".into(),
        });
        // A has symbol "src/a.ts::func"
        store.insert_symbol(SymbolNode {
            name: "func".into(),
            qualified_name: "src/a.ts::func".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/a.ts".into(),
                line_start: 1, line_end: 5, col_start: 0, col_end: 10,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        // B has symbol "src/b.ts::caller" that calls A's func
        store.insert_symbol(SymbolNode {
            name: "caller".into(),
            qualified_name: "src/b.ts::caller".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/b.ts".into(),
                line_start: 1, line_end: 5, col_start: 0, col_end: 10,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        // Edge: B::caller → A::func (B imports/calls A)
        store.insert_edge(Edge {
            kind: EdgeKind::Calls,
            source: "src/b.ts::caller".into(),
            target: "src/a.ts::func".into(),
            metadata: None,
        });
        // Parser returns data for both files
        let fd_a = make_file_data("src/a.ts", 1, 0);
        let fd_b = make_file_data("src/b.ts", 1, 0);
        let parser = MockParseProvider::new(vec![fd_a, fd_b]);
        let fs = MockFileSystem::new(vec![
                (PathBuf::from("/project/src/a.ts"), "new content".into()),
                (PathBuf::from("/project/src/b.ts"), "b content".into()),
            ])
            .with_hashes(vec![
                (PathBuf::from("/project/src/a.ts"), "new_hash".into()),
                (PathBuf::from("/project/src/b.ts"), "b_hash".into()),
            ]);
        // Only A is reported as modified by git
        let git = MockGitProvider::with_modified(vec![PathBuf::from("src/a.ts")]);
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.incremental_index(Path::new("/project")).unwrap();
        // Both A (changed) and B (dependent) should be re-parsed
        assert_eq!(stats.files_indexed, 2);
    }

    #[test]
    fn incremental_index_no_modified_files_returns_zeros() {
        let store = InMemoryGraphStore::new();
        let parser = MockParseProvider::new(vec![]);
        let fs = MockFileSystem::new(vec![]);
        let git = MockGitProvider::new(); // empty modified list
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.incremental_index(Path::new("/project")).unwrap();
        assert_eq!(stats.files_indexed, 0);
        assert_eq!(stats.symbols_extracted, 0);
        assert_eq!(stats.edges_created, 0);
    }

    #[test]
    fn incremental_files_processes_explicit_list() {
        // Explicit file list bypasses git status
        let mut store = InMemoryGraphStore::new();
        store.insert_file(FileNode {
            path: "src/a.ts".into(),
            language: Language::TypeScript,
            hash: "old_hash".into(),
        });
        let fd = make_file_data("src/a.ts", 1, 1);
        let parser = MockParseProvider::new(vec![fd]);
        let fs = MockFileSystem::new(vec![
                (PathBuf::from("/project/src/a.ts"), "content".into()),
            ])
            .with_hashes(vec![
                (PathBuf::from("/project/src/a.ts"), "new_hash".into()),
            ]);
        let git = MockGitProvider::new(); // no modified — git not consulted
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.incremental_files(
            Path::new("/project"),
            vec![PathBuf::from("src/a.ts")],
        ).unwrap();
        assert_eq!(stats.files_indexed, 1);
    }

    #[test]
    fn incremental_files_skips_unchanged_in_list() {
        // Explicit list but hash matches → skip
        let mut store = InMemoryGraphStore::new();
        store.insert_file(FileNode {
            path: "src/a.ts".into(),
            language: Language::TypeScript,
            hash: "same_hash".into(),
        });
        let parser = MockParseProvider::new(vec![]);
        let fs = MockFileSystem::new(vec![])
            .with_hashes(vec![
                (PathBuf::from("/project/src/a.ts"), "same_hash".into()),
            ]);
        let git = MockGitProvider::new();
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.incremental_files(
            Path::new("/project"),
            vec![PathBuf::from("src/a.ts")],
        ).unwrap();
        assert_eq!(stats.files_indexed, 0);
    }

    #[test]
    fn incremental_index_removes_deleted_files() {
        // File reported as modified but hash fails (deleted) → remove from store
        let mut store = InMemoryGraphStore::new();
        store.insert_file(FileNode {
            path: "src/deleted.ts".into(),
            language: Language::TypeScript,
            hash: "old".into(),
        });
        let parser = MockParseProvider::new(vec![]);
        // No hash for deleted file → file_hash will error
        let fs = MockFileSystem::new(vec![])
            .with_hashes(vec![]);
        let git = MockGitProvider::with_modified(vec![PathBuf::from("src/deleted.ts")]);
        let uc = IndexUseCase::new(store, parser, fs, git);
        let stats = uc.incremental_index(Path::new("/project")).unwrap();
        // Deleted file is removed, not re-parsed
        assert_eq!(stats.files_indexed, 0);
    }
}
