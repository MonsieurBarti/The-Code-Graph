use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use domain::error::{CodeGraphError, Result};
use domain::use_cases::index::IndexUseCase;
use tracing_appender::rolling;

use crate::pid;
use crate::watcher::CodeGraphWatcher;

#[derive(Debug, PartialEq)]
pub enum DaemonStatus {
    Running(u32),
    Stopped,
}

pub fn daemon_status(data_dir: &Path) -> DaemonStatus {
    match pid::check_daemon(data_dir) {
        Some(pid) => DaemonStatus::Running(pid),
        None => DaemonStatus::Stopped,
    }
}

pub fn stop_daemon(data_dir: &Path) -> Result<()> {
    match pid::read_pid(data_dir) {
        Some(pid_val) => {
            // SAFETY: sending SIGTERM to a process
            unsafe {
                libc::kill(pid_val as i32, libc::SIGTERM);
            }
            pid::remove_pid(data_dir);
            eprintln!("Stopped daemon (PID {pid_val})");
            Ok(())
        }
        None => {
            eprintln!("No daemon running");
            Ok(())
        }
    }
}

pub fn start_daemon(root: &Path, data_dir: &Path) -> Result<()> {
    if let Some(pid_val) = pid::check_daemon(data_dir) {
        return Err(CodeGraphError::Other(format!(
            "daemon already running (PID {pid_val})"
        )));
    }

    let exe = std::env::current_exe()
        .map_err(|e| CodeGraphError::Other(format!("failed to get current exe: {e}")))?;

    let child = Command::new(exe)
        .args(["watch", "--daemon-internal"])
        .arg("--path")
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| CodeGraphError::Other(format!("failed to spawn daemon: {e}")))?;

    eprintln!("Daemon started (PID {})", child.id());
    Ok(())
}

pub fn run_daemon<S, P, F, G>(
    use_case: IndexUseCase<S, P, F, G>,
    root: &Path,
    data_dir: &Path,
    debounce_ms: u64,
) -> Result<()>
where
    S: domain::ports::GraphStore,
    P: domain::ports::ParseProvider,
    F: domain::ports::FileSystem,
    G: domain::ports::GitProvider,
{
    let my_pid = std::process::id();
    pid::write_pid(data_dir, my_pid)?;

    // Init log rotation
    let log_dir = data_dir.to_path_buf();
    let file_appender = rolling::daily(&log_dir, "daemon");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    run_event_loop(use_case, root, data_dir, debounce_ms)
}

pub fn run_foreground<S, P, F, G>(
    use_case: IndexUseCase<S, P, F, G>,
    root: &Path,
    data_dir: &Path,
    debounce_ms: u64,
) -> Result<()>
where
    S: domain::ports::GraphStore,
    P: domain::ports::ParseProvider,
    F: domain::ports::FileSystem,
    G: domain::ports::GitProvider,
{
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    eprintln!(
        "Watching {} (debounce: {debounce_ms}ms, Ctrl+C to stop)",
        root.display()
    );
    run_event_loop(use_case, root, data_dir, debounce_ms)
}

fn run_event_loop<S, P, F, G>(
    use_case: IndexUseCase<S, P, F, G>,
    root: &Path,
    data_dir: &Path,
    debounce_ms: u64,
) -> Result<()>
where
    S: domain::ports::GraphStore,
    P: domain::ports::ParseProvider,
    F: domain::ports::FileSystem,
    G: domain::ports::GitProvider,
{
    let shutdown = Arc::new(AtomicBool::new(false));

    // Register signal handlers
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&shutdown))
        .map_err(|e| CodeGraphError::Other(format!("failed to register SIGTERM: {e}")))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))
        .map_err(|e| CodeGraphError::Other(format!("failed to register SIGINT: {e}")))?;

    let (tx, rx) = std::sync::mpsc::channel();
    let watcher = CodeGraphWatcher::new(root.to_path_buf(), debounce_ms);

    let shutdown_for_thread = Arc::clone(&shutdown);
    let watch_thread = std::thread::spawn(move || {
        let _ = watcher.watch(tx);
        shutdown_for_thread.store(true, Ordering::SeqCst);
    });

    // Event loop
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(paths) => {
                tracing::info!("re-indexing {} files", paths.len());
                match use_case.incremental_files(root, paths) {
                    Ok(stats) => {
                        tracing::info!(
                            "indexed {} files, {} symbols, {} edges in {:?}",
                            stats.files_indexed,
                            stats.symbols_extracted,
                            stats.edges_created,
                            stats.duration,
                        );
                    }
                    Err(e) => {
                        tracing::error!("incremental index failed: {e}");
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Cleanup
    pid::remove_pid(data_dir);
    tracing::info!("daemon shutdown complete");
    let _ = watch_thread.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_status_stopped_when_no_pid_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(daemon_status(tmp.path()), DaemonStatus::Stopped);
    }

    #[test]
    fn daemon_status_stopped_when_stale_pid() {
        let tmp = tempfile::tempdir().unwrap();
        pid::write_pid(tmp.path(), 999_999_999).unwrap();
        assert_eq!(daemon_status(tmp.path()), DaemonStatus::Stopped);
    }

    #[test]
    fn daemon_status_running_with_current_process() {
        let tmp = tempfile::tempdir().unwrap();
        let my_pid = std::process::id();
        pid::write_pid(tmp.path(), my_pid).unwrap();
        assert_eq!(daemon_status(tmp.path()), DaemonStatus::Running(my_pid));
    }

    #[test]
    fn stop_daemon_with_no_pid_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(stop_daemon(tmp.path()).is_ok());
    }
}
