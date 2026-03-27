use std::collections::HashMap;
use std::path::{Path, PathBuf};

use domain::model::{Edge, EdgeKind, Language};

use super::{ImportResolver, ResolveContext};
use crate::ParseResult;

/// Configuration for the Rust resolver, loaded from Cargo.toml.
pub struct RustConfig {
    pub workspace_members: Vec<String>,
    pub edition: Option<String>,
}

impl RustConfig {
    pub fn load(project_root: &Path) -> Self {
        let cargo_path = project_root.join("Cargo.toml");
        let contents = match std::fs::read_to_string(&cargo_path) {
            Ok(c) => c,
            Err(_) => return Self { workspace_members: vec![], edition: None },
        };
        let table: toml::Table = match contents.parse() {
            Ok(t) => t,
            Err(_) => return Self { workspace_members: vec![], edition: None },
        };
        let workspace_members = table.get("workspace")
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let edition = table.get("package")
            .and_then(|p| p.get("edition"))
            .and_then(|e| e.as_str())
            .map(String::from);
        Self { workspace_members, edition }
    }
}

/// Rust import resolver — module tree + use path resolution.
pub struct RustResolver {
    _config: RustConfig,
}

impl RustResolver {
    pub fn new(config: RustConfig) -> Self { Self { _config: config } }
}

// ---------------------------------------------------------------------------
// Module tree helpers
// ---------------------------------------------------------------------------

/// A mapping from crate-path prefix (e.g. "crate::auth") to the file that
/// declares that module.
type ModuleTree = HashMap<String, PathBuf>;

/// Find the crate root file (`src/lib.rs` or `src/main.rs`) relative to project_root
/// from the provided file_tree.
fn find_crate_root(project_root: &Path, file_tree: &[PathBuf]) -> Option<PathBuf> {
    let lib_rs = project_root.join("src").join("lib.rs");
    let main_rs = project_root.join("src").join("main.rs");
    for path in file_tree {
        if path == &lib_rs || path == &main_rs {
            return Some(path.clone());
        }
    }
    None
}

/// Recursively build the module tree starting from `current_file`.
/// `current_module_path` is the crate path to the current file (e.g. "crate" for the root).
fn build_module_tree_recursive(
    current_file: &Path,
    current_module_path: &str,
    parsed_files: &HashMap<PathBuf, ParseResult>,
    file_tree: &[PathBuf],
    tree: &mut ModuleTree,
    visited: &mut Vec<PathBuf>,
) {
    // Cycle guard
    if visited.contains(&current_file.to_path_buf()) {
        return;
    }
    visited.push(current_file.to_path_buf());

    let parse_result = match parsed_files.get(current_file) {
        Some(pr) => pr,
        None => return,
    };

    // Determine the directory in which submodule files live.
    //
    // Rust module-file placement rules:
    //  - lib.rs / main.rs           → submodules in <parent>/
    //  - foo/mod.rs                 → submodules in <parent>/ (== foo/)
    //  - foo.rs                     → submodules in <parent>/foo/
    //
    // In all cases "parent" is the directory containing the current file.
    let parent_dir = match current_file.parent() {
        Some(d) => d.to_path_buf(),
        None => return,
    };

    let file_name = current_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let is_root_file = matches!(file_name, "lib.rs" | "main.rs" | "mod.rs");

    // For a flat file (e.g. auth.rs), submodules live in a same-named directory.
    let submodule_dir = if is_root_file {
        parent_dir.clone()
    } else {
        // Strip .rs extension to get directory name
        let stem = current_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        parent_dir.join(stem)
    };

    for import in &parse_result.imports {
        if !import.specifier.starts_with("mod::") {
            continue;
        }
        let mod_name = &import.specifier["mod::".len()..];

        // Resolve to file path: flat style (foo.rs) or legacy style (foo/mod.rs)
        let flat = submodule_dir.join(format!("{mod_name}.rs"));
        let legacy = submodule_dir.join(mod_name).join("mod.rs");

        let mod_file = if file_tree.contains(&flat) {
            flat
        } else if file_tree.contains(&legacy) {
            legacy
        } else {
            continue;
        };

        let child_module_path = format!("{current_module_path}::{mod_name}");
        tree.insert(child_module_path.clone(), mod_file.clone());

        build_module_tree_recursive(
            &mod_file,
            &child_module_path,
            parsed_files,
            file_tree,
            tree,
            visited,
        );
    }
}

