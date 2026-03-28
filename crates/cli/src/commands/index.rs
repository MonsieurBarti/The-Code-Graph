use domain::error::Result;
use domain::use_cases::index::IndexUseCase;
use storage::SqliteStore;

use crate::adapters::fs::RealFileSystem;
use crate::adapters::git::ShellGitProvider;
use crate::adapters::parse::RayonParseProvider;
use crate::config::load_config;
use crate::output::{print, OutputFormat};
use crate::project::{ensure_data_dir, find_project_root};

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
    let config = load_config(&root)?;

    let db_path = data_dir.join("graph.db");
    let store = SqliteStore::open(&db_path)
        .map_err(|e| domain::error::CodeGraphError::Storage(format!("{e}")))?;

    let fs = RealFileSystem;
    let git = ShellGitProvider::new(root.clone());
    let parser = RayonParseProvider::new();

    let use_case = IndexUseCase::new(store.clone(), parser, fs, git);
    let stats = if let Some(files) = &args.files {
        use_case.incremental_files(&root, files.clone())?
    } else if args.incremental {
        use_case.incremental_index(&root)?
    } else {
        use_case.full_index(&root)?
    };

    print(&stats, output_format);

    if args.embed {
        let ep = embeddings::OnnxEmbeddingProvider::from_model_name(&args.embed_model, 384)
            .map_err(|e| domain::error::CodeGraphError::Other(e.to_string()))?;

        let embed_config = domain::model::EmbeddingConfig {
            model: args.embed_model.clone(),
            batch_size: config
                .embeddings
                .as_ref()
                .and_then(|e| e.batch_size)
                .unwrap_or(64),
        };

        let embed_uc =
            domain::use_cases::embed::EmbedUseCase::new(store.clone(), ep, store.clone());
        let embed_stats = embed_uc.embed_all(&embed_config)?;
        let removed = embed_uc.cleanup_orphans()?;
        let embed_stats = domain::model::EmbedStats {
            removed,
            ..embed_stats
        };
        print(&embed_stats, output_format);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_git_repo(root: &std::path::Path) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(root)
            .output()
            .unwrap();
    }

    #[test]
    fn index_on_fixture_project_creates_db() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        setup_git_repo(root);

        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("main.ts"),
            "export function hello(): void {}\nexport class Greeter {}",
        )
        .unwrap();
        fs::write(
            src.join("util.ts"),
            "export function add(a: number, b: number): number { return a + b; }",
        )
        .unwrap();

        let args = IndexArgs {
            path: Some(root.to_path_buf()),
            incremental: false,
            files: None,
            embed: false,
            embed_model: "all-MiniLM-L6-v2".into(),
        };
        let result = run_index(&args, OutputFormat::Compact);
        assert!(result.is_ok(), "index failed: {:?}", result.err());

        let db_path = root.join(".code-graph").join("graph.db");
        assert!(db_path.exists(), "graph.db should exist");
    }

    #[test]
    fn index_incremental_updates_changed_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        setup_git_repo(root);

        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("main.ts"), "export function hello(): void {}").unwrap();

        // Full index first
        let args = IndexArgs {
            path: Some(root.to_path_buf()),
            incremental: false,
            files: None,
            embed: false,
            embed_model: "all-MiniLM-L6-v2".into(),
        };
        run_index(&args, OutputFormat::Compact).unwrap();

        // Commit the file so git status shows it as clean
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(root)
            .output()
            .unwrap();

        // Modify file
        fs::write(
            src.join("main.ts"),
            "export function hello(): void {}\nexport function world(): void {}",
        )
        .unwrap();

        // Incremental index
        let args = IndexArgs {
            path: Some(root.to_path_buf()),
            incremental: true,
            files: None,
            embed: false,
            embed_model: "all-MiniLM-L6-v2".into(),
        };
        let result = run_index(&args, OutputFormat::Compact);
        assert!(
            result.is_ok(),
            "incremental index failed: {:?}",
            result.err()
        );
    }

    #[test]
    fn index_incremental_with_no_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        setup_git_repo(root);

        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("main.ts"), "export function hello(): void {}").unwrap();

        // Full index first
        let args = IndexArgs {
            path: Some(root.to_path_buf()),
            incremental: false,
            files: None,
            embed: false,
            embed_model: "all-MiniLM-L6-v2".into(),
        };
        run_index(&args, OutputFormat::Compact).unwrap();

        // Commit everything
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(root)
            .output()
            .unwrap();

        // Incremental with no changes
        let args = IndexArgs {
            path: Some(root.to_path_buf()),
            incremental: true,
            files: None,
            embed: false,
            embed_model: "all-MiniLM-L6-v2".into(),
        };
        let result = run_index(&args, OutputFormat::Compact);
        assert!(
            result.is_ok(),
            "incremental no-op failed: {:?}",
            result.err()
        );
    }

    #[test]
    fn index_files_updates_specific_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        setup_git_repo(root);

        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("main.ts"), "export function hello(): void {}").unwrap();

        // Full index first
        let args = IndexArgs {
            path: Some(root.to_path_buf()),
            incremental: false,
            files: None,
            embed: false,
            embed_model: "all-MiniLM-L6-v2".into(),
        };
        run_index(&args, OutputFormat::Compact).unwrap();

        // Modify file
        fs::write(
            src.join("main.ts"),
            "export function hello(): void {}\nexport function bar(): void {}",
        )
        .unwrap();

        // Index specific files
        let args = IndexArgs {
            path: Some(root.to_path_buf()),
            incremental: false,
            files: Some(vec![std::path::PathBuf::from("src/main.ts")]),
            embed: false,
            embed_model: "all-MiniLM-L6-v2".into(),
        };
        let result = run_index(&args, OutputFormat::Compact);
        assert!(result.is_ok(), "index --files failed: {:?}", result.err());
    }
}
