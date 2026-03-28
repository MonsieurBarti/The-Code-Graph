use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CodeGraphError {
    #[error("parse error in {file}: {message}")]
    Parse { file: PathBuf, message: String },
    #[error("resolution error: {0}")]
    Resolution(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("git error: {0}")]
    Git(String),
    #[error("file system error: {path}: {source}")]
    FileSystem {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("no project found (no .git directory)")]
    NoProject,
    #[error("refused to index blocklisted root: {0}")]
    BlocklistedRoot(PathBuf),
    #[error("index not built — run `code-graph index` first")]
    IndexNotBuilt,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CodeGraphError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_error_display_contains_file_and_message() {
        let err = CodeGraphError::Parse {
            file: PathBuf::from("src/main.rs"),
            message: "unexpected token".into(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("src/main.rs"), "missing file in: {msg}");
        assert!(
            msg.contains("unexpected token"),
            "missing message in: {msg}"
        );
    }

    #[test]
    fn filesystem_error_display_contains_path() {
        let err = CodeGraphError::FileSystem {
            path: PathBuf::from("/some/path"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("/some/path"), "missing path in: {msg}");
    }

    #[test]
    fn all_variants_display_nonempty() {
        let errors = vec![
            CodeGraphError::Parse {
                file: "f".into(),
                message: "m".into(),
            },
            CodeGraphError::Resolution("r".into()),
            CodeGraphError::Storage("s".into()),
            CodeGraphError::Git("g".into()),
            CodeGraphError::FileSystem {
                path: "p".into(),
                source: std::io::Error::new(std::io::ErrorKind::Other, "e"),
            },
            CodeGraphError::NoProject,
            CodeGraphError::BlocklistedRoot("/".into()),
            CodeGraphError::IndexNotBuilt,
            CodeGraphError::Other("o".into()),
        ];
        for err in &errors {
            assert!(!format!("{err}").is_empty(), "empty display for {err:?}");
        }
    }
}
