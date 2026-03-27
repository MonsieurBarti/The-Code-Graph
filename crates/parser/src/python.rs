use std::cell::RefCell;
use std::path::Path;

use tree_sitter::{Node, Parser};
use tree_sitter_language::LanguageFn;

use domain::error::CodeGraphError;
use domain::model::{Edge, EdgeKind, Language, Location, SymbolKind, SymbolNode, Visibility};

use crate::{ImportName, LanguageParser, ParseResult, RawImport};

thread_local! {
    static PY_PARSER: RefCell<Parser> = RefCell::new(Parser::new());
}

/// Parser for Python (.py) files.
pub struct PythonParser {
    lang: LanguageFn,
}

impl PythonParser {
    pub fn new() -> Self {
        Self {
            lang: tree_sitter_python::LANGUAGE,
        }
    }
}

impl Default for PythonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for PythonParser {
    fn language(&self) -> Language {
        Language::Python
    }

    fn file_extensions(&self) -> &[&str] {
        &["py"]
    }

    fn parse(&self, source: &[u8], path: &Path) -> domain::error::Result<ParseResult> {
        let lang: tree_sitter::Language = self.lang.into();

        PY_PARSER.with(|parser_cell| {
            let mut parser = parser_cell.borrow_mut();
            parser
                .set_language(&lang)
                .map_err(|e| CodeGraphError::Parse {
                    file: path.to_path_buf(),
                    message: format!("failed to set language: {e}"),
                })?;

            let tree = parser
                .parse(source, None)
                .ok_or_else(|| CodeGraphError::Parse {
                    file: path.to_path_buf(),
                    message: "tree-sitter parse returned None".into(),
                })?;

            extract_all(source, path, &tree)
        })
    }
}

// ---------------------------------------------------------------------------
// Main extraction
// ---------------------------------------------------------------------------

fn extract_all(
    source: &[u8],
    path: &Path,
    tree: &tree_sitter::Tree,
) -> domain::error::Result<ParseResult> {
    let mut symbols = Vec::new();
    let mut edges = Vec::new();
    let file_path = path.to_string_lossy().to_string();
    let root = tree.root_node();

    // Collect imports, handling TYPE_CHECKING blocks
    let imports = extract_imports_from_root(&root, source);

    // Walk top-level statements
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        extract_top_level(source, &file_path, child, &mut symbols, &mut edges);
    }

    Ok(ParseResult {
        symbols,
        edges,
        imports,
        exports: Vec::new(), // Python has no explicit export declarations
    })
}

// ---------------------------------------------------------------------------
// Top-level statement dispatch
// ---------------------------------------------------------------------------