/// Build the complete module tree for a crate, starting from the crate root.
/// Returns a map of `"crate::foo::bar"` → resolved file path.
fn build_module_tree(context: &ResolveContext) -> ModuleTree {
    let mut tree = ModuleTree::new();

    let crate_root = match find_crate_root(&context.project_root, &context.file_tree) {
        Some(r) => r,
        None => return tree,
    };

    let mut visited = Vec::new();
    build_module_tree_recursive(
        &crate_root,
        "crate",
        &context.parsed_files,
        &context.file_tree,
        &mut tree,
        &mut visited,
    );

    tree
}

/// Given a file path, derive its crate module path (e.g. `src/auth/validate.rs` → `crate::auth::validate`).
/// This is the inverse lookup: file → module path.
fn file_to_module_path(
    project_root: &Path,
    file_path: &Path,
    module_tree: &ModuleTree,
) -> Option<String> {
    for (module_path, mapped_file) in module_tree {
        if mapped_file == file_path {
            return Some(module_path.clone());
        }
    }

    // Check if this is the crate root itself
    let lib_rs = project_root.join("src").join("lib.rs");
    let main_rs = project_root.join("src").join("main.rs");
    if file_path == lib_rs || file_path == main_rs {
        return Some("crate".to_string());
    }

    None
}

/// Resolve a `use` specifier to a target file path by walking the module tree.
///
/// - `crate::foo::bar` → look up `"crate::foo"` then `"crate::foo::bar"` in the tree
/// - `self::sub` → resolve `self` to the current module's path, then recurse
/// - `super::foo` → resolve `super` to the parent module's path, then recurse
fn resolve_specifier_to_file(
    specifier: &str,
    file_path: &Path,
    project_root: &Path,
    module_tree: &ModuleTree,
) -> Option<PathBuf> {
    let segments: Vec<&str> = specifier.split("::").collect();
    if segments.is_empty() {
        return None;
    }

    // Determine the base module path for self/super resolution
    let base_module_path = match segments[0] {
        "self" => {
            // "self" refers to the current module
            file_to_module_path(project_root, file_path, module_tree)?
        }
        "super" => {
            // "super" refers to the parent module
            let current = file_to_module_path(project_root, file_path, module_tree)?;
            // Strip one segment from the end
            current.rsplit_once("::").map(|(p, _)| p.to_string())?
        }
        "crate" => "crate".to_string(),
        _ => return None,
    };

    // Remaining segments after the keyword
    let rest_segments = &segments[1..];

    // For self:: and super:: paths, the base module file is the fallback when
    // the remaining path refers to an item (function, struct, etc.) rather than
    // a submodule. For crate:: paths we start with no fallback — if no segment
    // matches a module in the tree, the path is unresolvable (external crate, std).
    let is_relative = matches!(segments[0], "self" | "super");
    let initial_resolved: Option<PathBuf> = if is_relative && base_module_path != "crate" {
        module_tree.get(&base_module_path).cloned()
    } else {
        None
    };

    // Try progressively longer prefixes and return the deepest match.
    // e.g. for crate::auth::validate::something:
    //   try crate::auth → auth.rs           (resolved updated)
    //   try crate::auth::validate → validate.rs (resolved updated)
    //   try crate::auth::validate::something → not in tree, skip
    let mut resolved = initial_resolved;
    let mut candidate_path = base_module_path.clone();

    for seg in rest_segments {
        candidate_path = format!("{candidate_path}::{seg}");
        if let Some(file) = module_tree.get(&candidate_path) {
            resolved = Some(file.clone());
        }
    }

    resolved
}

// ---------------------------------------------------------------------------
// ImportResolver implementation
// ---------------------------------------------------------------------------

