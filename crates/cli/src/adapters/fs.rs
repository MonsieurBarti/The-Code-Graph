use domain::error::{CodeGraphError, Result};
use domain::ports::FileSystem;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn list_files(&self, root: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
        let mut builder = ignore::WalkBuilder::new(root);
        builder.add_custom_ignore_filename(".code-graphignore");

        let files: Vec<PathBuf> = builder
            .build()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| extensions.contains(&ext))
            })
            .map(|entry| {
                entry
                    .path()
                    .strip_prefix(root)
                    .unwrap_or(entry.path())
                    .to_path_buf()
            })
            .collect();

        Ok(files)
    }

    fn read_file(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path).map_err(|e| CodeGraphError::FileSystem {
            path: path.into(),
            source: e,
        })
    }

    fn file_hash(&self, path: &Path) -> Result<String> {
        let content = std::fs::read(path).map_err(|e| CodeGraphError::FileSystem {
            path: path.into(),
            source: e,
        })?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        Ok(format!("{:x}", hasher.finalize()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn list_files_filters_by_extension() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(tmp.path().join("lib.ts"), "export {}").unwrap();
        fs::write(tmp.path().join("readme.md"), "# Hi").unwrap();

        let fs_impl = RealFileSystem;
        let files = fs_impl.list_files(tmp.path(), &["rs", "ts"]).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.to_str().unwrap().ends_with(".rs")));
        assert!(files.iter().any(|f| f.to_str().unwrap().ends_with(".ts")));
    }

    #[test]
    fn list_files_respects_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        // The `ignore` crate only honours .gitignore inside a git repo,
        // so we initialise one in the temp directory.
        fs::create_dir(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join(".gitignore"), "ignored.rs\n").unwrap();
        fs::write(tmp.path().join("kept.rs"), "fn kept() {}").unwrap();
        fs::write(tmp.path().join("ignored.rs"), "fn ignored() {}").unwrap();

        let fs_impl = RealFileSystem;
        let files = fs_impl.list_files(tmp.path(), &["rs"]).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].to_str().unwrap().contains("kept"));
    }

    #[test]
    fn list_files_respects_code_graphignore() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".code-graphignore"), "skip_me.rs\n").unwrap();
        fs::write(tmp.path().join("keep.rs"), "fn keep() {}").unwrap();
        fs::write(tmp.path().join("skip_me.rs"), "fn skip() {}").unwrap();

        let fs_impl = RealFileSystem;
        let files = fs_impl.list_files(tmp.path(), &["rs"]).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].to_str().unwrap().contains("keep"));
    }

    #[test]
    fn read_file_returns_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.txt");
        fs::write(&path, "hello world").unwrap();

        let fs_impl = RealFileSystem;
        let content = fs_impl.read_file(&path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn file_hash_is_deterministic_sha256() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.txt");
        fs::write(&path, "hello").unwrap();

        let fs_impl = RealFileSystem;
        let hash1 = fs_impl.file_hash(&path).unwrap();
        let hash2 = fs_impl.file_hash(&path).unwrap();
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA-256 hex is 64 chars
    }
}