fn extract_top_level(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    match node.kind() {
        "function_definition" => {
            if let Some(sym) = extract_function(source, file_path, node, &[]) {
                edges.push(contains_edge(file_path, &sym.qualified_name));
                symbols.push(sym);
            }
        }
        "class_definition" => {
            extract_class(source, file_path, node, &[], symbols, edges);
        }
        "decorated_definition" => {
            extract_decorated(source, file_path, node, symbols, edges);
        }
        "expression_statement" => {
            extract_assignment(source, file_path, node, symbols, edges);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Function extraction
// ---------------------------------------------------------------------------

fn extract_function(
    source: &[u8],
    file_path: &str,
    node: Node,
    decorators: &[String],
) -> Option<SymbolNode> {
    let name = node_text_field(node, "name", source)?;
    let qualified_name = format!("{file_path}::{name}");
    let is_async = has_async_keyword(node, source);
    let signature = build_py_signature(source, node);
    let visibility = python_visibility(&name);

    Some(SymbolNode {
        name: name.clone(),
        qualified_name,
        kind: SymbolKind::Function,
        location: node_location(file_path, node),
        visibility,
        is_exported: matches!(visibility, Visibility::Public),
        is_async,
        is_test: is_test_name(&name),
        decorators: decorators.to_vec(),
        signature,
    })
}

// ---------------------------------------------------------------------------
// Class extraction
// ---------------------------------------------------------------------------

fn extract_class(
    source: &[u8],
    file_path: &str,
    node: Node,
    decorators: &[String],
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let name = match node_text_field(node, "name", source) {
        Some(n) => n,
        None => return,
    };
    let qualified_name = format!("{file_path}::{name}");
    let visibility = python_visibility(&name);

    let class_sym = SymbolNode {
        name: name.clone(),
        qualified_name: qualified_name.clone(),
        kind: SymbolKind::Class,
        location: node_location(file_path, node),
        visibility,
        is_exported: matches!(visibility, Visibility::Public),
        is_async: false,
        is_test: is_test_name(&name),
        decorators: decorators.to_vec(),
        signature: None,
    };
    edges.push(contains_edge(file_path, &class_sym.qualified_name));
    symbols.push(class_sym);

    // Extends edges: class Foo(Bar, Baz):
    extract_extends_edges(source, file_path, node, &name, &qualified_name, edges);

    // Methods and properties inside the class body
    if let Some(body) = node.child_by_field_name("body") {
        extract_class_body(source, file_path, &name, &qualified_name, body, symbols, edges);
    }
}

fn extract_extends_edges(
    source: &[u8],
    file_path: &str,
    class_node: Node,
    _class_name: &str,
    class_qualified_name: &str,
    edges: &mut Vec<Edge>,
) {
    // Python tree-sitter: class arguments are in "argument_list" or "superclasses"
    // The field name in tree-sitter-python is "superclasses" → argument_list
    let superclasses_node = class_node.child_by_field_name("superclasses");
    let target_node = superclasses_node.or_else(|| {
        // Also try "argument_list" as direct child
        let mut cursor = class_node.walk();
        let found = class_node
            .children(&mut cursor)
            .find(|c| c.kind() == "argument_list");
        found
    });

    if let Some(args) = target_node {
        let mut cursor = args.walk();
        for arg in args.children(&mut cursor) {
            if !arg.is_named() {
                continue;
            }
            // Simple identifier base class
            let base_name = match arg.kind() {
                "identifier" => arg.utf8_text(source).ok().map(|s| s.to_string()),
                "attribute" => arg.utf8_text(source).ok().map(|s| s.to_string()),
                _ => None,
            };
            if let Some(base) = base_name {
                // Only emit Extends for same-file resolution: use qualified name pattern
                let target = format!("{file_path}::{base}");
                edges.push(Edge {
                    kind: EdgeKind::Extends,
                    source: class_qualified_name.to_string(),
                    target,
                    metadata: None,
                });
            }
        }
    }
}

fn extract_class_body(
    source: &[u8],
    file_path: &str,
    class_name: &str,
    class_qualified_name: &str,
    body: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let mut cursor = body.walk();
    for stmt in body.children(&mut cursor) {
        if !stmt.is_named() {
            continue;
        }
        match stmt.kind() {
            "function_definition" => {
                extract_method(
                    source,
                    file_path,
                    class_name,
                    class_qualified_name,
                    stmt,
                    &[],
                    symbols,
                    edges,
                );
            }
            "decorated_definition" => {
                let decorators = collect_decorators(source, stmt);
                // Find the inner definition
                let inner = stmt
                    .children(&mut stmt.walk())
                    .find(|c| c.is_named() && matches!(c.kind(), "function_definition" | "class_definition"));
                if let Some(inner_node) = inner {
                    if inner_node.kind() == "function_definition" {
                        extract_method(
                            source,
                            file_path,
                            class_name,
                            class_qualified_name,
                            inner_node,
                            &decorators,
                            symbols,
                            edges,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_method(
    source: &[u8],
    file_path: &str,
    class_name: &str,
    class_qualified_name: &str,
    node: Node,
    decorators: &[String],
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let name = match node_text_field(node, "name", source) {
        Some(n) => n,
        None => return,
    };
    let member_qualified = format!("{file_path}::{class_name}.{name}");
    let is_async = has_async_keyword(node, source);
    let signature = build_py_signature(source, node);
    let visibility = python_visibility(&name);

    // Determine kind: @property → Property, else Method
    let kind = if decorators.iter().any(|d| d == "@property" || d == "property") {
        SymbolKind::Property
    } else {
        SymbolKind::Method
    };

    let sym = SymbolNode {
        name: name.clone(),
        qualified_name: member_qualified.clone(),
        kind,
        location: node_location(file_path, node),
        visibility,
        is_exported: matches!(visibility, Visibility::Public),
        is_async,
        is_test: is_test_name(&name),
        decorators: decorators.to_vec(),
        signature,
    };
    symbols.push(sym);
    edges.push(Edge {
        kind: EdgeKind::ChildOf,
        source: member_qualified,
        target: class_qualified_name.to_string(),
        metadata: None,
    });
}

// ---------------------------------------------------------------------------
// Decorated definition
// ---------------------------------------------------------------------------

fn extract_decorated(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let decorators = collect_decorators(source, node);

    // Find the inner function_definition or class_definition
    let inner = {
        let mut cursor = node.walk();
        let found = node.children(&mut cursor)
            .find(|c| c.is_named() && matches!(c.kind(), "function_definition" | "class_definition"));
        found
    };

    match inner {
        Some(inner_node) if inner_node.kind() == "function_definition" => {
            if let Some(sym) = extract_function(source, file_path, inner_node, &decorators) {
                edges.push(contains_edge(file_path, &sym.qualified_name));
                symbols.push(sym);
            }
        }
        Some(inner_node) if inner_node.kind() == "class_definition" => {
            extract_class(source, file_path, inner_node, &decorators, symbols, edges);
        }
        _ => {}
    }
}

fn collect_decorators(source: &[u8], node: Node) -> Vec<String> {
    let mut decorators = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() && child.kind() == "decorator" {
            if let Ok(text) = child.utf8_text(source) {
                // Strip the leading '@' for the decorator text
                let text = text.trim();
                decorators.push(text.to_string());
            }
        }
    }
    decorators
}

// ---------------------------------------------------------------------------
// Assignment (top-level variable)
// ---------------------------------------------------------------------------

fn extract_assignment(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    // expression_statement → assignment
    let assignment = {
        let mut cursor = node.walk();
        let found = node.children(&mut cursor)
            .find(|c| c.is_named() && c.kind() == "assignment");
        found
    };

    let assignment = match assignment {
        Some(a) => a,
        None => return,
    };

    // Get left-hand side identifier
    let lhs = match assignment.child_by_field_name("left") {
        Some(l) => l,
        None => return,
    };

    if lhs.kind() != "identifier" {
        return;
    }

    let name = match lhs.utf8_text(source).ok() {
        Some(n) => n.to_string(),
        None => return,
    };

    let qualified_name = format!("{file_path}::{name}");
    let visibility = python_visibility(&name);

    let sym = SymbolNode {
        name: name.clone(),
        qualified_name: qualified_name.clone(),
        kind: SymbolKind::Variable,
        location: node_location(file_path, node),
        visibility,
        is_exported: matches!(visibility, Visibility::Public),
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    };
    edges.push(contains_edge(file_path, &sym.qualified_name));
    symbols.push(sym);
}

// ---------------------------------------------------------------------------
// Import extraction
// ---------------------------------------------------------------------------

fn extract_imports_from_root(root: &Node, source: &[u8]) -> Vec<RawImport> {
    let mut imports = Vec::new();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "import_statement" => {
                imports.extend(parse_import_statement(&child, source, false));
            }
            "import_from_statement" => {
                if let Some(imp) = parse_import_from_statement(&child, source, false) {
                    imports.push(imp);
                }
            }
            "if_statement" => {
                // Detect TYPE_CHECKING blocks
                if is_type_checking_guard(&child, source) {
                    let consequence = child.child_by_field_name("consequence");
                    if let Some(block) = consequence {
                        let mut block_cursor = block.walk();
                        for stmt in block.children(&mut block_cursor) {
                            if !stmt.is_named() {
                                continue;
                            }
                            match stmt.kind() {
                                "import_statement" => {
                                    imports.extend(parse_import_statement(&stmt, source, true));
                                }
                                "import_from_statement" => {
                                    if let Some(imp) =
                                        parse_import_from_statement(&stmt, source, true)
                                    {
                                        imports.push(imp);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    imports
}

/// Detect `if TYPE_CHECKING:` or `if typing.TYPE_CHECKING:`
fn is_type_checking_guard(node: &Node, source: &[u8]) -> bool {
    let condition = match node.child_by_field_name("condition") {
        Some(c) => c,
        None => return false,
    };
    match condition.kind() {
        "identifier" => condition.utf8_text(source).ok() == Some("TYPE_CHECKING"),
        "attribute" => {
            // typing.TYPE_CHECKING → attribute node with attribute field "TYPE_CHECKING"
            condition
                .child_by_field_name("attribute")
                .and_then(|a| a.utf8_text(source).ok())
                == Some("TYPE_CHECKING")
        }
        _ => false,
    }
}

/// Parse `import os`, `import os.path`, `import os as o`, `import a, b`
fn parse_import_statement(
    node: &Node,
    source: &[u8],
    is_type_only: bool,
) -> Vec<RawImport> {
    let line = node.start_position().row + 1;
    let mut imports = Vec::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "dotted_name" => {
                let specifier = child.utf8_text(source).unwrap_or("").to_string();
                imports.push(RawImport {
                    specifier: specifier.clone(),
                    names: vec![ImportName {
                        name: specifier,
                        alias: None,
                        is_type: false,
                    }],
                    is_type_only,
                    is_side_effect: false,
                    is_namespace: false,
                    line,
                });
            }
            "aliased_import" => {
                // `import os as o` → aliased_import with name=dotted_name, alias=identifier
                let specifier = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                    .unwrap_or("")
                    .to_string();
                let alias = child
                    .child_by_field_name("alias")
                    .and_then(|a| a.utf8_text(source).ok())
                    .map(|s| s.to_string());
                imports.push(RawImport {
                    specifier: specifier.clone(),
                    names: vec![ImportName {
                        name: specifier,
                        alias,
                        is_type: false,
                    }],
                    is_type_only,
                    is_side_effect: false,
                    is_namespace: false,
                    line,
                });
            }
            _ => {}
        }
    }

    imports
}

/// Parse `from X import Y`, `from . import Y`, `from .X import Y`, `from X import *`
fn parse_import_from_statement(
    node: &Node,
    source: &[u8],
    is_type_only: bool,
) -> Option<RawImport> {
    let line = node.start_position().row + 1;

    // Build specifier from module_name field
    let specifier = build_from_specifier(node, source);

    // Collect names
    let mut names = Vec::new();
    let mut is_namespace = false;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "wildcard_import" => {
                is_namespace = true;
                names.push(ImportName {
                    name: "*".to_string(),
                    alias: None,
                    is_type: false,
                });
            }
            "dotted_name" => {
                // This is a bare import name (not the module part)
                // Skip if it's the module_name field — only process non-field children
                // In tree-sitter-python, "from foo import bar" has module_name="foo" (field)
                // and children include the import names as non-field "dotted_name" nodes
                // We need to skip the module_name field child
                let is_module_name = node
                    .child_by_field_name("module_name")
                    .map(|mn| mn.id() == child.id())
                    .unwrap_or(false);
                if !is_module_name {
                    let name = child.utf8_text(source).unwrap_or("").to_string();
                    names.push(ImportName {
                        name,
                        alias: None,
                        is_type: false,
                    });
                }
            }
            "aliased_import" => {
                let name = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                    .unwrap_or("")
                    .to_string();
                let alias = child
                    .child_by_field_name("alias")
                    .and_then(|a| a.utf8_text(source).ok())
                    .map(|s| s.to_string());
                names.push(ImportName {
                    name,
                    alias,
                    is_type: false,
                });
            }
            "identifier" => {
                // Plain identifier as import name
                // Skip if it matches the module_name field
                let is_module_name = node
                    .child_by_field_name("module_name")
                    .map(|mn| mn.id() == child.id())
                    .unwrap_or(false);
                if !is_module_name {
                    let name = child.utf8_text(source).unwrap_or("").to_string();
                    names.push(ImportName {
                        name,
                        alias: None,
                        is_type: false,
                    });
                }
            }
            _ => {}
        }
    }

    Some(RawImport {
        specifier,
        names,
        is_type_only,
        is_side_effect: false,
        is_namespace,
        line,
    })
}

/// Build the specifier string from a `from X import` statement.
/// Handles:
/// - `from os.path import join` → "os.path"
/// - `from . import models` → "."
/// - `from .models import User` → ".models"
/// - `from .. import utils` → ".."
/// - `from ..utils import helper` → "..utils"
fn build_from_specifier(node: &Node, source: &[u8]) -> String {
    let module_name = node.child_by_field_name("module_name");

    match module_name {
        None => {
            // `from . import X` — no module_name field; look for relative_import or import_prefix
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() && child.kind() == "relative_import" {
                    return relative_import_specifier(&child, source);
                }
            }
            ".".to_string()
        }
        Some(mn) => {
            match mn.kind() {
                "relative_import" => relative_import_specifier(&mn, source),
                "dotted_name" => mn.utf8_text(source).unwrap_or("").to_string(),
                _ => mn.utf8_text(source).unwrap_or("").to_string(),
            }
        }
    }
}

/// Build specifier from a `relative_import` node.
/// relative_import = import_prefix + dotted_name?
fn relative_import_specifier(node: &Node, source: &[u8]) -> String {
    let mut dots = String::new();
    let mut module_part = String::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_prefix" => {
                dots = child.utf8_text(source).unwrap_or(".").to_string();
            }
            "dotted_name" => {
                module_part = child.utf8_text(source).unwrap_or("").to_string();
            }
            _ => {}
        }
    }

    if module_part.is_empty() {
        dots
    } else {
        format!("{dots}{module_part}")
    }
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

/// Get text of a named field on a node.
fn node_text_field<'a>(node: Node, field: &str, source: &'a [u8]) -> Option<String> {
    node.child_by_field_name(field)
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// Build a Location from a tree-sitter node.
fn node_location(file_path: &str, node: Node) -> Location {
    let start = node.start_position();
    let end = node.end_position();
    Location {
        file: file_path.into(),
        line_start: start.row + 1,
        line_end: end.row + 1,
        col_start: start.column,
        col_end: end.column,
    }
}

/// Check if a function_definition node is preceded by `async` keyword.
fn has_async_keyword(node: Node, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "async" {
            return true;
        }
        // Also handle as text
        if !child.is_named() {
            if let Ok(text) = child.utf8_text(source) {
                if text == "async" {
                    return true;
                }
            }
        }
    }
    false
}

/// Python visibility rules:
/// - `_prefix` → Private
/// - `__prefix` (without trailing `__`) → Private
/// - `__dunder__` → Public (dunder methods)
/// - no prefix → Public
fn python_visibility(name: &str) -> Visibility {
    if name.starts_with("__") && name.ends_with("__") && name.len() > 4 {
        // Dunder method: __init__, __str__, etc.
        Visibility::Public
    } else if name.starts_with("__") {
        // Name mangled: __private
        Visibility::Private
    } else if name.starts_with('_') {
        // Convention private: _helper
        Visibility::Private
    } else {
        Visibility::Public
    }
}

/// Build function signature from parameters.
fn build_py_signature(source: &[u8], node: Node) -> Option<String> {
    node.child_by_field_name("parameters")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// Create a Contains edge.
fn contains_edge(file_path: &str, qualified_name: &str) -> Edge {
    Edge {
        kind: EdgeKind::Contains,
        source: file_path.to_string(),
        target: qualified_name.to_string(),
        metadata: None,
    }
}

/// Check if a name looks like a test function.
fn is_test_name(name: &str) -> bool {
    name.starts_with("test_") || name.starts_with("Test") || name == "test"
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use domain::model::{EdgeKind, SymbolKind, Visibility};

    fn parse_python(source: &str) -> ParseResult {
        let parser = PythonParser::new();
        parser
            .parse(source.as_bytes(), Path::new("test.py"))
            .expect("parse failed")
    }

    // -----------------------------------------------------------------------
    // AC15: def foo(): → Function symbol
    // -----------------------------------------------------------------------

    #[test]
    fn ac15_function_definition() {
        let result = parse_python("def foo():\n    pass\n");
        let sym = result.symbols.iter().find(|s| s.name == "foo").unwrap();
        assert_eq!(sym.kind, SymbolKind::Function);
        assert_eq!(sym.qualified_name, "test.py::foo");
        assert!(!sym.is_async);
        assert_eq!(sym.visibility, Visibility::Public);
        assert!(sym.is_exported);
    }

    #[test]
    fn function_contains_edge() {
        let result = parse_python("def foo():\n    pass\n");
        let edge = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Contains && e.target == "test.py::foo")
            .unwrap();
        assert_eq!(edge.source, "test.py");
    }

    #[test]
    fn function_location_populated() {
        let result = parse_python("def foo():\n    pass\n");
        let sym = result.symbols.iter().find(|s| s.name == "foo").unwrap();
        assert_eq!(sym.location.file.to_string_lossy(), "test.py");
        assert_eq!(sym.location.line_start, 1);
    }

    // -----------------------------------------------------------------------
    // AC16: class Bar: with methods → Class + Method + ChildOf
    // -----------------------------------------------------------------------

    #[test]
    fn ac16_class_with_method() {
        let source = "class Bar:\n    def greet(self):\n        pass\n";
        let result = parse_python(source);

        let class_sym = result.symbols.iter().find(|s| s.name == "Bar").unwrap();
        assert_eq!(class_sym.kind, SymbolKind::Class);
        assert_eq!(class_sym.qualified_name, "test.py::Bar");

        let method_sym = result.symbols.iter().find(|s| s.name == "greet").unwrap();
        assert_eq!(method_sym.kind, SymbolKind::Method);
        assert_eq!(method_sym.qualified_name, "test.py::Bar.greet");

        let child_of = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::ChildOf)
            .unwrap();
        assert_eq!(child_of.source, "test.py::Bar.greet");
        assert_eq!(child_of.target, "test.py::Bar");
    }

    #[test]
    fn class_contains_edge() {
        let source = "class Foo:\n    pass\n";
        let result = parse_python(source);
        let edge = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Contains && e.target == "test.py::Foo")
            .unwrap();
        assert_eq!(edge.source, "test.py");
    }

    #[test]
    fn class_multiple_methods() {
        let source = "class Calc:\n    def add(self, a, b):\n        return a + b\n    def sub(self, a, b):\n        return a - b\n";
        let result = parse_python(source);

        assert!(result.symbols.iter().any(|s| s.name == "add" && s.kind == SymbolKind::Method));
        assert!(result.symbols.iter().any(|s| s.name == "sub" && s.kind == SymbolKind::Method));

        let child_of_count = result
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::ChildOf)
            .count();
        assert_eq!(child_of_count, 2);
    }

    // -----------------------------------------------------------------------
    // AC17: from .models import User → RawImport with specifier=".models"
    // -----------------------------------------------------------------------

    #[test]
    fn ac17_relative_import_with_module() {
        let result = parse_python("from .models import User\n");
        assert_eq!(result.imports.len(), 1);
        let imp = &result.imports[0];
        assert_eq!(imp.specifier, ".models");
        assert_eq!(imp.names.len(), 1);
        assert_eq!(imp.names[0].name, "User");
        assert!(!imp.is_type_only);
        assert!(!imp.is_namespace);
    }

    #[test]
    fn relative_import_double_dot() {
        let result = parse_python("from ..utils import helper\n");
        assert_eq!(result.imports.len(), 1);
        let imp = &result.imports[0];
        assert_eq!(imp.specifier, "..utils");
        assert_eq!(imp.names[0].name, "helper");
    }

    #[test]
    fn relative_import_dot_only() {
        let result = parse_python("from . import models\n");
        assert_eq!(result.imports.len(), 1);
        let imp = &result.imports[0];
        assert_eq!(imp.specifier, ".");
        assert_eq!(imp.names[0].name, "models");
    }

    #[test]
    fn relative_import_double_dot_only() {
        let result = parse_python("from .. import utils\n");
        assert_eq!(result.imports.len(), 1);
        let imp = &result.imports[0];
        assert_eq!(imp.specifier, "..");
        assert_eq!(imp.names[0].name, "utils");
    }

    // -----------------------------------------------------------------------
    // AC18: import os.path → RawImport with specifier="os.path"
    // -----------------------------------------------------------------------

    #[test]
    fn ac18_import_dotted_module() {
        let result = parse_python("import os.path\n");
        assert_eq!(result.imports.len(), 1);
        let imp = &result.imports[0];
        assert_eq!(imp.specifier, "os.path");
        assert_eq!(imp.names[0].name, "os.path");
    }

    #[test]
    fn import_simple_module() {
        let result = parse_python("import os\n");
        assert_eq!(result.imports.len(), 1);
        let imp = &result.imports[0];
        assert_eq!(imp.specifier, "os");
        assert_eq!(imp.names[0].name, "os");
    }

    #[test]
    fn import_with_alias() {
        let result = parse_python("import numpy as np\n");
        assert_eq!(result.imports.len(), 1);
        let imp = &result.imports[0];
        assert_eq!(imp.specifier, "numpy");
        assert_eq!(imp.names[0].alias, Some("np".to_string()));
    }

    #[test]
    fn from_import_multiple_names() {
        let result = parse_python("from os.path import join, exists\n");
        assert_eq!(result.imports.len(), 1);
        let imp = &result.imports[0];
        assert_eq!(imp.specifier, "os.path");
        assert_eq!(imp.names.len(), 2);
        assert!(imp.names.iter().any(|n| n.name == "join"));
        assert!(imp.names.iter().any(|n| n.name == "exists"));
    }

    #[test]
    fn from_import_wildcard() {
        let result = parse_python("from os import *\n");
        assert_eq!(result.imports.len(), 1);
        let imp = &result.imports[0];
        assert_eq!(imp.specifier, "os");
        assert!(imp.is_namespace);
    }

    #[test]
    fn from_import_with_alias() {
        let result = parse_python("from os.path import join as path_join\n");
        let imp = &result.imports[0];
        assert_eq!(imp.specifier, "os.path");
        assert_eq!(imp.names[0].name, "join");
        assert_eq!(imp.names[0].alias, Some("path_join".to_string()));
    }

    // -----------------------------------------------------------------------
    // AC19: async def foo(): → Function with is_async=true
    // -----------------------------------------------------------------------

    #[test]
    fn ac19_async_function() {
        let result = parse_python("async def fetch():\n    pass\n");
        let sym = result.symbols.iter().find(|s| s.name == "fetch").unwrap();
        assert_eq!(sym.kind, SymbolKind::Function);
        assert!(sym.is_async);
    }

    #[test]
    fn async_method() {
        let source = "class Client:\n    async def get(self):\n        pass\n";
        let result = parse_python(source);
        let sym = result.symbols.iter().find(|s| s.name == "get").unwrap();
        assert!(sym.is_async);
        assert_eq!(sym.kind, SymbolKind::Method);
    }

    // -----------------------------------------------------------------------
    // AC20: decorated functions → decorators field populated
    // -----------------------------------------------------------------------

    #[test]
    fn ac20_decorated_function() {
        let source = "@my_decorator\ndef foo():\n    pass\n";
        let result = parse_python(source);
        let sym = result.symbols.iter().find(|s| s.name == "foo").unwrap();
        assert_eq!(sym.kind, SymbolKind::Function);
        assert!(!sym.decorators.is_empty(), "decorators should be populated");
        assert!(sym.decorators.iter().any(|d| d.contains("my_decorator")));
    }

    #[test]
    fn decorated_class() {
        let source = "@dataclass\nclass Point:\n    pass\n";
        let result = parse_python(source);
        let sym = result.symbols.iter().find(|s| s.name == "Point").unwrap();
        assert_eq!(sym.kind, SymbolKind::Class);
        assert!(!sym.decorators.is_empty());
        assert!(sym.decorators.iter().any(|d| d.contains("dataclass")));
    }

    #[test]
    fn property_decorator_produces_property_kind() {
        let source = "class Foo:\n    @property\n    def name(self):\n        return self._name\n";
        let result = parse_python(source);
        let sym = result.symbols.iter().find(|s| s.name == "name").unwrap();
        assert_eq!(sym.kind, SymbolKind::Property);
    }

    #[test]
    fn multiple_decorators() {
        let source = "@decorator_one\n@decorator_two\ndef bar():\n    pass\n";
        let result = parse_python(source);
        let sym = result.symbols.iter().find(|s| s.name == "bar").unwrap();
        assert_eq!(sym.decorators.len(), 2);
    }

    // -----------------------------------------------------------------------
    // AC21: if TYPE_CHECKING: imports → is_type_only=true
    // -----------------------------------------------------------------------

    #[test]
    fn ac21_type_checking_guard() {
        let source = "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from .models import User\n";
        let result = parse_python(source);

        let type_guarded = result
            .imports
            .iter()
            .find(|i| i.specifier == ".models")
            .unwrap();
        assert!(type_guarded.is_type_only);
    }

    #[test]
    fn type_checking_attribute_form() {
        let source = "import typing\nif typing.TYPE_CHECKING:\n    from .types import MyType\n";
        let result = parse_python(source);

        let type_guarded = result
            .imports
            .iter()
            .find(|i| i.specifier == ".types")
            .unwrap();
        assert!(type_guarded.is_type_only);
    }

    #[test]
    fn regular_imports_not_type_only() {
        let result = parse_python("from os.path import join\n");
        assert!(!result.imports[0].is_type_only);
    }

    // -----------------------------------------------------------------------
    // AC49: Invalid/empty source → no panic
    // -----------------------------------------------------------------------

    #[test]
    fn ac49_empty_source_no_panic() {
        let result = parse_python("");
        assert!(result.symbols.is_empty());
        assert!(result.imports.is_empty());
        assert!(result.edges.is_empty());
    }

    #[test]
    fn ac49_invalid_source_no_panic() {
        // Should not panic even with malformed code
        let result = parse_python("def (\nclass {{{");
        // At minimum it should not panic — partial extraction may occur
        let _ = result;
    }

    // -----------------------------------------------------------------------
    // AC50: Source with errors → partial extraction
    // -----------------------------------------------------------------------

    #[test]
    fn ac50_partial_extraction_on_error() {
        let source = "def valid_function():\n    pass\n\ndef (\n\ndef another_valid():\n    pass\n";
        let result = parse_python(source);
        // Should extract at least valid_function
        assert!(
            result.symbols.iter().any(|s| s.name == "valid_function"),
            "should extract valid_function even with parse errors"
        );
    }

    // -----------------------------------------------------------------------
    // Visibility rules
    // -----------------------------------------------------------------------

    #[test]
    fn visibility_public_function() {
        let result = parse_python("def public_func():\n    pass\n");
        let sym = result.symbols.iter().find(|s| s.name == "public_func").unwrap();
        assert_eq!(sym.visibility, Visibility::Public);
        assert!(sym.is_exported);
    }

    #[test]
    fn visibility_private_single_underscore() {
        let result = parse_python("def _private():\n    pass\n");
        let sym = result.symbols.iter().find(|s| s.name == "_private").unwrap();
        assert_eq!(sym.visibility, Visibility::Private);
        assert!(!sym.is_exported);
    }

    #[test]
    fn visibility_private_double_underscore() {
        let result = parse_python("def __mangled():\n    pass\n");
        let sym = result.symbols.iter().find(|s| s.name == "__mangled").unwrap();
        assert_eq!(sym.visibility, Visibility::Private);
    }

    #[test]
    fn visibility_dunder_is_public() {
        let source = "class Foo:\n    def __init__(self):\n        pass\n";
        let result = parse_python(source);
        let sym = result.symbols.iter().find(|s| s.name == "__init__").unwrap();
        assert_eq!(sym.visibility, Visibility::Public);
    }

    // -----------------------------------------------------------------------
    // Extends edges
    // -----------------------------------------------------------------------

    #[test]
    fn class_extends_single_base() {
        let source = "class Animal:\n    pass\n\nclass Dog(Animal):\n    pass\n";
        let result = parse_python(source);

        let extends = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Extends)
            .unwrap();
        assert_eq!(extends.source, "test.py::Dog");
        assert_eq!(extends.target, "test.py::Animal");
    }

    #[test]
    fn class_no_base_no_extends_edge() {
        let source = "class Simple:\n    pass\n";
        let result = parse_python(source);
        assert!(!result.edges.iter().any(|e| e.kind == EdgeKind::Extends));
    }

    // -----------------------------------------------------------------------
    // Top-level variable assignment
    // -----------------------------------------------------------------------

    #[test]
    fn top_level_variable_assignment() {
        let result = parse_python("x = 42\n");
        assert!(result.symbols.iter().any(|s| s.name == "x" && s.kind == SymbolKind::Variable));
    }

    // -----------------------------------------------------------------------
    // Import line numbers
    // -----------------------------------------------------------------------

    #[test]
    fn import_line_number() {
        let result = parse_python("import os\n");
        assert_eq!(result.imports[0].line, 1);
    }

    #[test]
    fn import_line_number_second_line() {
        let result = parse_python("\nimport sys\n");
        assert_eq!(result.imports[0].line, 2);
    }

    // -----------------------------------------------------------------------
    // Parser metadata
    // -----------------------------------------------------------------------

    #[test]
    fn language_returns_python() {
        let parser = PythonParser::new();
        assert_eq!(parser.language(), Language::Python);
    }

    #[test]
    fn file_extensions_includes_py() {
        let parser = PythonParser::new();
        assert!(parser.file_extensions().contains(&"py"));
    }

    // -----------------------------------------------------------------------
    // Integration test
    // -----------------------------------------------------------------------

    #[test]
    fn integration_multi_construct_file() {
        let source = r#"
import os
import numpy as np
from os.path import join, exists
from .models import User
from ..utils import helper

from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from .types import MyType

def standalone_func(x, y):
    return x + y

async def async_handler():
    pass

@staticmethod
def decorated_func():
    pass

class BaseModel:
    pass

class MyModel(BaseModel):
    def __init__(self):
        pass

    @property
    def name(self):
        return self._name

    async def save(self):
        pass

    def _private_method(self):
        pass

x = 42
"#;
        let result = parse_python(source);

        // Symbols
        assert!(result.symbols.iter().any(|s| s.name == "standalone_func" && s.kind == SymbolKind::Function));
        assert!(result.symbols.iter().any(|s| s.name == "async_handler" && s.kind == SymbolKind::Function && s.is_async));
        assert!(result.symbols.iter().any(|s| s.name == "decorated_func" && s.kind == SymbolKind::Function));
        assert!(result.symbols.iter().any(|s| s.name == "BaseModel" && s.kind == SymbolKind::Class));
        assert!(result.symbols.iter().any(|s| s.name == "MyModel" && s.kind == SymbolKind::Class));
        assert!(result.symbols.iter().any(|s| s.name == "__init__" && s.kind == SymbolKind::Method));
        assert!(result.symbols.iter().any(|s| s.name == "name" && s.kind == SymbolKind::Property));
        assert!(result.symbols.iter().any(|s| s.name == "save" && s.kind == SymbolKind::Method && s.is_async));
        assert!(result.symbols.iter().any(|s| s.name == "_private_method" && s.visibility == Visibility::Private));
        assert!(result.symbols.iter().any(|s| s.name == "x" && s.kind == SymbolKind::Variable));

        // Edges
        let contains_count = result.edges.iter().filter(|e| e.kind == EdgeKind::Contains).count();
        assert!(contains_count >= 5, "expected >= 5 Contains edges, got {contains_count}");

        let child_of_count = result.edges.iter().filter(|e| e.kind == EdgeKind::ChildOf).count();
        assert!(child_of_count >= 3, "expected >= 3 ChildOf edges, got {child_of_count}");

        let extends_count = result.edges.iter().filter(|e| e.kind == EdgeKind::Extends).count();
        assert_eq!(extends_count, 1, "expected 1 Extends edge");

        // Imports
        assert!(result.imports.iter().any(|i| i.specifier == "os"));
        assert!(result.imports.iter().any(|i| i.specifier == "numpy" && i.names[0].alias == Some("np".to_string())));
        assert!(result.imports.iter().any(|i| i.specifier == "os.path"));
        assert!(result.imports.iter().any(|i| i.specifier == ".models"));
        assert!(result.imports.iter().any(|i| i.specifier == "..utils"));
        assert!(result.imports.iter().any(|i| i.specifier == ".types" && i.is_type_only));
    }
}
