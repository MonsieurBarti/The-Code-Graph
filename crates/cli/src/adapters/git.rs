use std::path::{Path, PathBuf};
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

        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string())
    }
}

fn parse_hunk_range(s: &str) -> Result<(usize, usize)> {
    if let Some((start_s, count_s)) = s.split_once(',') {
        let start = start_s
            .parse::<usize>()
            .map_err(|e| CodeGraphError::Git(format!("bad hunk start '{start_s}': {e}")))?;
        let count = count_s
            .parse::<usize>()
            .map_err(|e| CodeGraphError::Git(format!("bad hunk count '{count_s}': {e}")))?;
        Ok((start, count))
    } else {
        let start = s
            .parse::<usize>()
            .map_err(|e| CodeGraphError::Git(format!("bad hunk start '{s}': {e}")))?;
        Ok((start, 1))
    }
}

fn parse_diff_output(output: &str) -> Result<Vec<DiffHunk>> {
    let mut hunks = Vec::new();
    let mut current_file: Option<PathBuf> = None;

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // Extract the b/Y path from "a/X b/Y"
            if let Some(b_part) = rest.split(" b/").last() {
                current_file = Some(PathBuf::from(b_part));
            }
        } else if let Some(to_path) = line.strip_prefix("rename to ") {
            current_file = Some(PathBuf::from(to_path));
        } else if line.starts_with("@@ ") {
            // Parse @@ -old_start[,old_count] +new_start[,new_count] @@
            let inner = line
                .strip_prefix("@@ ")
                .and_then(|s| s.split_once(" @@"))
                .map(|(ranges, _)| ranges);

            let ranges = match inner {
                Some(r) => r,
                None => {
                    return Err(CodeGraphError::Git(format!(
                        "malformed hunk header: {line}"
                    )));
                }
            };

            let parts: Vec<&str> = ranges.split_whitespace().collect();
            if parts.len() != 2 {
                return Err(CodeGraphError::Git(format!(
                    "expected 2 range specs, got {}: {line}",
                    parts.len()
                )));
            }

            let old_range = parts[0]
                .strip_prefix('-')
                .ok_or_else(|| CodeGraphError::Git(format!("missing '-' prefix: {line}")))?;
            let new_range = parts[1]
                .strip_prefix('+')
                .ok_or_else(|| CodeGraphError::Git(format!("missing '+' prefix: {line}")))?;

            let (old_start, old_count) = parse_hunk_range(old_range)?;
            let (new_start, new_count) = parse_hunk_range(new_range)?;

            let file = current_file.clone().ok_or_else(|| {
                CodeGraphError::Git("hunk header before any diff --git line".into())
            })?;

            hunks.push(DiffHunk {
                file,
                old_start,
                old_count,
                new_start,
                new_count,
            });
        }
    }

    Ok(hunks)
}

/// Reject git ref strings that start with `-` to prevent argument injection.
fn validate_git_ref(refspec: &str) -> Result<()> {
    if refspec.starts_with('-') {
        return Err(CodeGraphError::Git(format!(
            "invalid git ref: '{refspec}' (must not start with '-')"
        )));
    }
    Ok(())
}

const SUPPORTED_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "rs", "py", "go"];

fn has_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SUPPORTED_EXTENSIONS.contains(&e))
}

