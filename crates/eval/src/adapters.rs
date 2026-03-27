use std::collections::HashMap;
use std::path::{Path, PathBuf};

use domain::error::{CodeGraphError, Result};
use domain::model::{Edge, FileNode};
use domain::ports::FileData;
use parser::resolver::{ResolveContext, ResolverRegistry};
use parser::{ParseResult, ParserRegistry};
use rayon::prelude::*;
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// EvalFileSystem — mirrors cli RealFileSystem
// ---------------------------------------------------------------------------

pub struct EvalFileSystem;

impl domain::ports::FileSystem for EvalFileSystem {
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
        std::fs::read_to_string(path)
            .map_err(|e| CodeGraphError::FileSystem { path: path.into(), source: e })
    }

    fn file_hash(&self, path: &Path) -> Result<String> {
        let content = std::fs::read(path)
            .map_err(|e| CodeGraphError::FileSystem { path: path.into(), source: e })?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        Ok(format!("{:x}", hasher.finalize()))
    }
}

// ---------------------------------------------------------------------------
// EvalParseProvider — mirrors cli RayonParseProvider
// ---------------------------------------------------------------------------

pub struct EvalParseProvider {
    registry: ParserRegistry,
}

impl EvalParseProvider {
    pub fn new() -> Self {
        Self {
            registry: ParserRegistry::new(),
        }
    }

    fn compute_hash(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        format!("{:x}", hasher.finalize())
    }
}

impl domain::ports::ParseProvider for EvalParseProvider {
    fn parse_and_resolve(
        &self,
        files: &[(PathBuf, Vec<u8>)],
        project_root: &Path,
    ) -> Result<Vec<FileData>> {
        if files.is_empty() {
            return Ok(vec![]);
        }

        // Phase 1: parallel parse
        let parse_results: Vec<(PathBuf, Vec<u8>, ParseResult, domain::model::Language)> = files
            .par_iter()
            .filter_map(|(path, source)| {
                let parser = self.registry.parser_for_file(path)?;
                match parser.parse(source, path) {
                    Ok(result) => Some((path.clone(), source.clone(), result, parser.language())),
                    Err(e) => {
                        tracing::warn!("parse failed for {}: {e}", path.display());
                        None
                    }
                }
            })
            .collect();

        // Phase 2: build ResolveContext
        let parsed_files: HashMap<PathBuf, ParseResult> = parse_results
            .iter()
            .map(|(path, _, result, _)| (path.clone(), result.clone()))
            .collect();

        let file_tree: Vec<PathBuf> = files.iter().map(|(p, _)| p.clone()).collect();

        let context = ResolveContext {
            project_root: project_root.to_path_buf(),
            parsed_files,
            file_tree,
        };

        // Phase 3: resolve imports (parallel)
        let resolver_registry = ResolverRegistry::new(project_root);

        let file_data: Vec<FileData> = parse_results
            .par_iter()
            .map(|(path, source, parse_result, lang)| {
                let resolved_edges = resolver_registry
                    .resolve_file(path, *lang, parse_result, &context)
                    .unwrap_or_else(|e| {
                        tracing::warn!("resolve failed for {}: {e}", path.display());
                        vec![]
                    });

                let mut all_edges: Vec<Edge> = parse_result.edges.clone();
                all_edges.extend(resolved_edges);

                let file = FileNode {
                    path: path.clone(),
                    language: *lang,
                    hash: Self::compute_hash(source),
                };

                FileData {
                    file,
                    symbols: parse_result.symbols.clone(),
                    edges: all_edges,
                }
            })
            .collect();

        Ok(file_data)
    }
}

// ---------------------------------------------------------------------------
// NoOpGitProvider — eval doesn't need git operations
// ---------------------------------------------------------------------------

use domain::model::DiffHunk;
use domain::ports::GitProvider;

pub struct NoOpGitProvider;

impl GitProvider for NoOpGitProvider {
    fn current_head(&self) -> Result<String> {
        Ok("eval".into())
    }
    fn changed_files(&self, _from: &str, _to: &str) -> Result<Vec<PathBuf>> {
        Ok(vec![])
    }
    fn diff_hunks(&self, _from: &str, _to: Option<&str>) -> Result<Vec<DiffHunk>> {
        Ok(vec![])
    }
    fn modified_files(&self) -> Result<Vec<PathBuf>> {
        Ok(vec![])
    }
}