impl ImportResolver for RustResolver {
    fn languages(&self) -> &[Language] {
        &[Language::Rust]
    }

    fn resolve(
        &self,
        file_path: &Path,
        parse_result: &ParseResult,
        context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>> {
        let module_tree = build_module_tree(context);
        let mut edges = Vec::new();

        let source_str = file_path.to_string_lossy().into_owned();

        for import in &parse_result.imports {
            // Skip mod declarations — they're not import edges
            if import.specifier.starts_with("mod::") {
                continue;
            }

            let Some(target_file) = resolve_specifier_to_file(
                &import.specifier,
                file_path,
                &context.project_root,
                &module_tree,
            ) else {
                continue;
            };

            let target_str = target_file.to_string_lossy().into_owned();

            // Determine edge kind: ReExport for pub use, ImportsFrom otherwise
            let is_reexport = parse_result
                .exports
                .iter()
                .any(|e| e.is_reexport && e.source_specifier.as_deref() == Some(&import.specifier));

            let edge_kind = if is_reexport {
                EdgeKind::ReExport
            } else {
                EdgeKind::ImportsFrom
            };

            edges.push(Edge {
                kind: edge_kind,
                source: source_str.clone(),
                target: target_str,
                metadata: None,
            });
        }

        Ok(edges)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Helper: build a ResolveContext from in-memory data.
    fn make_context(
        project_root: PathBuf,
        file_tree: Vec<PathBuf>,
        parsed_files: HashMap<PathBuf, ParseResult>,
    ) -> ResolveContext {
        ResolveContext {
            project_root,
            parsed_files,
            file_tree,
        }
    }

    /// Helper: build a RawImport for a `mod::` declaration.
    fn mod_import(name: &str) -> crate::RawImport {
        crate::RawImport {
            specifier: format!("mod::{name}"),
            ..Default::default()
        }
    }

    /// Helper: build a RawImport for a `use` path.
    fn use_import(specifier: &str) -> crate::RawImport {
        crate::RawImport {
            specifier: specifier.to_string(),
            ..Default::default()
        }
    }

    /// Helper: build a pub-use Export entry (is_reexport = true).
    fn pub_use_export(specifier: &str) -> crate::Export {
        crate::Export {
            name: specifier.rsplit("::").next().unwrap_or("").to_string(),
            is_reexport: true,
            source_specifier: Some(specifier.to_string()),
            ..Default::default()
        }
    }

    // -----------------------------------------------------------------------
    // AC35: builds_module_tree_from_mod_declarations
    // -----------------------------------------------------------------------
    #[test]
    fn builds_module_tree_from_mod_declarations() {
        let root = PathBuf::from("/project");
        let lib_rs = root.join("src/lib.rs");
        let auth_rs = root.join("src/auth.rs");
        let db_rs = root.join("src/db.rs");

        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            lib_rs.clone(),
            ParseResult {
                imports: vec![mod_import("auth"), mod_import("db")],
                ..Default::default()
            },
        );
        parsed_files.insert(auth_rs.clone(), ParseResult::default());
        parsed_files.insert(db_rs.clone(), ParseResult::default());

        let context = make_context(
            root.clone(),
            vec![lib_rs.clone(), auth_rs.clone(), db_rs.clone()],
            parsed_files,
        );

        let tree = build_module_tree(&context);

        assert!(
            tree.contains_key("crate::auth"),
            "module tree should contain crate::auth, got: {tree:?}"
        );
        assert_eq!(tree["crate::auth"], auth_rs);
        assert!(
            tree.contains_key("crate::db"),
            "module tree should contain crate::db, got: {tree:?}"
        );
        assert_eq!(tree["crate::db"], db_rs);
    }

    // -----------------------------------------------------------------------
    // AC36: resolves use crate::auth::validate to src/auth.rs
    // -----------------------------------------------------------------------
    #[test]
    fn resolves_use_crate_path() {
        let root = PathBuf::from("/project");
        let lib_rs = root.join("src/lib.rs");
        let auth_rs = root.join("src/auth.rs");
        let main_rs = root.join("src/main.rs");

        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            lib_rs.clone(),
            ParseResult {
                imports: vec![mod_import("auth")],
                ..Default::default()
            },
        );
        parsed_files.insert(auth_rs.clone(), ParseResult::default());

        let context = make_context(
            root.clone(),
            vec![lib_rs.clone(), auth_rs.clone()],
            parsed_files,
        );

        let resolver = RustResolver::new(RustConfig { workspace_members: vec![], edition: None });
        let importer = ParseResult {
            imports: vec![use_import("crate::auth::validate")],
            ..Default::default()
        };

        let edges = resolver.resolve(&main_rs, &importer, &context).unwrap();

        assert_eq!(
            edges.len(),
            1,
            "Expected one ImportsFrom edge, got: {edges:?}"
        );
        assert_eq!(edges[0].kind, EdgeKind::ImportsFrom);
        assert_eq!(edges[0].source, main_rs.to_string_lossy());
        assert_eq!(edges[0].target, auth_rs.to_string_lossy());
    }

