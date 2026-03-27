use std::collections::HashSet;
use std::path::{Path, PathBuf};

use domain::model::{Edge, EdgeKind, Language};

use super::{ImportResolver, ResolveContext};
use crate::{Export, ParseResult};

/// TS/JS import resolver using oxc_resolver + barrel chain traversal.
pub struct TypeScriptResolver {
    _project_root: PathBuf,
}

impl TypeScriptResolver {
    pub fn new(project_root: &Path) -> Self {
        Self {
            _project_root: project_root.to_path_buf(),
        }
    }

    fn build_resolver() -> oxc_resolver::Resolver {
        oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
            extensions: vec![
                ".ts".into(),
                ".tsx".into(),
                ".js".into(),
                ".jsx".into(),
                ".json".into(),
                ".mjs".into(),
                ".mts".into(),
            ],
            condition_names: vec![
                "import".into(),
                "require".into(),
                "node".into(),
                "default".into(),
            ],
            main_fields: vec!["module".into(), "main".into()],
            ..Default::default()
        })
    }

    /// Resolve a specifier from a given file, returning the absolute resolved path.
    /// Returns None if the specifier cannot be resolved (e.g. node_modules not present).
    fn resolve_specifier(
        resolver: &oxc_resolver::Resolver,
        from_file: &Path,
        specifier: &str,
    ) -> Option<PathBuf> {
        let dir = from_file.parent()?;
        resolver
            .resolve(dir, specifier)
            .ok()
            .map(|r| r.into_path_buf())
    }

    /// Traverse a barrel chain starting from `start_file`, looking for where `name` is
    /// ultimately defined (non-reexport). Returns the origin file path.
    ///
    /// - Max depth: 10 hops
    /// - Cycle detection via visited set
    /// - Checks named re-exports first, then star re-exports
    fn trace_barrel_chain(
        resolver: &oxc_resolver::Resolver,
        context: &ResolveContext,
        start_file: &Path,
        name: &str,
        visited: &mut HashSet<PathBuf>,
    ) -> Option<PathBuf> {
        if visited.len() >= 10 {
            return None;
        }
        if !visited.insert(start_file.to_path_buf()) {
            // Cycle detected
            return None;
        }

        let parse_result = context.parsed_files.get(start_file)?;

        // Check named re-exports: `export { X } from "./mod"`
        for export in &parse_result.exports {
            if export.is_reexport
                && export.name == name
                && export.source_specifier.is_some()
                && export.name != "*"
            {
                if let Some(specifier) = &export.source_specifier {
                    if let Some(target) = Self::resolve_specifier(resolver, start_file, specifier) {
                        // Recurse: the name may be re-exported further
                        return Self::trace_barrel_chain(resolver, context, &target, name, visited)
                            .or(Some(target));
                    }
                }
            }
        }

        // Check star re-exports: `export * from "./mod"`
        for export in &parse_result.exports {
            if export.is_reexport && export.name == "*" {
                if let Some(specifier) = &export.source_specifier {
                    if let Some(target) = Self::resolve_specifier(resolver, start_file, specifier) {
                        // Check if target has the name (directly or via re-export)
                        if let Some(target_result) = context.parsed_files.get(&target) {
                            let has_name = target_result.exports.iter().any(|e| e.name == name);
                            if has_name {
                                let mut sub_visited = visited.clone();
                                return Self::trace_barrel_chain(
                                    resolver,
                                    context,
                                    &target,
                                    name,
                                    &mut sub_visited,
                                )
                                .or(Some(target));
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Build re-export edges from a file's exports list.
    fn build_reexport_edges(
        resolver: &oxc_resolver::Resolver,
        file_path: &Path,
        exports: &[Export],
        edges: &mut Vec<Edge>,
    ) {
        for export in exports {
            if !export.is_reexport {
                continue;
            }
            let Some(specifier) = &export.source_specifier else {
                continue;
            };
            let Some(target) = Self::resolve_specifier(resolver, file_path, specifier) else {
                continue;
            };
            let target_str = target.to_string_lossy().into_owned();
            let source_str = file_path.to_string_lossy().into_owned();

            let kind = if export.name == "*" {
                EdgeKind::BarrelReExportAll
            } else {
                EdgeKind::ReExport
            };

            edges.push(Edge {
                kind,
                source: source_str,
                target: target_str,
                metadata: None,
            });
        }
    }
}

impl ImportResolver for TypeScriptResolver {
    fn languages(&self) -> &[Language] {
        &[Language::TypeScript, Language::JavaScript]
    }

    fn resolve(
        &self,
        file_path: &Path,
        parse_result: &ParseResult,
        context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>> {
        let resolver = Self::build_resolver();
        let mut edges = Vec::new();

        // AC32/AC29: ImportsFrom edges for each import
        for import in &parse_result.imports {
            let Some(target) = Self::resolve_specifier(&resolver, file_path, &import.specifier)
            else {
                // AC: unresolvable imports are silently skipped
                continue;
            };

            let source_str = file_path.to_string_lossy().into_owned();
            let target_str = target.to_string_lossy().into_owned();

            edges.push(Edge {
                kind: EdgeKind::ImportsFrom,
                source: source_str.clone(),
                target: target_str.clone(),
                metadata: None,
            });

            // AC30: Barrel chain traversal for named imports
            if !import.is_namespace && !import.is_side_effect && !import.names.is_empty() {
                for imp_name in &import.names {
                    let mut visited = HashSet::new();
                    if let Some(origin) = Self::trace_barrel_chain(
                        &resolver,
                        context,
                        &target,
                        &imp_name.name,
                        &mut visited,
                    ) {
                        let origin_str = origin.to_string_lossy().into_owned();
                        // Only add if different from the direct target (would be a duplicate otherwise)
                        if origin_str != target_str {
                            edges.push(Edge {
                                kind: EdgeKind::ImportsFrom,
                                source: source_str.clone(),
                                target: origin_str,
                                metadata: None,
                            });
                        }
                    }
                }
            }
        }

        // AC33/AC34: BarrelReExportAll and ReExport edges from this file's exports
        Self::build_reexport_edges(&resolver, file_path, &parse_result.exports, &mut edges);

        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    fn make_context(project_root: &Path) -> ResolveContext {
        ResolveContext {
            project_root: project_root.to_path_buf(),
            parsed_files: HashMap::new(),
            file_tree: vec![],
        }
    }

    // -----------------------------------------------------------------------
    // AC: unresolvable imports are skipped (AC29 negative path)
    // -----------------------------------------------------------------------
    #[test]
    fn skips_unresolvable_imports() {
        let resolver = TypeScriptResolver::new(Path::new("/nonexistent"));
        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "./nonexistent_module".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = make_context(Path::new("/nonexistent"));
        let edges = resolver
            .resolve(Path::new("/nonexistent/test.ts"), &parse_result, &context)
            .unwrap();
        assert!(
            edges.is_empty(),
            "Unresolvable imports should produce no edges"
        );
    }

    // -----------------------------------------------------------------------
    // AC33: BarrelReExportAll edge from `export * from "./mod"`
    // -----------------------------------------------------------------------
    #[test]
    fn creates_barrel_reexport_all_edge() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();

        // Create source file
        let mod_path = root.join("mod.ts");
        fs::write(&mod_path, "export const x = 1;\n").unwrap();

        // Create barrel file with export * from "./mod"
        let barrel_path = root.join("index.ts");
        fs::write(&barrel_path, "export * from './mod';\n").unwrap();

        let resolver = TypeScriptResolver::new(&root);
        let parse_result = ParseResult {
            exports: vec![crate::Export {
                name: "*".into(),
                is_reexport: true,
                source_specifier: Some("./mod".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = make_context(&root);
        let edges = resolver
            .resolve(&barrel_path, &parse_result, &context)
            .unwrap();

        assert_eq!(edges.len(), 1, "Expected one BarrelReExportAll edge");
        assert_eq!(edges[0].kind, EdgeKind::BarrelReExportAll);
        assert_eq!(edges[0].source, barrel_path.to_string_lossy());
        assert_eq!(edges[0].target, mod_path.to_string_lossy());
    }

    // -----------------------------------------------------------------------
    // AC34: ReExport edge from `export { X } from "./mod"`
    // -----------------------------------------------------------------------
    #[test]
    fn creates_reexport_edge_for_named_reexport() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();

        let mod_path = root.join("utils.ts");
        fs::write(&mod_path, "export function foo() {}\n").unwrap();

        let barrel_path = root.join("index.ts");
        fs::write(&barrel_path, "export { foo } from './utils';\n").unwrap();

        let resolver = TypeScriptResolver::new(&root);
        let parse_result = ParseResult {
            exports: vec![crate::Export {
                name: "foo".into(),
                is_reexport: true,
                source_specifier: Some("./utils".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = make_context(&root);
        let edges = resolver
            .resolve(&barrel_path, &parse_result, &context)
            .unwrap();

        assert_eq!(edges.len(), 1, "Expected one ReExport edge");
        assert_eq!(edges[0].kind, EdgeKind::ReExport);
        assert_eq!(edges[0].source, barrel_path.to_string_lossy());
        assert_eq!(edges[0].target, mod_path.to_string_lossy());
    }

    // -----------------------------------------------------------------------
    // AC32/AC29: ImportsFrom edge for a resolvable import
    // -----------------------------------------------------------------------
    #[test]
    fn creates_imports_from_edge_for_resolvable_import() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();

        let target_path = root.join("utils.ts");
        fs::write(&target_path, "export function foo() {}\n").unwrap();

        let importer_path = root.join("main.ts");
        fs::write(&importer_path, "import { foo } from './utils';\n").unwrap();

        let resolver = TypeScriptResolver::new(&root);
        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "./utils".into(),
                names: vec![crate::ImportName {
                    name: "foo".into(),
                    alias: None,
                    is_type: false,
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = make_context(&root);
        let edges = resolver
            .resolve(&importer_path, &parse_result, &context)
            .unwrap();

        assert!(
            edges.iter().any(|e| e.kind == EdgeKind::ImportsFrom
                && e.source == importer_path.to_string_lossy()
                && e.target == target_path.to_string_lossy()),
            "Expected ImportsFrom edge from main.ts to utils.ts, got: {:?}",
            edges
        );
    }

    // -----------------------------------------------------------------------
    // AC30: Barrel chain traversal through index.ts re-exports
    // -----------------------------------------------------------------------
    #[test]
    fn traces_barrel_chain_through_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();

        // Create: utils/helper.ts (origin)
        let utils_dir = root.join("utils");
        fs::create_dir_all(&utils_dir).unwrap();
        let helper_path = utils_dir.join("helper.ts");
        fs::write(&helper_path, "export function doThing() {}\n").unwrap();

        // Create: utils/index.ts (barrel) — `export * from "./helper"`
        let utils_index = utils_dir.join("index.ts");
        fs::write(&utils_index, "export * from './helper';\n").unwrap();

        // Create: src/main.ts imports from utils/index
        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let main_path = src_dir.join("main.ts");
        fs::write(&main_path, "import { doThing } from '../utils';\n").unwrap();

        // Build context with the barrel's parse result
        // Use canonicalized paths as keys so context.parsed_files lookup matches resolved paths
        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            utils_index.clone(),
            ParseResult {
                exports: vec![crate::Export {
                    name: "*".into(),
                    is_reexport: true,
                    source_specifier: Some("./helper".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        // helper.ts exports doThing (non-reexport)
        parsed_files.insert(
            helper_path.clone(),
            ParseResult {
                exports: vec![crate::Export {
                    name: "doThing".into(),
                    is_reexport: false,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );

        let context = ResolveContext {
            project_root: root.clone(),
            parsed_files,
            file_tree: vec![],
        };

        let resolver = TypeScriptResolver::new(&root);
        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "../utils".into(),
                names: vec![crate::ImportName {
                    name: "doThing".into(),
                    alias: None,
                    is_type: false,
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let edges = resolver
            .resolve(&main_path, &parse_result, &context)
            .unwrap();

        // Should have ImportsFrom from main.ts → utils/index.ts (direct)
        let direct = edges.iter().any(|e| {
            e.kind == EdgeKind::ImportsFrom
                && e.source == main_path.to_string_lossy()
                && e.target == utils_index.to_string_lossy()
        });
        assert!(
            direct,
            "Expected direct ImportsFrom edge to barrel index, got: {:?}",
            edges
        );

        // Should also have ImportsFrom from main.ts → utils/helper.ts (origin via barrel chain)
        let through_chain = edges.iter().any(|e| {
            e.kind == EdgeKind::ImportsFrom
                && e.source == main_path.to_string_lossy()
                && e.target == helper_path.to_string_lossy()
        });
        assert!(
            through_chain,
            "Expected barrel-chain ImportsFrom edge to origin helper.ts, got: {:?}",
            edges
        );
    }

    // -----------------------------------------------------------------------
    // AC31: Circular barrel → graceful termination (no panic, no infinite loop)
    // -----------------------------------------------------------------------
    #[test]
    fn circular_barrel_terminates_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let root = root.as_path();

        // a.ts exports * from ./b, b.ts exports * from ./a (cycle)
        let a_path = root.join("a.ts");
        let b_path = root.join("b.ts");
        fs::write(&a_path, "export * from './b';\n").unwrap();
        fs::write(&b_path, "export * from './a';\n").unwrap();

        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            a_path.clone(),
            ParseResult {
                exports: vec![crate::Export {
                    name: "*".into(),
                    is_reexport: true,
                    source_specifier: Some("./b".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        parsed_files.insert(
            b_path.clone(),
            ParseResult {
                exports: vec![crate::Export {
                    name: "*".into(),
                    is_reexport: true,
                    source_specifier: Some("./a".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );

        let context = ResolveContext {
            project_root: root.to_path_buf(),
            parsed_files,
            file_tree: vec![],
        };

        // main.ts imports { something } from ./a — this triggers barrel chain on a cycle
        let main_path = root.join("main.ts");
        fs::write(&main_path, "import { something } from './a';\n").unwrap();

        let resolver = TypeScriptResolver::new(root);
        let parse_result = ParseResult {
            imports: vec![crate::RawImport {
                specifier: "./a".into(),
                names: vec![crate::ImportName {
                    name: "something".into(),
                    alias: None,
                    is_type: false,
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Should not panic or infinite-loop, and must return Ok
        let result = resolver.resolve(&main_path, &parse_result, &context);
        assert!(result.is_ok(), "Circular barrel must not cause an error");
    }

    // -----------------------------------------------------------------------
    // Exports with no source_specifier are ignored
    // -----------------------------------------------------------------------
    #[test]
    fn ignores_non_reexport_exports() {
        let resolver = TypeScriptResolver::new(Path::new("/tmp"));
        let parse_result = ParseResult {
            exports: vec![crate::Export {
                name: "MyFn".into(),
                is_reexport: false,
                source_specifier: None,
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = make_context(Path::new("/tmp"));
        let edges = resolver
            .resolve(Path::new("/tmp/src.ts"), &parse_result, &context)
            .unwrap();
        assert!(
            edges.is_empty(),
            "Non-reexport exports should not create edges"
        );
    }
}
