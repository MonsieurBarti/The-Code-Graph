use std::path::{Path, PathBuf};

use domain::model::{Edge, EdgeKind, Language};

use super::{ImportResolver, ResolveContext};
use crate::ParseResult;

/// Configuration for the Go resolver, loaded from go.mod.
pub struct GoConfig {
    pub module_path: Option<String>,
}

impl GoConfig {
    pub fn load(project_root: &Path) -> Self {
        GoConfig { module_path: parse_go_mod(project_root) }
    }
}

/// Go import resolver — go.mod + module path resolution.
pub struct GoResolver {
    config: GoConfig,
}

impl GoResolver {
    pub fn new(config: GoConfig) -> Self { Self { config } }
}

/// Parse go.mod from the filesystem and extract the module path.
fn parse_go_mod(project_root: &Path) -> Option<String> {
    let go_mod_path = project_root.join("go.mod");
    let contents = std::fs::read_to_string(go_mod_path).ok()?;
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("module ") {
            let module_path = rest.trim();
            if !module_path.is_empty() {
                return Some(module_path.to_string());
            }
        }
    }
    None
}

/// Check if an import path is from the Go standard library.
/// Standard library packages have a first path element with no dots.
fn is_stdlib(import_path: &str) -> bool {
    let first = import_path.split('/').next().unwrap_or("");
    !first.contains('.')
}

/// Resolve a local module import path to a directory in the project.
/// Returns the resolved directory PathBuf if the import is local and files exist under it.
fn resolve_local_import(
    import_path: &str,
    module_path: &str,
    project_root: &Path,
    file_tree: &[PathBuf],
) -> Option<PathBuf> {
    if !import_path.starts_with(module_path) {
        return None;
    }
    // Strip module prefix and leading slash
    let relative = import_path[module_path.len()..].trim_start_matches('/');
    let dir = project_root.join(relative);

    // Check if any file in file_tree lives under this directory
    let has_files = file_tree.iter().any(|p| p.starts_with(&dir));
    if has_files {
        Some(dir)
    } else {
        None
    }
}

impl ImportResolver for GoResolver {
    fn languages(&self) -> &[Language] {
        &[Language::Go]
    }

    fn resolve(
        &self,
        file_path: &Path,
        parse_result: &ParseResult,
        context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>> {
        let source = file_path.to_string_lossy().to_string();

        // Try to read go.mod; if absent we can still handle side-effect/dot imports
        let module_path = self.config.module_path.clone();

        let mut edges = Vec::new();

        for import in &parse_result.imports {
            let specifier = &import.specifier;

            // Blank imports → SideEffectImport (always, regardless of path type)
            if import.is_side_effect {
                edges.push(Edge {
                    kind: EdgeKind::SideEffectImport,
                    source: source.clone(),
                    target: specifier.clone(),
                    metadata: None,
                });
                continue;
            }

            // Dot imports → DotImport
            if import.is_namespace {
                let target = if let Some(ref mp) = module_path {
                    resolve_local_import(specifier, mp, &context.project_root, &context.file_tree)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| specifier.clone())
                } else {
                    specifier.clone()
                };
                edges.push(Edge {
                    kind: EdgeKind::DotImport,
                    source: source.clone(),
                    target,
                    metadata: None,
                });
                continue;
            }

            // Standard library → skip
            if is_stdlib(specifier) {
                continue;
            }

            // Local module import
            if let Some(ref mp) = module_path {
                if let Some(resolved) =
                    resolve_local_import(specifier, mp, &context.project_root, &context.file_tree)
                {
                    edges.push(Edge {
                        kind: EdgeKind::ImportsFrom,
                        source: source.clone(),
                        target: resolved.to_string_lossy().to_string(),
                        metadata: None,
                    });
                    continue;
                }
            }

            // External (third-party) → skip
        }

        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ---------------------------------------------------------------------------
    // AC46: Skip stdlib imports
    // ---------------------------------------------------------------------------

    #[test]
    fn skips_stdlib_imports() {
        let resolver = GoResolver::new(GoConfig { module_path: None });
        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "fmt".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = ResolveContext {
            project_root: "/nonexistent".into(),
            parsed_files: HashMap::new(),
            file_tree: vec![],
        };
        let edges = resolver
            .resolve(Path::new("main.go"), &parse_result, &context)
            .unwrap();
        assert!(edges.is_empty());
    }

    #[test]
    fn skips_stdlib_multipart_path() {
        let resolver = GoResolver::new(GoConfig { module_path: None });
        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "net/http".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = ResolveContext {
            project_root: "/nonexistent".into(),
            parsed_files: HashMap::new(),
            file_tree: vec![],
        };
        let edges = resolver
            .resolve(Path::new("main.go"), &parse_result, &context)
            .unwrap();
        assert!(edges.is_empty(), "net/http is stdlib and must be skipped");
    }

    // ---------------------------------------------------------------------------
    // AC47: SideEffectImport for blank imports
    // ---------------------------------------------------------------------------

    #[test]
    fn creates_side_effect_import_edge() {
        let resolver = GoResolver::new(GoConfig { module_path: None });
        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "github.com/lib/pq".into(),
                is_side_effect: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = ResolveContext {
            project_root: "/project".into(),
            parsed_files: HashMap::new(),
            file_tree: vec![],
        };
        let edges = resolver
            .resolve(Path::new("main.go"), &parse_result, &context)
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::SideEffectImport);
        assert_eq!(edges[0].source, "main.go");
        assert_eq!(edges[0].target, "github.com/lib/pq");
    }

