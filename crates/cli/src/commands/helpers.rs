use domain::error::{CodeGraphError, Result};
use domain::model::Confidence;
use storage::SqliteStore;
use std::path::PathBuf;
use crate::project::{find_project_root, ensure_data_dir};

pub fn open_graph() -> Result<(SqliteStore, PathBuf)> {
    let cwd = std::env::current_dir().map_err(|e| {
        CodeGraphError::FileSystem { path: ".".into(), source: e }
    })?;
    let root = find_project_root(&cwd)?;
    let data_dir = ensure_data_dir(&root)?;
    let db_path = data_dir.join("graph.db");
    if !db_path.exists() {
        return Err(CodeGraphError::IndexNotBuilt);
    }
    let store = SqliteStore::open(&db_path)
        .map_err(|e| CodeGraphError::Storage(format!("{e}")))?;
    Ok((store, root))
}

pub fn parse_confidence(s: &str) -> Result<Confidence> {
    match s {
        "high" => Ok(Confidence::High),
        "medium" => Ok(Confidence::Medium),
        "low" => Ok(Confidence::Low),
        "all" => Ok(Confidence::Structural),
        _ => Err(CodeGraphError::Other(format!("invalid confidence level: {s}"))),
    }
}