    // -----------------------------------------------------------------------
    // AC37: resolves use self::sub relative to current module
    // -----------------------------------------------------------------------
    #[test]
    fn resolves_use_self_path() {
        let root = PathBuf::from("/project");
        let lib_rs = root.join("src/lib.rs");
        let auth_rs = root.join("src/auth.rs");
        let sub_rs = root.join("src/auth/sub.rs");

        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            lib_rs.clone(),
            ParseResult {
                imports: vec![mod_import("auth")],
                ..Default::default()
            },
        );
        // auth.rs declares a submodule `sub`
        parsed_files.insert(
            auth_rs.clone(),
            ParseResult {
                imports: vec![mod_import("sub")],
                ..Default::default()
            },
        );
        parsed_files.insert(sub_rs.clone(), ParseResult::default());

        let context = make_context(
            root.clone(),
            vec![lib_rs.clone(), auth_rs.clone(), sub_rs.clone()],
            parsed_files,
        );

        // auth.rs uses `use self::sub::something`
        let resolver = RustResolver::new(RustConfig { workspace_members: vec![], edition: None });
        let parse_result = ParseResult {
            imports: vec![use_import("self::sub::something")],
            ..Default::default()
        };

        let edges = resolver.resolve(&auth_rs, &parse_result, &context).unwrap();

