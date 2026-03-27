use std::path::{Path, PathBuf};
use domain::error::{CodeGraphError, Result};

const BLOCKLIST: &[&str] = &["/", "/home", "/Users", "/tmp", "/var"];

pub fn find_project_root(start: &Path) -> Result<PathBuf> {
    let mut current = start.canonicalize()
        .map_err(|e| CodeGraphError::FileSystem { path: start.into(), source: e })?;

    loop {
        if is_blocklisted(&current) {
            return Err(CodeGraphError::BlocklistedRoot(current));
        }
        if current.join(".git").is_dir() {
            return Ok(current);
        }
        if !current.pop() {
            return Err(CodeGraphError::NoProject);
        }
    }
}

fn is_blocklisted(path: &Path) -> bool {
    let s = path.to_string_lossy();
    BLOCKLIST.contains(&s.as_ref())
        || std::env::var("HOME").ok().is_some_and(|h| s == *h)
}

pub fn ensure_data_dir(project_root: &Path) -> Result<PathBuf> {
    let dir = project_root.join(".code-graph");
    std::fs::create_dir_all(&dir)
        .map_err(|e| CodeGraphError::FileSystem { path: dir.clone(), source: e })?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_root_with_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join(".git")).unwrap();
        let found = find_project_root(root).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn walks_up_to_find_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join(".git")).unwrap();
        let nested = root.join("src").join("deep");
        fs::create_dir_all(&nested).unwrap();
        let found = find_project_root(&nested).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn no_git_dir_returns_no_project_or_blocklisted() {
        let tmp = tempfile::tempdir().unwrap();
        // No .git created - should eventually hit a blocklisted root or no project
        let result = find_project_root(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn blocklisted_root_slash() {
        let result = find_project_root(Path::new("/"));
        assert!(matches!(result, Err(CodeGraphError::BlocklistedRoot(_))));
    }

    #[test]
    fn ensure_data_dir_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let data = ensure_data_dir(tmp.path()).unwrap();
        assert_eq!(data, tmp.path().join(".code-graph"));
        assert!(data.is_dir());
    }

    #[test]
    fn ensure_data_dir_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        ensure_data_dir(tmp.path()).unwrap();
        let data = ensure_data_dir(tmp.path()).unwrap();
        assert!(data.is_dir());
    }
}
