use std::path::Path;

use domain::model::SymbolNode;

use crate::{
    Export, GoParser, JavaScriptParser, LanguageParser, ParseResult, PythonParser, RawImport,
    RustParser, TypeScriptParser,
};

/// Parse TypeScript source and return the result.
pub fn parse_ts(source: &str) -> ParseResult {
    let parser = TypeScriptParser::new();
    parser
        .parse(source.as_bytes(), Path::new("test.ts"))
        .expect("parse failed")
}

/// Parse JavaScript source and return the result.
pub fn parse_js(source: &str) -> ParseResult {
    let parser = JavaScriptParser::new();
    parser
        .parse(source.as_bytes(), Path::new("test.js"))
        .expect("parse failed")
}

/// Parse Rust source and return the result.
pub fn parse_rust(source: &str) -> ParseResult {
    let parser = RustParser::new();
    parser
        .parse(source.as_bytes(), Path::new("test.rs"))
        .expect("parse failed")
}

/// Parse Python source and return the result.
pub fn parse_python(source: &str) -> ParseResult {
    let parser = PythonParser::new();
    parser
        .parse(source.as_bytes(), Path::new("test.py"))
        .expect("parse failed")
}

/// Parse Go source and return the result.
pub fn parse_go(source: &str) -> ParseResult {
    let parser = GoParser::new();
    parser
        .parse(source.as_bytes(), Path::new("test.go"))
        .expect("parse failed")
}

/// Find a symbol by name in a parse result.
pub fn find_symbol<'a>(result: &'a ParseResult, name: &str) -> &'a SymbolNode {
    result
        .symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("symbol '{name}' not found"))
}

/// Find an import by specifier in a parse result.
pub fn find_import<'a>(result: &'a ParseResult, specifier: &str) -> &'a RawImport {
    result
        .imports
        .iter()
        .find(|i| i.specifier == specifier)
        .unwrap_or_else(|| panic!("import '{specifier}' not found"))
}

/// Find an export by name in a parse result.
pub fn find_export<'a>(result: &'a ParseResult, name: &str) -> &'a Export {
    result
        .exports
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("export '{name}' not found"))
}