        assert_eq!(
            edges.len(),
            1,
            "Expected one ImportsFrom edge, got: {edges:?}"
        );
        assert_eq!(edges[0].kind, EdgeKind::ImportsFrom);
        assert_eq!(edges[0].source, auth_rs.to_string_lossy());
        assert_eq!(edges[0].target, sub_rs.to_string_lossy());
    }

    // -----------------------------------------------------------------------
    // AC38: creates ReExport edge for pub use statements
    // -----------------------------------------------------------------------
    #[test]
    fn creates_reexport_edge_for_pub_use() {
        let root = PathBuf::from("/project");
        let lib_rs = root.join("src/lib.rs");
        let auth_rs = root.join("src/auth.rs");

        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            lib_rs.clone(),
            ParseResult {
                imports: vec![mod_import("auth")],
                ..Default::default()
            },
        );
        parsed_files.insert(auth_rs.clone(), ParseResult::default());

        let context = make_context(
            root.clone(),
            vec![lib_rs.clone(), auth_rs.clone()],
            parsed_files,
        );

        // lib.rs does `pub use crate::auth::validate;`
        let resolver = RustResolver::new(RustConfig { workspace_members: vec![], edition: None });
        let parse_result = ParseResult {
            imports: vec![use_import("crate::auth::validate")],
            exports: vec![pub_use_export("crate::auth::validate")],
            ..Default::default()
        };

        let edges = resolver.resolve(&lib_rs, &parse_result, &context).unwrap();

        assert_eq!(edges.len(), 1, "Expected one ReExport edge, got: {edges:?}");
        assert_eq!(edges[0].kind, EdgeKind::ReExport);
        assert_eq!(edges[0].source, lib_rs.to_string_lossy());
        assert_eq!(edges[0].target, auth_rs.to_string_lossy());
    }

    // -----------------------------------------------------------------------
    // AC39: handles both foo.rs and foo/mod.rs naming conventions
    // -----------------------------------------------------------------------
    #[test]
    fn handles_both_foo_rs_and_foo_mod_rs() {
        let root = PathBuf::from("/project");
        let lib_rs = root.join("src/lib.rs");
        // flat style: auth.rs
        let auth_rs = root.join("src/auth.rs");
        // legacy style: db/mod.rs
        let db_mod_rs = root.join("src/db/mod.rs");

        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            lib_rs.clone(),
            ParseResult {
                imports: vec![mod_import("auth"), mod_import("db")],
                ..Default::default()
            },
        );
        parsed_files.insert(auth_rs.clone(), ParseResult::default());
        parsed_files.insert(db_mod_rs.clone(), ParseResult::default());

        let context = make_context(
            root.clone(),
            vec![lib_rs.clone(), auth_rs.clone(), db_mod_rs.clone()],
            parsed_files,
        );

        let tree = build_module_tree(&context);

        // Flat style: crate::auth → src/auth.rs
        assert!(
            tree.contains_key("crate::auth"),
            "expected crate::auth in tree, got: {tree:?}"
        );
        assert_eq!(tree["crate::auth"], auth_rs);

        // Legacy style: crate::db → src/db/mod.rs
        assert!(
            tree.contains_key("crate::db"),
            "expected crate::db in tree, got: {tree:?}"
        );
        assert_eq!(tree["crate::db"], db_mod_rs);
    }

    // -----------------------------------------------------------------------
    // mod declarations are not emitted as edges
    // -----------------------------------------------------------------------
    #[test]
    fn mod_declarations_do_not_produce_edges() {
        let root = PathBuf::from("/project");
        let lib_rs = root.join("src/lib.rs");

        let context = make_context(root.clone(), vec![lib_rs.clone()], HashMap::new());

        let resolver = RustResolver::new(RustConfig { workspace_members: vec![], edition: None });
        let parse_result = ParseResult {
            imports: vec![mod_import("auth"), mod_import("db")],
            ..Default::default()
        };

        let edges = resolver.resolve(&lib_rs, &parse_result, &context).unwrap();
        assert!(edges.is_empty(), "mod declarations must not produce edges");
    }

    // -----------------------------------------------------------------------
    // Unresolvable specifiers are silently skipped
    // -----------------------------------------------------------------------
    #[test]
    fn unresolvable_specifier_is_skipped() {
        let root = PathBuf::from("/project");
        let lib_rs = root.join("src/lib.rs");

        let context = make_context(root.clone(), vec![lib_rs.clone()], HashMap::new());

        let resolver = RustResolver::new(RustConfig { workspace_members: vec![], edition: None });
        let parse_result = ParseResult {
            // std paths and external crates are not resolvable by this resolver
            imports: vec![use_import("std::fmt"), use_import("serde::Serialize")],
            ..Default::default()
        };

        let edges = resolver.resolve(&lib_rs, &parse_result, &context).unwrap();
        assert!(
            edges.is_empty(),
            "unresolvable imports must not produce edges"
        );
    }

    // -----------------------------------------------------------------------
    // Recursive module tree from nested mod declarations
    // -----------------------------------------------------------------------
    #[test]
    fn builds_nested_module_tree() {
        let root = PathBuf::from("/project");
        let lib_rs = root.join("src/lib.rs");
        let auth_rs = root.join("src/auth.rs");
        let validate_rs = root.join("src/auth/validate.rs");

        let mut parsed_files = HashMap::new();
        // lib.rs declares mod auth
        parsed_files.insert(
            lib_rs.clone(),
            ParseResult {
                imports: vec![mod_import("auth")],
                ..Default::default()
            },
        );
        // auth.rs declares mod validate
        parsed_files.insert(
            auth_rs.clone(),
            ParseResult {
                imports: vec![mod_import("validate")],
                ..Default::default()
            },
        );
        parsed_files.insert(validate_rs.clone(), ParseResult::default());

        let context = make_context(
            root.clone(),
            vec![lib_rs.clone(), auth_rs.clone(), validate_rs.clone()],
            parsed_files,
        );

        let tree = build_module_tree(&context);

        assert_eq!(tree.get("crate::auth"), Some(&auth_rs));
        assert_eq!(tree.get("crate::auth::validate"), Some(&validate_rs));
    }

    // -----------------------------------------------------------------------
    // use crate::auth (direct module reference, not a subitem)
    // -----------------------------------------------------------------------
    #[test]
    fn resolves_direct_module_reference() {
        let root = PathBuf::from("/project");
        let lib_rs = root.join("src/lib.rs");
        let auth_rs = root.join("src/auth.rs");
        let main_rs = root.join("src/main.rs");

        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            lib_rs.clone(),
            ParseResult {
                imports: vec![mod_import("auth")],
                ..Default::default()
            },
        );
        parsed_files.insert(auth_rs.clone(), ParseResult::default());

        let context = make_context(
            root.clone(),
            vec![lib_rs.clone(), auth_rs.clone()],
            parsed_files,
        );

        let resolver = RustResolver::new(RustConfig { workspace_members: vec![], edition: None });
        let parse_result = ParseResult {
            imports: vec![use_import("crate::auth")],
            ..Default::default()
        };

        let edges = resolver.resolve(&main_rs, &parse_result, &context).unwrap();

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::ImportsFrom);
        assert_eq!(edges[0].target, auth_rs.to_string_lossy());
    }

    // -----------------------------------------------------------------------
    // super:: resolution
    // -----------------------------------------------------------------------
    #[test]
    fn resolves_super_path() {
        let root = PathBuf::from("/project");
        let lib_rs = root.join("src/lib.rs");
        let auth_rs = root.join("src/auth.rs");
        let validate_rs = root.join("src/auth/validate.rs");

        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            lib_rs.clone(),
            ParseResult {
                imports: vec![mod_import("auth")],
                ..Default::default()
            },
        );
        parsed_files.insert(
            auth_rs.clone(),
            ParseResult {
                imports: vec![mod_import("validate")],
                ..Default::default()
            },
        );
        parsed_files.insert(validate_rs.clone(), ParseResult::default());

        let context = make_context(
            root.clone(),
            vec![lib_rs.clone(), auth_rs.clone(), validate_rs.clone()],
            parsed_files,
        );

        // validate.rs does `use super::something` — resolves to crate::auth → auth.rs
        let resolver = RustResolver::new(RustConfig { workspace_members: vec![], edition: None });
        let parse_result = ParseResult {
            imports: vec![use_import("super::something")],
            ..Default::default()
        };

        // Note: super::something resolves to parent path "crate::auth", which maps to auth_rs.
        // "something" is just a name in auth module, not a submodule, so no deeper resolution.
        // But "crate::auth" IS in the module tree.
        let edges = resolver
            .resolve(&validate_rs, &parse_result, &context)
            .unwrap();

        assert_eq!(
            edges.len(),
            1,
            "Expected ImportsFrom edge via super, got: {edges:?}"
        );
        assert_eq!(edges[0].kind, EdgeKind::ImportsFrom);
        assert_eq!(edges[0].source, validate_rs.to_string_lossy());
        assert_eq!(edges[0].target, auth_rs.to_string_lossy());
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn rust_config_parses_workspace_members() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), r#"
[workspace]
members = ["crates/foo", "crates/bar"]

[package]
edition = "2021"
"#).unwrap();
        let config = RustConfig::load(dir.path());
        assert_eq!(config.workspace_members, vec!["crates/foo", "crates/bar"]);
        assert_eq!(config.edition.as_deref(), Some("2021"));
    }

    #[test]
    fn rust_config_empty_without_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), r#"
[package]
name = "solo"
edition = "2021"
"#).unwrap();
        let config = RustConfig::load(dir.path());
        assert!(config.workspace_members.is_empty());
    }

    #[test]
    fn rust_config_empty_without_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        let config = RustConfig::load(dir.path());
        assert!(config.workspace_members.is_empty());
        assert!(config.edition.is_none());
    }
}
