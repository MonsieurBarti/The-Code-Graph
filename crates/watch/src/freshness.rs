use std::path::Path;

use domain::error::Result;
use domain::ports::{FileSystem, GitProvider, GraphStore, ParseProvider};
use domain::use_cases::index::run_incremental_pipeline;

use crate::pid;

/// Ensures the graph is fresh before queries.
///
/// If a daemon is running (PID file exists and process alive), the graph
/// is assumed fresh. Otherwise, runs the full incremental pipeline
/// (hash check + 1-hop dependent discovery + re-parse) via git status.
pub fn ensure_fresh<S, P, F, G>(
    store: &S,
    parser: &P,
    fs: &F,
    git: &G,
    root: &Path,
    data_dir: &Path,
) -> Result<()>
where
    S: GraphStore,
    P: ParseProvider,
    F: FileSystem,
    G: GitProvider,
{
    // If daemon is running, graph is already fresh
    if pid::check_daemon(data_dir).is_some() {
        return Ok(());
    }

    // Run lazy staleness check via the full incremental pipeline
    let modified = git.modified_files()?;
    if modified.is_empty() {
        return Ok(());
    }

    let _ = run_incremental_pipeline(store, parser, fs, root, modified)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::test_support::*;
    use std::path::PathBuf;

    #[test]
    fn ensure_fresh_skips_when_daemon_running() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        // Write current process PID to simulate running daemon
        pid::write_pid(data_dir, std::process::id()).unwrap();

        let store = InMemoryGraphStore::new();
        let parser = MockParseProvider::new(vec![]);
        let fs = MockFileSystem::new(vec![]);
        let git = MockGitProvider::with_modified(vec![PathBuf::from("src/a.ts")]);

        // Should return Ok without doing anything — daemon is "running"
        let result = ensure_fresh(&store, &parser, &fs, &git, Path::new("/project"), data_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn ensure_fresh_with_no_daemon_and_no_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let store = InMemoryGraphStore::new();
        let parser = MockParseProvider::new(vec![]);
        let fs = MockFileSystem::new(vec![]);
        let git = MockGitProvider::new(); // no modified files

        let result = ensure_fresh(&store, &parser, &fs, &git, Path::new("/project"), tmp.path());
        assert!(result.is_ok());
    }

    #[test]
    fn ensure_fresh_with_no_daemon_and_changed_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = InMemoryGraphStore::new();
        store.insert_file(domain::model::FileNode {
            path: "src/a.ts".into(),
            language: domain::model::Language::TypeScript,
            hash: "old_hash".into(),
        });
        let parser = MockParseProvider::new(vec![]);
        let fs = MockFileSystem::new(vec![])
            .with_hashes(vec![
                (PathBuf::from("/project/src/a.ts"), "new_hash".into()),
            ]);
        let git = MockGitProvider::with_modified(vec![PathBuf::from("src/a.ts")]);

        // Should attempt incremental update (won't fail even without read content — parser returns empty)
        let result = ensure_fresh(&store, &parser, &fs, &git, Path::new("/project"), tmp.path());
        assert!(result.is_ok());
    }
}
