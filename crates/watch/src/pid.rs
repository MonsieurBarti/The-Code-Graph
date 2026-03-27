use std::path::{Path, PathBuf};

use domain::error::{CodeGraphError, Result};

pub fn pid_path(data_dir: &Path) -> PathBuf {
    data_dir.join("daemon.pid")
}

pub fn write_pid(data_dir: &Path, pid: u32) -> Result<()> {
    std::fs::write(pid_path(data_dir), pid.to_string()).map_err(|e| CodeGraphError::FileSystem {
        path: pid_path(data_dir),
        source: e,
    })
}

pub fn read_pid(data_dir: &Path) -> Option<u32> {
    std::fs::read_to_string(pid_path(data_dir))
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn remove_pid(data_dir: &Path) {
    let _ = std::fs::remove_file(pid_path(data_dir));
}

pub fn is_process_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: kill with signal 0 just checks process existence
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Returns Some(pid) if daemon is alive, None otherwise.
/// Cleans up stale PID file if process is dead.
pub fn check_daemon(data_dir: &Path) -> Option<u32> {
    let pid = read_pid(data_dir)?;
    if is_process_running(pid) {
        Some(pid)
    } else {
        remove_pid(data_dir);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_pid() {
        let tmp = tempfile::tempdir().unwrap();
        write_pid(tmp.path(), 12345).unwrap();
        assert_eq!(read_pid(tmp.path()), Some(12345));
    }

    #[test]
    fn read_pid_missing_file_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(read_pid(tmp.path()), None);
    }

    #[test]
    fn read_pid_invalid_content_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(pid_path(tmp.path()), "not_a_number").unwrap();
        assert_eq!(read_pid(tmp.path()), None);
    }

    #[test]
    fn remove_pid_deletes_file() {
        let tmp = tempfile::tempdir().unwrap();
        write_pid(tmp.path(), 12345).unwrap();
        assert!(pid_path(tmp.path()).exists());
        remove_pid(tmp.path());
        assert!(!pid_path(tmp.path()).exists());
    }

    #[test]
    fn remove_pid_noop_if_missing() {
        let tmp = tempfile::tempdir().unwrap();
        remove_pid(tmp.path()); // should not panic
    }

    #[test]
    fn is_process_running_false_for_zero() {
        assert!(!is_process_running(0));
    }

    #[test]
    fn is_process_running_false_for_large_pid() {
        assert!(!is_process_running(999_999_999));
    }

    #[test]
    fn check_daemon_with_no_pid_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(check_daemon(tmp.path()), None);
    }

    #[test]
    fn check_daemon_with_stale_pid_cleans_up() {
        let tmp = tempfile::tempdir().unwrap();
        write_pid(tmp.path(), 999_999_999).unwrap();
        assert_eq!(check_daemon(tmp.path()), None);
        // Stale PID file should be cleaned up
        assert!(!pid_path(tmp.path()).exists());
    }

    #[test]
    fn check_daemon_with_current_process() {
        let tmp = tempfile::tempdir().unwrap();
        let my_pid = std::process::id();
        write_pid(tmp.path(), my_pid).unwrap();
        assert_eq!(check_daemon(tmp.path()), Some(my_pid));
    }
}
