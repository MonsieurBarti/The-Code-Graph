use domain::error::Result;
use domain::use_cases::index::IndexUseCase;
use storage::SqliteStore;
use watch::daemon;

use crate::adapters::fs::RealFileSystem;
use crate::adapters::git::ShellGitProvider;
use crate::adapters::parse::RayonParseProvider;
use crate::config::load_config;
use crate::project::{find_project_root, ensure_data_dir};

use super::WatchArgs;

pub fn run_watch(args: &WatchArgs) -> Result<()> {
    let root = match &args.path {
        Some(p) => p.clone(),
        None => find_project_root(&std::env::current_dir().map_err(|e| {
            domain::error::CodeGraphError::FileSystem {
                path: ".".into(),
                source: e,
            }
        })?)?,
    };
    let data_dir = ensure_data_dir(&root)?;
    let config = load_config(&root)?;
    let debounce_ms = config.watch.as_ref().and_then(|w| w.debounce_ms).unwrap_or(100);

    if args.status {
        return show_status(&data_dir);
    }
    if args.stop {
        return daemon::stop_daemon(&data_dir);
    }
    if args.daemon {
        return daemon::start_daemon(&root, &data_dir);
    }

    // Build adapters
    let db_path = data_dir.join("graph.db");
    let store = SqliteStore::open(&db_path).map_err(|e| {
        domain::error::CodeGraphError::Storage(format!("{e}"))
    })?;
    let fs = RealFileSystem;
    let git = ShellGitProvider::new(root.clone());
    let parser = RayonParseProvider::new();
    let use_case = IndexUseCase::new(store, parser, fs, git);

    if args.daemon_internal {
        return daemon::run_daemon(use_case, &root, &data_dir, debounce_ms);
    }

    // Default: foreground mode
    daemon::run_foreground(use_case, &root, &data_dir, debounce_ms)
}

fn show_status(data_dir: &std::path::Path) -> Result<()> {
    match daemon::daemon_status(data_dir) {
        daemon::DaemonStatus::Running(pid) => {
            eprintln!("Daemon running (PID {pid})");
        }
        daemon::DaemonStatus::Stopped => {
            eprintln!("Daemon stopped");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_status_when_not_running() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join(".code-graph")).unwrap();

        let args = WatchArgs {
            daemon: false,
            status: true,
            stop: false,
            daemon_internal: false,
            path: Some(root.to_path_buf()),
        };
        let result = run_watch(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn watch_stop_when_not_running() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join(".code-graph")).unwrap();

        let args = WatchArgs {
            daemon: false,
            status: false,
            stop: true,
            daemon_internal: false,
            path: Some(root.to_path_buf()),
        };
        let result = run_watch(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn ensure_fresh_skips_when_daemon_running() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join(".code-graph");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Write current PID to simulate running daemon
        watch::pid::write_pid(&data_dir, std::process::id()).unwrap();

        let store = domain::test_support::InMemoryGraphStore::new();
        let parser = domain::test_support::MockParseProvider::new(vec![]);
        let fs = domain::test_support::MockFileSystem::new(vec![]);
        let git = domain::test_support::MockGitProvider::with_modified(vec![
            std::path::PathBuf::from("src/a.ts"),
        ]);

        let result = watch::freshness::ensure_fresh(
            &store, &parser, &fs, &git,
            std::path::Path::new("/project"),
            &data_dir,
        );
        assert!(result.is_ok());
    }
}
