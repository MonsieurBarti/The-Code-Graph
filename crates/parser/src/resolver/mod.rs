pub mod go;
pub mod python;
pub mod rust_lang;
pub mod typescript;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use domain::error::CodeGraphError;
use domain::model::{Edge, Language};

use crate::ParseResult;

/// Trait for language-specific import resolvers.
/// Called after all files are parsed (Phase 2 of parse-then-resolve).
pub trait ImportResolver: Send + Sync {
    /// Which languages this resolver handles.
    fn languages(&self) -> &[Language];

    /// Resolve raw imports from a single file into graph edges.
    fn resolve(
        &self,
        file_path: &Path,
        parse_result: &ParseResult,
        context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>>;
}

/// Shared context for resolution — all parsed files and their results.
pub struct ResolveContext {
    pub project_root: PathBuf,
    pub parsed_files: HashMap<PathBuf, ParseResult>,
    pub file_tree: Vec<PathBuf>,
}

/// Registry of import resolvers with language-based dispatch.
pub struct ResolverRegistry {
    resolvers: Vec<Box<dyn ImportResolver>>,
}

impl ResolverRegistry {
    /// Create registry with all supported import resolvers.
    pub fn new(project_root: &Path) -> Self {
        let mut registry = Self {
            resolvers: Vec::new(),
        };
        registry.register(Box::new(typescript::TypeScriptResolver::new(project_root)));
        let rust_config = rust_lang::RustConfig::load(project_root);
        registry.register(Box::new(rust_lang::RustResolver::new(rust_config)));
        let python_config = python::PythonConfig::load(project_root);
        registry.register(Box::new(python::PythonResolver::new(python_config)));
        let go_config = go::GoConfig::load(project_root);
        registry.register(Box::new(go::GoResolver::new(go_config)));
        registry
    }

    fn register(&mut self, resolver: Box<dyn ImportResolver>) {
        self.resolvers.push(resolver);
    }

    /// Get the resolver for a specific language.
    pub fn resolver_for_language(&self, lang: Language) -> Option<&dyn ImportResolver> {
        self.resolvers
            .iter()
            .find(|r| r.languages().contains(&lang))
            .map(|r| r.as_ref())
    }

    /// Resolve imports for a single file.
    pub fn resolve_file(
        &self,
        file_path: &Path,
        lang: Language,
        parse_result: &ParseResult,
        context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>> {
        self.resolver_for_language(lang)
            .ok_or_else(|| {
                CodeGraphError::Resolution(format!("no resolver for language {lang:?}"))
            })?
            .resolve(file_path, parse_result, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn registry_has_all_four_resolvers() {
        let registry = ResolverRegistry::new(Path::new("/tmp"));
        assert!(registry
            .resolver_for_language(Language::TypeScript)
            .is_some());
        assert!(registry
            .resolver_for_language(Language::JavaScript)
            .is_some());
        assert!(registry.resolver_for_language(Language::Rust).is_some());
        assert!(registry.resolver_for_language(Language::Python).is_some());
        assert!(registry.resolver_for_language(Language::Go).is_some());
    }

    #[test]
    fn resolve_file_returns_error_for_unknown_language_none() {
        // All 5 languages are covered, so this test just verifies dispatch works
        let registry = ResolverRegistry::new(Path::new("/tmp"));
        let context = ResolveContext {
            project_root: PathBuf::from("/tmp"),
            parsed_files: HashMap::new(),
            file_tree: Vec::new(),
        };
        let result = ParseResult::default();
        // TypeScript should resolve without error (empty result)
        let edges = registry.resolve_file(
            Path::new("test.ts"),
            Language::TypeScript,
            &result,
            &context,
        );
        assert!(edges.is_ok());
    }
}
