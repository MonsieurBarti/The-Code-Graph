use domain::error::{CodeGraphError, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Resolve the path to the settings JSON file.
///
/// - `global == true`  → `$HOME/.claude/settings.json`
/// - `global == false` → `<project_root>/.claude/settings.json`
///
/// Returns an error when `global` is false and `project_root` is `None`.
pub(super) fn resolve_settings_path(project_root: Option<&Path>, global: bool) -> Result<PathBuf> {
    if global {
        let home =
            std::env::var("HOME").map_err(|_| CodeGraphError::Other("$HOME is not set".into()))?;
        Ok(PathBuf::from(home).join(".claude").join("settings.json"))
    } else {
        match project_root {
            Some(root) => Ok(root.join(".claude").join("settings.json")),
            None => Err(CodeGraphError::Other(
                "project root is required for non-global settings".into(),
            )),
        }
    }
}

/// Ensure `.code-graph/` is listed in `<project_root>/.gitignore`.
///
/// Returns `true` if the entry was added, `false` if it was already present.
pub(super) fn ensure_gitignore_entry(project_root: &Path) -> Result<bool> {
    let gitignore = project_root.join(".gitignore");

    let content = if gitignore.exists() {
        fs::read_to_string(&gitignore).map_err(|e| CodeGraphError::FileSystem {
            path: gitignore.clone(),
            source: e,
        })?
    } else {
        String::new()
    };

    // Check for an exact line match
    let already_present = content.lines().any(|line| line.trim() == ".code-graph/");

    if already_present {
        return Ok(false);
    }

    // Ensure there is a trailing newline before appending
    let mut new_content = content.clone();
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str("# Code Graph data\n.code-graph/\n");

    fs::write(&gitignore, new_content).map_err(|e| CodeGraphError::FileSystem {
        path: gitignore,
        source: e,
    })?;

    Ok(true)
}

/// Remove `.code-graph/` (and the `# Code Graph data` comment) from
/// `<project_root>/.gitignore`.
///
/// Returns `true` if any lines were removed, `false` if no changes were made
/// or the file does not exist.
pub(super) fn remove_gitignore_entry(project_root: &Path) -> Result<bool> {
    let gitignore = project_root.join(".gitignore");

    if !gitignore.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&gitignore).map_err(|e| CodeGraphError::FileSystem {
        path: gitignore.clone(),
        source: e,
    })?;

    let original_line_count = content.lines().count();

    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed != ".code-graph/" && trimmed != "# Code Graph data"
        })
        .collect();

    let removed = filtered.len() < original_line_count;

    if removed {
        let mut new_content = filtered.join("\n");
        // Preserve a trailing newline if the original had one
        if content.ends_with('\n') {
            new_content.push('\n');
        }
        fs::write(&gitignore, new_content).map_err(|e| CodeGraphError::FileSystem {
            path: gitignore,
            source: e,
        })?;
    }

    Ok(removed)
}

/// Return the full path to `binary` if it can be found on `$PATH`.
pub(super) fn find_on_path(binary: &str) -> Option<PathBuf> {
    which::which(binary).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ── resolve_settings_path ────────────────────────────────────────────────

    #[test]
    fn resolve_settings_path_local() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let path = resolve_settings_path(Some(root), false).unwrap();
        assert_eq!(path, root.join(".claude").join("settings.json"));
    }

    #[test]
    fn resolve_settings_path_global() {
        let path = resolve_settings_path(None, true).unwrap();
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            path,
            PathBuf::from(home).join(".claude").join("settings.json")
        );
    }

    #[test]
    fn resolve_settings_path_local_without_root_errors() {
        let result = resolve_settings_path(None, false);
        assert!(result.is_err(), "expected error when project_root is None");
    }

    // ── ensure_gitignore_entry ───────────────────────────────────────────────

    #[test]
    fn ensure_gitignore_creates_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let result = ensure_gitignore_entry(root).unwrap();
        assert!(result, "expected true when entry was added");
        let content = fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(content.contains(".code-graph/"));
    }

    #[test]
    fn ensure_gitignore_appends() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let gitignore = root.join(".gitignore");
        fs::write(&gitignore, "target/\n").unwrap();

        let result = ensure_gitignore_entry(root).unwrap();
        assert!(result, "expected true when entry was appended");
        let content = fs::read_to_string(&gitignore).unwrap();
        assert!(content.contains("target/"));
        assert!(content.contains(".code-graph/"));
    }

    #[test]
    fn ensure_gitignore_idempotent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let gitignore = root.join(".gitignore");
        fs::write(&gitignore, "# Code Graph data\n.code-graph/\n").unwrap();

        let result = ensure_gitignore_entry(root).unwrap();
        assert!(!result, "expected false when entry already present");

        let content = fs::read_to_string(&gitignore).unwrap();
        // Verify no duplicate was added
        assert_eq!(content.matches(".code-graph/").count(), 1);
    }

    #[test]
    fn ensure_gitignore_handles_no_trailing_newline() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let gitignore = root.join(".gitignore");
        // No trailing newline
        fs::write(&gitignore, "target/").unwrap();

        let result = ensure_gitignore_entry(root).unwrap();
        assert!(result, "expected true when entry was appended");
        let content = fs::read_to_string(&gitignore).unwrap();
        // Verify the original line and the new entry are on separate lines
        assert!(content.contains("target/\n"));
        assert!(content.contains(".code-graph/"));
    }

    // ── remove_gitignore_entry ───────────────────────────────────────────────

    #[test]
    fn remove_gitignore_entry_removes_line_and_comment() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let gitignore = root.join(".gitignore");
        fs::write(&gitignore, "target/\n# Code Graph data\n.code-graph/\n").unwrap();

        let result = remove_gitignore_entry(root).unwrap();
        assert!(result, "expected true when lines were removed");

        let content = fs::read_to_string(&gitignore).unwrap();
        assert!(!content.contains(".code-graph/"));
        assert!(!content.contains("# Code Graph data"));
        assert!(content.contains("target/"));
    }

    #[test]
    fn remove_gitignore_entry_noop_when_absent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let gitignore = root.join(".gitignore");
        fs::write(&gitignore, "target/\n").unwrap();

        let result = remove_gitignore_entry(root).unwrap();
        assert!(!result, "expected false when no entry found");

        let content = fs::read_to_string(&gitignore).unwrap();
        assert_eq!(content, "target/\n");
    }

    #[test]
    fn remove_gitignore_noop_no_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // No .gitignore at all
        let result = remove_gitignore_entry(root).unwrap();
        assert!(!result, "expected false when .gitignore does not exist");
    }

    // ── find_on_path ─────────────────────────────────────────────────────────

    #[test]
    fn find_on_path_returns_some_for_existing_binary() {
        let result = find_on_path("ls");
        assert!(result.is_some(), "expected Some for 'ls'");
    }

    #[test]
    fn find_on_path_returns_none_for_nonexistent() {
        let result = find_on_path("nonexistent_binary_xyz_123");
        assert!(result.is_none(), "expected None for nonexistent binary");
    }
}