    #[test]
    fn side_effect_import_targets_raw_specifier() {
        let resolver = GoResolver::new(GoConfig { module_path: None });
        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "database/sql".into(),
                is_side_effect: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = ResolveContext {
            project_root: "/project".into(),
            parsed_files: HashMap::new(),
            file_tree: vec![],
        };
        let edges = resolver
            .resolve(Path::new("cmd/app/main.go"), &parse_result, &context)
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::SideEffectImport);
        assert_eq!(edges[0].source, "cmd/app/main.go");
    }

    // ---------------------------------------------------------------------------
    // AC48: DotImport for dot imports
    // ---------------------------------------------------------------------------

    #[test]
    fn creates_dot_import_edge() {
        let resolver = GoResolver::new(GoConfig { module_path: None });
        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "github.com/some/pkg".into(),
                is_namespace: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = ResolveContext {
            project_root: "/project".into(),
            parsed_files: HashMap::new(),
            file_tree: vec![],
        };
        let edges = resolver
            .resolve(Path::new("main.go"), &parse_result, &context)
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::DotImport);
    }

    // ---------------------------------------------------------------------------
    // AC44 + AC45: go.mod parsing and local import resolution
    // ---------------------------------------------------------------------------

    #[test]
    fn resolves_local_import_to_directory() {
        // Set up a temporary directory with go.mod
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path().to_path_buf();
        std::fs::write(
            project_root.join("go.mod"),
            "module github.com/myorg/myapp\n\ngo 1.21\n",
        )
        .unwrap();
        let resolver = GoResolver::new(GoConfig::load(&project_root));

        // Simulate a file tree with files under internal/store
        let store_dir = project_root.join("internal").join("store");
        let store_file = store_dir.join("store.go");

        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "github.com/myorg/myapp/internal/store".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = ResolveContext {
            project_root: project_root.clone(),
            parsed_files: HashMap::new(),
            file_tree: vec![store_file.clone()],
        };

        let edges = resolver
            .resolve(Path::new("main.go"), &parse_result, &context)
            .unwrap();

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::ImportsFrom);
        assert_eq!(edges[0].source, "main.go");
        assert_eq!(edges[0].target, store_dir.to_string_lossy());
    }

    #[test]
    fn skips_external_third_party_imports() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path().to_path_buf();
        std::fs::write(
            project_root.join("go.mod"),
            "module github.com/myorg/myapp\n\ngo 1.21\n",
        )
        .unwrap();
        let resolver = GoResolver::new(GoConfig::load(&project_root));

        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "github.com/some/external".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = ResolveContext {
            project_root,
            parsed_files: HashMap::new(),
            file_tree: vec![],
        };

        let edges = resolver
            .resolve(Path::new("main.go"), &parse_result, &context)
            .unwrap();
        assert!(edges.is_empty(), "external imports must be skipped");
    }

    #[test]
    fn returns_empty_when_go_mod_missing() {
        let resolver = GoResolver::new(GoConfig { module_path: None });
        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "github.com/org/repo/pkg".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = ResolveContext {
            project_root: "/nonexistent_project".into(),
            parsed_files: HashMap::new(),
            file_tree: vec![PathBuf::from("/nonexistent_project/pkg/file.go")],
        };
        let edges = resolver
            .resolve(Path::new("main.go"), &parse_result, &context)
            .unwrap();
        // Without go.mod we can't resolve module-relative paths, so external is skipped
        assert!(edges.is_empty());
    }

    #[test]
    fn multiple_imports_mixed_types() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path().to_path_buf();
        std::fs::write(
            project_root.join("go.mod"),
            "module github.com/myorg/myapp\n\ngo 1.21\n",
        )
        .unwrap();
        let resolver = GoResolver::new(GoConfig::load(&project_root));

        let util_file = project_root.join("util").join("util.go");

        let parse_result = ParseResult {
            imports: vec![
                // stdlib → skip
                crate::RawImport {
                    specifier: "fmt".into(),
                    ..Default::default()
                },
                // external → skip
                crate::RawImport {
                    specifier: "github.com/external/lib".into(),
                    ..Default::default()
                },
                // side effect
                crate::RawImport {
                    specifier: "github.com/lib/pq".into(),
                    is_side_effect: true,
                    ..Default::default()
                },
                // local
                crate::RawImport {
                    specifier: "github.com/myorg/myapp/util".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let context = ResolveContext {
            project_root: project_root.clone(),
            parsed_files: HashMap::new(),
            file_tree: vec![util_file],
        };

        let edges = resolver
            .resolve(Path::new("main.go"), &parse_result, &context)
            .unwrap();

        assert_eq!(edges.len(), 2, "expected side-effect + local edges only");
        assert!(edges.iter().any(|e| e.kind == EdgeKind::SideEffectImport));
        assert!(edges.iter().any(|e| e.kind == EdgeKind::ImportsFrom));
    }

    // ---------------------------------------------------------------------------
    // is_stdlib helper unit tests
    // ---------------------------------------------------------------------------

    #[test]
    fn is_stdlib_detects_standard_packages() {
        assert!(is_stdlib("fmt"));
        assert!(is_stdlib("net/http"));
        assert!(is_stdlib("encoding/json"));
        assert!(is_stdlib("os"));
        assert!(is_stdlib("sync"));
    }

    #[test]
    fn is_stdlib_rejects_third_party() {
        assert!(!is_stdlib("github.com/lib/pq"));
        assert!(!is_stdlib("golang.org/x/sync/errgroup"));
        assert!(!is_stdlib("gopkg.in/yaml.v3"));
    }

    // ---------------------------------------------------------------------------
    // parse_go_mod unit tests
    // ---------------------------------------------------------------------------

    #[test]
    fn parse_go_mod_extracts_module_path() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path();
        std::fs::write(
            project_root.join("go.mod"),
            "module github.com/myorg/myapp\n\ngo 1.21\n",
        )
        .unwrap();
        let result = parse_go_mod(project_root);
        assert_eq!(result, Some("github.com/myorg/myapp".to_string()));
    }

    #[test]
    fn parse_go_mod_returns_none_when_missing() {
        let result = parse_go_mod(Path::new("/nonexistent_dir_xyz"));
        assert_eq!(result, None);
    }

    #[test]
    fn parse_go_mod_handles_whitespace() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path();
        std::fs::write(
            project_root.join("go.mod"),
            "  module   github.com/example/repo  \n\ngo 1.20\n",
        )
        .unwrap();
        // The trimmed line starts with "module " only if after trim it starts that way
        // Our impl trims the whole line, so "module   github.com/..." → strip_prefix("module ")
        // gives "  github.com/..." → trim → "github.com/..."
        let result = parse_go_mod(project_root);
        assert_eq!(result, Some("github.com/example/repo".to_string()));
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn go_config_loads_module_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module github.com/example/app\n\ngo 1.21\n").unwrap();
        let config = GoConfig::load(dir.path());
        assert_eq!(config.module_path.as_deref(), Some("github.com/example/app"));
    }

    #[test]
    fn go_config_none_without_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        let config = GoConfig::load(dir.path());
        assert!(config.module_path.is_none());
    }
}
