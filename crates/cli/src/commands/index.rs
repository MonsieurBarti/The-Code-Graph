use domain::error::Result;
use domain::use_cases::index::IndexUseCase;
use storage::SqliteStore;

use crate::adapters::fs::RealFileSystem;
use crate::adapters::git::ShellGitProvider;
use crate::adapters::parse::RayonParseProvider;
use crate::config::load_config;
use crate::output::{OutputFormat, print};
use crate::project::{find_project_root, ensure_data_dir};

use super::IndexArgs;

pub fn run_index(args: &IndexArgs, output_format: OutputFormat) -> Result<()> {
    let root = match &args.path {
        Some(p) => p.clone(),
        None => find_project_root(&std::env::current_dir().map_err(|e| {
            domain::error::CodeGraphError::FileSystem {
                path: ".".into(),
                source: e,
            }
        })?)?,
    };

    let data_dir = ensure_data_dir(&root)?;
    let _config = load_config(&root)?;

    let db_path = data_dir.join("graph.db");
    let store = SqliteStore::open(&db_path).map_err(|e| {
        domain::error::CodeGraphError::Storage(format!("{e}"))
    })?;

    let fs = RealFileSystem;
    let git = ShellGitProvider::new(root.clone());
    let parser = RayonParseProvider::new();

    let use_case = IndexUseCase::new(store, parser, fs, git);
    let stats = use_case.full_index(&root)?;

    print(&stats, output_format);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn index_on_fixture_project_creates_db() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Create a minimal git repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();

        // Create a TypeScript fixture
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("main.ts"), "export function hello(): void {}\nexport class Greeter {}").unwrap();
        fs::write(src.join("util.ts"), "export function add(a: number, b: number): number { return a + b; }").unwrap();

        let args = IndexArgs { path: Some(root.to_path_buf()) };
        let result = run_index(&args, OutputFormat::Compact);
        assert!(result.is_ok(), "index failed: {:?}", result.err());

        let db_path = root.join(".code-graph").join("graph.db");
        assert!(db_path.exists(), "graph.db should exist");
    }
}
