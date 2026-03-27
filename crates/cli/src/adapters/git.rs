use std::path::PathBuf;
use std::process::Command;

use domain::error::{CodeGraphError, Result};
use domain::model::DiffHunk;
use domain::ports::GitProvider;

pub struct ShellGitProvider {
    work_dir: PathBuf,
}

impl ShellGitProvider {
    pub fn new(work_dir: PathBuf) -> Self {
        Self { work_dir }
    }

    fn run_git(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.work_dir)
            .output()
            .map_err(|e| CodeGraphError::Git(format!("failed to run git: {e}")))?;

        if !output.status.success() {
            return Err(CodeGraphError::Git(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl GitProvider for ShellGitProvider {
    fn current_head(&self) -> Result<String> {
        self.run_git(&["rev-parse", "HEAD"])
    }

    fn changed_files(&self, from: &str, to: &str) -> Result<Vec<PathBuf>> {
        let output = self.run_git(&["diff", "--name-only", from, to])?;
        Ok(output
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect())
    }

    fn diff_hunks(&self, _from: &str, _to: Option<&str>) -> Result<Vec<DiffHunk>> {
        todo!("diff hunk parsing — implemented in T05")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_head_returns_40_char_hex() {
        // This test runs in the actual code-graph repo
        let provider = ShellGitProvider::new(PathBuf::from("."));
        let head = provider.current_head().unwrap();
        assert_eq!(head.len(), 40, "HEAD should be 40 hex chars, got: {head}");
        assert!(
            head.chars().all(|c| c.is_ascii_hexdigit()),
            "HEAD should be hex: {head}"
        );
    }

    #[test]
    fn changed_files_returns_paths() {
        // Compare HEAD with itself — should return empty
        let provider = ShellGitProvider::new(PathBuf::from("."));
        let files = provider.changed_files("HEAD", "HEAD").unwrap();
        assert!(files.is_empty(), "no changes between HEAD and HEAD");
    }
}