fn parse_git_status(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter(|line| line.len() >= 4) // "XY <path>" minimum
        .filter_map(|line| {
            let status = &line[..2];
            let rest = &line[3..]; // skip "XY "

            // Handle renames: "R  old -> new" or "RM old -> new"
            let path_str = if status.starts_with('R') {
                rest.split(" -> ").last().unwrap_or(rest)
            } else {
                rest
            };

            let path = PathBuf::from(path_str.trim());
            if has_supported_extension(&path) {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}

impl GitProvider for ShellGitProvider {
    fn current_head(&self) -> Result<String> {
        self.run_git(&["rev-parse", "HEAD"])
    }

    fn changed_files(&self, from: &str, to: &str) -> Result<Vec<PathBuf>> {
        validate_git_ref(from)?;
        validate_git_ref(to)?;
        let output = self.run_git(&["diff", "--name-only", from, to])?;
        Ok(output
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect())
    }

    fn diff_hunks(&self, from: &str, to: Option<&str>) -> Result<Vec<DiffHunk>> {
        validate_git_ref(from)?;
        if let Some(r) = to {
            validate_git_ref(r)?;
        }
        let output = match to {
            None => self.run_git(&["diff", "--unified=0", from])?,
            Some(r) => self.run_git(&["diff", "--unified=0", from, r])?,
        };
        parse_diff_output(&output)
    }

    fn modified_files(&self) -> Result<Vec<PathBuf>> {
        let output = self.run_git(&["status", "--porcelain"])?;
        Ok(parse_git_status(&output))
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

    #[test]
    fn parse_single_hunk_add() {
        let input = "\
diff --git a/src/lib.rs b/src/lib.rs
new file mode 100644
--- /dev/null
+++ b/src/lib.rs
@@ -0,0 +1,5 @@ some context";
        let hunks = parse_diff_output(input).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file, PathBuf::from("src/lib.rs"));
        assert_eq!(hunks[0].old_start, 0);
        assert_eq!(hunks[0].old_count, 0);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].new_count, 5);
    }

    #[test]
    fn parse_modify_hunk() {
        let input = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,3 +10,5 @@ fn main()";
        let hunks = parse_diff_output(input).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file, PathBuf::from("src/main.rs"));
        assert_eq!(hunks[0].old_start, 10);
        assert_eq!(hunks[0].old_count, 3);
        assert_eq!(hunks[0].new_start, 10);
        assert_eq!(hunks[0].new_count, 5);
    }

    #[test]
    fn parse_delete_hunk() {
        let input = "\
diff --git a/src/old.rs b/src/old.rs
--- a/src/old.rs
+++ b/src/old.rs
@@ -5,3 +4,0 @@ fn removed()";
        let hunks = parse_diff_output(input).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 5);
        assert_eq!(hunks[0].old_count, 3);
        assert_eq!(hunks[0].new_start, 4);
        assert_eq!(hunks[0].new_count, 0);
    }

    #[test]
    fn parse_multi_file_diff() {
        let input = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,4 @@ fn a()
@@ -20,1 +22,3 @@ fn b()
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -5,3 +5,3 @@ fn c()
@@ -30,0 +30,10 @@ fn d()";
        let hunks = parse_diff_output(input).unwrap();
        assert_eq!(hunks.len(), 4);
        assert_eq!(hunks[0].file, PathBuf::from("src/a.rs"));
        assert_eq!(hunks[1].file, PathBuf::from("src/a.rs"));
        assert_eq!(hunks[2].file, PathBuf::from("src/b.rs"));
        assert_eq!(hunks[3].file, PathBuf::from("src/b.rs"));
        // First file, second hunk
        assert_eq!(hunks[1].old_start, 20);
        assert_eq!(hunks[1].old_count, 1);
        assert_eq!(hunks[1].new_start, 22);
        assert_eq!(hunks[1].new_count, 3);
        // Second file, second hunk
        assert_eq!(hunks[3].new_start, 30);
        assert_eq!(hunks[3].new_count, 10);
    }

    #[test]
    fn parse_rename() {
        let input = "\
diff --git a/old.rs b/new.rs
similarity index 90%
rename from old.rs
rename to new.rs
--- a/old.rs
+++ b/new.rs
@@ -1,2 +1,3 @@ fn renamed()";
        let hunks = parse_diff_output(input).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file, PathBuf::from("new.rs"));
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].old_count, 2);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].new_count, 3);
    }

    #[test]
    fn parse_empty_output() {
        let hunks = parse_diff_output("").unwrap();
        assert!(hunks.is_empty());
    }

    // --- parse_git_status tests ---

    #[test]
    fn parse_git_status_modified_file() {
        let output = " M src/main.rs\n";
        let files = parse_git_status(output);
        assert_eq!(files, vec![PathBuf::from("src/main.rs")]);
    }

    #[test]
    fn parse_git_status_untracked_file() {
        let output = "?? new_file.ts\n";
        let files = parse_git_status(output);
        assert_eq!(files, vec![PathBuf::from("new_file.ts")]);
    }

    #[test]
    fn parse_git_status_deleted_file() {
        let output = " D deleted.rs\n";
        let files = parse_git_status(output);
        assert_eq!(files, vec![PathBuf::from("deleted.rs")]);
    }

    #[test]
    fn parse_git_status_both_modified() {
        let output = "MM both.ts\n";
        let files = parse_git_status(output);
        assert_eq!(files, vec![PathBuf::from("both.ts")]);
    }

    #[test]
    fn parse_git_status_rename_uses_new_name() {
        let output = "R  old.rs -> new.rs\n";
        let files = parse_git_status(output);
        assert_eq!(files, vec![PathBuf::from("new.rs")]);
    }

    #[test]
    fn parse_git_status_multi_line_mixed() {
        let output = " M src/main.rs\n?? new_file.ts\nA  added.py\n D deleted.go\n";
        let files = parse_git_status(output);
        assert_eq!(files.len(), 4);
        assert!(files.contains(&PathBuf::from("src/main.rs")));
        assert!(files.contains(&PathBuf::from("new_file.ts")));
        assert!(files.contains(&PathBuf::from("added.py")));
        assert!(files.contains(&PathBuf::from("deleted.go")));
    }

    #[test]
    fn parse_git_status_empty_output() {
        let files = parse_git_status("");
        assert!(files.is_empty());
    }

    #[test]
    fn parse_git_status_filters_unsupported_extensions() {
        let output = " M readme.md\n M config.json\n M src/main.rs\n";
        let files = parse_git_status(output);
        assert_eq!(files, vec![PathBuf::from("src/main.rs")]);
    }

    #[test]
    fn parse_single_line_hunk() {
        let input = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -5 +5 @@ fn single()";
        let hunks = parse_diff_output(input).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 5);
        assert_eq!(hunks[0].old_count, 1);
        assert_eq!(hunks[0].new_start, 5);
        assert_eq!(hunks[0].new_count, 1);
    }
}
