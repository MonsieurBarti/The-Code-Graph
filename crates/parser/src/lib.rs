#![forbid(unsafe_code)]

mod go;
mod python;
mod registry;
pub mod resolver;
mod rust_lang;
mod typescript;

#[cfg(test)]
pub mod test_utils;

pub use go::GoParser;
pub use python::PythonParser;
pub use registry::ParserRegistry;
pub use rust_lang::RustParser;
pub use typescript::{JavaScriptParser, TypeScriptParser};

use domain::model::{Edge, SymbolNode};
use std::path::Path;

// ---------------------------------------------------------------------------
// Core parser types
// ---------------------------------------------------------------------------

/// Output of parsing a single source file.
/// Phase 1 of two-phase "parse then resolve" — imports are unresolved.
#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub symbols: Vec<SymbolNode>,
    pub edges: Vec<Edge>,
    pub imports: Vec<RawImport>,
    pub exports: Vec<Export>,
}

/// An unresolved import statement extracted from source code.
/// Converted to resolved edges during the resolution phase (S04).
#[derive(Debug, Clone, Default)]
pub struct RawImport {
    pub specifier: String,
    pub names: Vec<ImportName>,
    pub is_type_only: bool,
    pub is_side_effect: bool,
    pub is_namespace: bool,
    pub line: usize,
}

/// A single named import within an import statement.
#[derive(Debug, Clone)]
pub struct ImportName {
    pub name: String,
    pub alias: Option<String>,
    pub is_type: bool,
}

/// An export declaration extracted from source code.
/// Used during resolution (S04) to build ImportsFrom and ReExport edges.
#[derive(Debug, Clone, Default)]
pub struct Export {
    pub name: String,
    pub local_name: Option<String>,
    pub is_default: bool,
    pub is_type_only: bool,
    pub is_reexport: bool,
    pub source_specifier: Option<String>,
}

// ---------------------------------------------------------------------------
// LanguageParser trait
// ---------------------------------------------------------------------------

/// Trait for language-specific tree-sitter parsers.
/// Implementations must be Send + Sync so the registry can be shared across threads.
pub trait LanguageParser: Send + Sync {
    /// Which domain Language this parser handles.
    fn language(&self) -> domain::model::Language;

    /// File extensions this parser handles (without leading dot).
    fn file_extensions(&self) -> &[&str];

    /// Parse source code and extract symbols, edges, imports, exports.
    /// `path` is the project-relative file path (used for qualified names).
    fn parse(&self, source: &[u8], path: &Path) -> domain::error::Result<ParseResult>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_result_default_is_empty() {
        let result = ParseResult::default();
        assert!(result.symbols.is_empty());
        assert!(result.edges.is_empty());
        assert!(result.imports.is_empty());
        assert!(result.exports.is_empty());
    }

    #[test]
    fn raw_import_default_values() {
        let imp = RawImport::default();
        assert!(imp.specifier.is_empty());
        assert!(imp.names.is_empty());
        assert!(!imp.is_type_only);
        assert!(!imp.is_side_effect);
        assert!(!imp.is_namespace);
        assert_eq!(imp.line, 0);
    }

    #[test]
    fn export_default_values() {
        let exp = Export::default();
        assert!(exp.name.is_empty());
        assert!(exp.local_name.is_none());
        assert!(!exp.is_default);
        assert!(!exp.is_type_only);
        assert!(!exp.is_reexport);
        assert!(exp.source_specifier.is_none());
    }

    #[test]
    fn import_name_construction() {
        let name = ImportName {
            name: "foo".into(),
            alias: Some("bar".into()),
            is_type: true,
        };
        assert_eq!(name.name, "foo");
        assert_eq!(name.alias.as_deref(), Some("bar"));
        assert!(name.is_type);
    }

    #[test]
    fn parse_result_can_hold_symbols_and_edges() {
        use domain::model::*;

        let sym = SymbolNode {
            name: "foo".into(),
            qualified_name: "test.ts::foo".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "test.ts".into(),
                line_start: 1,
                line_end: 3,
                col_start: 0,
                col_end: 1,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        };

        let edge = Edge {
            kind: EdgeKind::Contains,
            source: "test.ts".into(),
            target: "test.ts::foo".into(),
            metadata: None,
        };

        let result = ParseResult {
            symbols: vec![sym],
            edges: vec![edge],
            imports: vec![],
            exports: vec![],
        };
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.edges.len(), 1);
    }
}
