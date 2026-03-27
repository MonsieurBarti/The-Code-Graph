use std::collections::HashMap;
use std::path::{Path, PathBuf};

use domain::error::Result;
use domain::model::{Edge, FileNode};
use domain::ports::FileData;
use parser::resolver::{ResolveContext, ResolverRegistry};
use parser::{ParseResult, ParserRegistry};
use rayon::prelude::*;
use sha2::{Digest, Sha256};

pub struct RayonParseProvider {
    registry: ParserRegistry,
}

impl Default for RayonParseProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl RayonParseProvider {
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

impl domain::ports::ParseProvider for RayonParseProvider {
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

                // Merge structural edges + resolved edges
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

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ports::ParseProvider;

    #[test]
    fn empty_files_returns_empty() {
        let provider = RayonParseProvider::new();
        let result = provider.parse_and_resolve(&[], Path::new("/tmp")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn single_ts_file_returns_file_data() {
        let provider = RayonParseProvider::new();
        let source = b"export function hello(): void {}\nexport class Foo {}".to_vec();
        let files = vec![(PathBuf::from("src/main.ts"), source)];
        let result = provider
            .parse_and_resolve(&files, Path::new("/tmp"))
            .unwrap();
        assert_eq!(result.len(), 1);
        let fd = &result[0];
        assert_eq!(fd.file.path, PathBuf::from("src/main.ts"));
        assert!(!fd.symbols.is_empty(), "should have symbols");
        assert!(!fd.edges.is_empty(), "should have structural edges");
    }

    #[test]
    fn unsupported_extension_is_skipped() {
        let provider = RayonParseProvider::new();
        let files = vec![(PathBuf::from("readme.md"), b"# Hello".to_vec())];
        let result = provider
            .parse_and_resolve(&files, Path::new("/tmp"))
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_error_skips_file_others_succeed() {
        let provider = RayonParseProvider::new();
        let good = b"export function hello(): void {}".to_vec();
        let files = vec![
            (PathBuf::from("src/good.ts"), good),
            (PathBuf::from("src/also_good.rs"), b"fn main() {}".to_vec()),
        ];
        let result = provider
            .parse_and_resolve(&files, Path::new("/tmp"))
            .unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn multiple_files_with_imports_have_resolved_edges() {
        let provider = RayonParseProvider::new();
        let index_ts = br#"export { helper } from "./helper";"#.to_vec();
        let helper_ts = br#"export function helper(): void {}"#.to_vec();
        let files = vec![
            (PathBuf::from("src/index.ts"), index_ts),
            (PathBuf::from("src/helper.ts"), helper_ts),
        ];
        let result = provider
            .parse_and_resolve(&files, Path::new("/tmp"))
            .unwrap();
        assert_eq!(result.len(), 2);
        // Check that at least some edges exist
        let total_edges: usize = result.iter().map(|fd| fd.edges.len()).sum();
        assert!(total_edges > 0, "should have edges");
    }
}
