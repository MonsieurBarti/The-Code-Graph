use domain::error::{CodeGraphError, Result};
use domain::model::Confidence;
use storage::SqliteStore;
use std::path::PathBuf;
use crate::project::find_project_root;
use crate::adapters::fs::RealFileSystem;
use crate::adapters::git::ShellGitProvider;
use crate::adapters::parse::RayonParseProvider;

pub fn open_graph() -> Result<(SqliteStore, PathBuf)> {
    let cwd = std::env::current_dir().map_err(|e| {
        CodeGraphError::FileSystem { path: ".".into(), source: e }
    })?;
    let root = find_project_root(&cwd)?;
    let db_path = root.join(".code-graph").join("graph.db");
    if !db_path.exists() {
        return Err(CodeGraphError::IndexNotBuilt);
    }
    let store = SqliteStore::open(&db_path)
        .map_err(|e| CodeGraphError::Storage(format!("{e}")))?;

    // Lazy freshness check — skips if daemon is running
    let data_dir = root.join(".code-graph");
    let fs = RealFileSystem;
    let git = ShellGitProvider::new(root.clone());
    let parser = RayonParseProvider::new();
    if let Err(e) = watch::freshness::ensure_fresh(&store, &parser, &fs, &git, &root, &data_dir) {
        tracing::debug!("freshness check skipped: {e}");
    }

    Ok((store, root))
}

pub fn parse_confidence(s: &str) -> Result<Confidence> {
    match s.to_ascii_lowercase().as_str() {
        "high" => Ok(Confidence::High),
        "medium" => Ok(Confidence::Medium),
        "low" => Ok(Confidence::Low),
        "all" => Ok(Confidence::Structural),
        _ => Err(CodeGraphError::Other(format!(
            "invalid confidence level: {s} (expected: high, medium, low, all)"
        ))),
    }
}
