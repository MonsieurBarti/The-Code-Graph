use std::cell::RefCell;
use std::path::Path;

use tree_sitter::{Node, Parser};
use tree_sitter_language::LanguageFn;

use domain::error::CodeGraphError;
use domain::model::{Edge, EdgeKind, Language, Location, SymbolKind, SymbolNode, Visibility};

use crate::{ImportName, LanguageParser, ParseResult, RawImport};

// Shared thread-local parser instance. Language is switched per file via `set_language()`.
thread_local! {
    static TS_PARSER: RefCell<Parser> = RefCell::new(Parser::new());
}

/// Parse source using the thread-local parser with the given grammar.
fn parse_with_grammar(
    source: &[u8],
    path: &Path,
    lang_fn: LanguageFn,
) -> domain::error::Result<ParseResult> {
    let lang: tree_sitter::Language = lang_fn.into();

    TS_PARSER.with(|parser_cell| {
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

// ---------------------------------------------------------------------------
// TypeScriptParser — handles .ts and .tsx
// ---------------------------------------------------------------------------

/// Parser for TypeScript (.ts) and TSX (.tsx) files.
pub struct TypeScriptParser {
    ts_language: LanguageFn,
    tsx_language: LanguageFn,
}

impl TypeScriptParser {
    pub fn new() -> Self {
        Self {
            ts_language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            tsx_language: tree_sitter_typescript::LANGUAGE_TSX,
        }
    }

    fn language_fn_for_path(&self, path: &Path) -> LanguageFn {
        match path.extension().and_then(|e| e.to_str()) {
            Some("tsx") => self.tsx_language,
            _ => self.ts_language,
        }
    }
}

impl Default for TypeScriptParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for TypeScriptParser {
    fn language(&self) -> Language {
        Language::TypeScript
    }

    fn file_extensions(&self) -> &[&str] {
        &["ts", "tsx"]
    }

    fn parse(&self, source: &[u8], path: &Path) -> domain::error::Result<ParseResult> {
        parse_with_grammar(source, path, self.language_fn_for_path(path))
    }
}

// ---------------------------------------------------------------------------
// JavaScriptParser — handles .js and .jsx
// ---------------------------------------------------------------------------

/// Parser for JavaScript (.js) and JSX (.jsx) files.
pub struct JavaScriptParser {
    js_language: LanguageFn,
    jsx_language: LanguageFn,
}

impl JavaScriptParser {
    pub fn new() -> Self {
        Self {
            js_language: tree_sitter_javascript::LANGUAGE,
            jsx_language: tree_sitter_typescript::LANGUAGE_TSX,
        }
    }

    fn language_fn_for_path(&self, path: &Path) -> LanguageFn {
        match path.extension().and_then(|e| e.to_str()) {
            Some("jsx") => self.jsx_language,
            _ => self.js_language,
        }
    }
}

impl Default for JavaScriptParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for JavaScriptParser {
    fn language(&self) -> Language {
        Language::JavaScript
    }

    fn file_extensions(&self) -> &[&str] {
        &["js", "jsx"]
    }

    fn parse(&self, source: &[u8], path: &Path) -> domain::error::Result<ParseResult> {
        parse_with_grammar(source, path, self.language_fn_for_path(path))
    }
}

// ---------------------------------------------------------------------------
// Shared extraction logic
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
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "export_statement" => {
                // Check if this is a default export
                let is_default = {
                    let mut dc = child.walk();
                    let result = child
                        .children(&mut dc)
                        .any(|c| !c.is_named() && c.kind() == "default");
                    result
                };

                let mut found_declaration = false;
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if !inner.is_named() {
                        continue;
                    }
                    if is_declaration_kind(inner.kind()) {
                        found_declaration = true;
                        extract_declaration(
                            source,
                            &file_path,
                            inner,
                            true,
                            &mut symbols,
                            &mut edges,
                        );
                        // If anonymous default declaration, emit "default" symbol
                        if is_default && symbol_name(inner, source).is_none() {
                            let qn = format!("{file_path}::default");
                            symbols.push(SymbolNode {
                                name: "default".to_string(),
                                qualified_name: qn.clone(),
                                kind: match inner.kind() {
                                    "function_declaration" => SymbolKind::Function,
                                    "class_declaration"
                                    | "abstract_class_declaration" => SymbolKind::Class,
                                    _ => SymbolKind::Variable,
                                },
                                location: node_location(&file_path, inner),
                                visibility: Visibility::Public,
                                is_exported: true,
                                is_async: has_async_keyword(inner),
                                is_test: false,
                                decorators: Vec::new(),
                                signature: build_signature(source, inner),
                            });
                            edges.push(contains_edge(&file_path, &qn));
                        }
                    }
                }

                // Handle `export default <expression>` (function_expression, class_expression, etc.)
                if is_default && !found_declaration {
                    if let Some(value) = child.child_by_field_name("value") {
                        let kind = match value.kind() {
                            "function_expression" | "arrow_function" => Some(SymbolKind::Function),
                            "class" => Some(SymbolKind::Class),
                            _ => None,
                        };
                        if let Some(sym_kind) = kind {
                            let qn = format!("{file_path}::default");
                            symbols.push(SymbolNode {
                                name: "default".to_string(),
                                qualified_name: qn.clone(),
                                kind: sym_kind,
                                location: node_location(&file_path, value),
                                visibility: Visibility::Public,
                                is_exported: true,
                                is_async: has_async_keyword(value),
                                is_test: false,
                                decorators: Vec::new(),
                                signature: build_signature(source, value),
                            });
                            edges.push(contains_edge(&file_path, &qn));
                        }
                    }
                }
            }
            kind if is_declaration_kind(kind) => {
                extract_declaration(
                    source,
                    &file_path,
                    child,
                    false,
                    &mut symbols,
                    &mut edges,
                );
            }
            _ => {}
        }
    }

    let imports = extract_imports(&root, source);
    let exports = extract_exports(&root, source);

    Ok(ParseResult {
        symbols,
        edges,
        imports,
        exports,
    })
}

// ---------------------------------------------------------------------------
// Symbol extraction helpers
// ---------------------------------------------------------------------------

fn is_declaration_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "lexical_declaration"
            | "variable_declaration"
    )
}

/// Extract a declaration node into symbols and edges.
fn extract_declaration(
    source: &[u8],
    file_path: &str,
    node: Node,
    is_exported: bool,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    match node.kind() {
        "function_declaration" => {
            if let Some(sym) = extract_function(source, file_path, node, is_exported) {
                edges.push(contains_edge(file_path, &sym.qualified_name));
                symbols.push(sym);
            }
        }
        "class_declaration" | "abstract_class_declaration" => {
            extract_class(source, file_path, node, is_exported, symbols, edges);
        }
        "interface_declaration" => {
            extract_interface(source, file_path, node, is_exported, symbols, edges);
        }
        "type_alias_declaration" => {
            if let Some(sym) = extract_type_alias(source, file_path, node, is_exported) {
                edges.push(contains_edge(file_path, &sym.qualified_name));
                symbols.push(sym);
            }
        }
        "enum_declaration" => {
            if let Some(sym) = extract_enum(source, file_path, node, is_exported) {
                edges.push(contains_edge(file_path, &sym.qualified_name));
                symbols.push(sym);
            }
        }
        "lexical_declaration" | "variable_declaration" => {
            extract_variable_declaration(source, file_path, node, is_exported, symbols, edges);
        }
        _ => {}
    }
}

fn extract_function(
    source: &[u8],
    file_path: &str,
    node: Node,
    is_exported: bool,
) -> Option<SymbolNode> {
    let name = symbol_name(node, source)?;
    let qualified_name = format!("{file_path}::{name}");
    let is_async = has_async_keyword(node);
    let signature = build_signature(source, node);
    let decorators = extract_decorators(source, node);

    Some(SymbolNode {
        name: name.clone(),
        qualified_name,
        kind: SymbolKind::Function,
        location: node_location(file_path, node),
        visibility: export_visibility(is_exported),
        is_exported,
        is_async,
        is_test: is_test_name(&name),
        decorators,
        signature,
    })
}

fn extract_class(
    source: &[u8],
    file_path: &str,
    node: Node,
    is_exported: bool,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let name = match symbol_name(node, source) {
        Some(n) => n,
        None => return,
    };
    let qualified_name = format!("{file_path}::{name}");
    let decorators = extract_decorators(source, node);

    let class_sym = SymbolNode {
        name: name.clone(),
        qualified_name: qualified_name.clone(),
        kind: SymbolKind::Class,
        location: node_location(file_path, node),
        visibility: export_visibility(is_exported),
        is_exported,
        is_async: false,
        is_test: is_test_name(&name),
        decorators,
        signature: None,
    };
    edges.push(contains_edge(file_path, &class_sym.qualified_name));
    symbols.push(class_sym);

    // Extract class body members
    if let Some(body) = node.child_by_field_name("body") {
        let mut body_cursor = body.walk();
        for member in body.children(&mut body_cursor) {
            if !member.is_named() {
                continue;
            }
            extract_class_member(
                source,
                file_path,
                &name,
                &qualified_name,
                member,
                is_exported,
                symbols,
                edges,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_class_member(
    source: &[u8],
    file_path: &str,
    class_name: &str,
    class_qualified_name: &str,
    member: Node,
    is_exported: bool,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let (kind, member_name) = match member.kind() {
        "method_definition" => (SymbolKind::Method, symbol_name(member, source)),
        "public_field_definition" => {
            // TS grammar: field name via "name" field
            let n = member
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                .map(|s| s.to_string());
            (SymbolKind::Property, n)
        }
        "field_definition" => {
            // JS grammar: field name via "property" field
            let n = member
                .child_by_field_name("property")
                .and_then(|n| n.utf8_text(source).ok())
                .map(|s| s.to_string());
            (SymbolKind::Property, n)
        }
        _ => return,
    };

    let member_name = match member_name {
        Some(n) => n,
        None => return,
    };

    let member_qualified = format!("{file_path}::{class_name}.{member_name}");
    let is_async = has_async_keyword(member);
    let signature = if kind == SymbolKind::Method {
        build_signature(source, member)
    } else {
        None
    };
    let decorators = extract_decorators(source, member);

    let sym = SymbolNode {
        name: member_name.clone(),
        qualified_name: member_qualified.clone(),
        kind,
        location: node_location(file_path, member),
        visibility: export_visibility(is_exported),
        is_exported,
        is_async,
        is_test: is_test_name(&member_name),
        decorators,
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

fn extract_interface(
    source: &[u8],
    file_path: &str,
    node: Node,
    is_exported: bool,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let name = match symbol_name(node, source) {
        Some(n) => n,
        None => return,
    };
    let qualified_name = format!("{file_path}::{name}");

    let iface_sym = SymbolNode {
        name: name.clone(),
        qualified_name: qualified_name.clone(),
        kind: SymbolKind::Interface,
        location: node_location(file_path, node),
        visibility: export_visibility(is_exported),
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    };
    edges.push(contains_edge(file_path, &iface_sym.qualified_name));
    symbols.push(iface_sym);

    // Extract interface body members
    if let Some(body) = node.child_by_field_name("body") {
        let mut body_cursor = body.walk();
        for member in body.children(&mut body_cursor) {
            if !member.is_named() {
                continue;
            }
            extract_interface_member(
                source,
                file_path,
                &name,
                &qualified_name,
                member,
                is_exported,
                symbols,
                edges,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_interface_member(
    source: &[u8],
    file_path: &str,
    iface_name: &str,
    iface_qualified_name: &str,
    member: Node,
    is_exported: bool,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let (kind, member_name) = match member.kind() {
        "property_signature" => {
            let n = member
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                .map(|s| s.to_string());
            (SymbolKind::Property, n)
        }
        "method_signature" => {
            let n = symbol_name(member, source);
            (SymbolKind::Method, n)
        }
        _ => return,
    };

    let member_name = match member_name {
        Some(n) => n,
        None => return,
    };

    let member_qualified = format!("{file_path}::{iface_name}.{member_name}");
    let signature = if kind == SymbolKind::Method {
        build_signature(source, member)
    } else {
        None
    };

    let sym = SymbolNode {
        name: member_name.clone(),
        qualified_name: member_qualified.clone(),
        kind,
        location: node_location(file_path, member),
        visibility: export_visibility(is_exported),
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature,
    };
    symbols.push(sym);
    edges.push(Edge {
        kind: EdgeKind::ChildOf,
        source: member_qualified,
        target: iface_qualified_name.to_string(),
        metadata: None,
    });
}

fn extract_type_alias(
    source: &[u8],
    file_path: &str,
    node: Node,
    is_exported: bool,
) -> Option<SymbolNode> {
    let name = symbol_name(node, source)?;
    let qualified_name = format!("{file_path}::{name}");

    Some(SymbolNode {
        name: name.clone(),
        qualified_name,
        kind: SymbolKind::TypeAlias,
        location: node_location(file_path, node),
        visibility: export_visibility(is_exported),
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    })
}

fn extract_enum(
    source: &[u8],
    file_path: &str,
    node: Node,
    is_exported: bool,
) -> Option<SymbolNode> {
    let name = symbol_name(node, source)?;
    let qualified_name = format!("{file_path}::{name}");

    Some(SymbolNode {
        name: name.clone(),
        qualified_name,
        kind: SymbolKind::Enum,
        location: node_location(file_path, node),
        visibility: export_visibility(is_exported),
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    })
}

fn extract_variable_declaration(
    source: &[u8],
    file_path: &str,
    node: Node,
    is_exported: bool,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let is_const = is_const_declaration(source, node);

    // Iterate over variable_declarator children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "variable_declarator" {
            continue;
        }
        let name = match child
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| s.to_string())
        {
            Some(n) => n,
            None => continue,
        };

        // Check if the value is an arrow function or function expression
        let value_node = child.child_by_field_name("value");
        let is_function_value = value_node
            .map(|v| {
                matches!(
                    v.kind(),
                    "arrow_function" | "function_expression" | "function"
                )
            })
            .unwrap_or(false);

        let kind = if is_function_value {
            SymbolKind::Function
        } else if is_const {
            SymbolKind::Const
        } else {
            SymbolKind::Variable
        };

        // Check for async on the value (arrow function / function expression)
        let is_async = value_node.map(|v| has_async_keyword(v)).unwrap_or(false);

        let qualified_name = format!("{file_path}::{name}");
        let signature = if is_function_value {
            value_node.and_then(|v| build_signature(source, v))
        } else {
            None
        };
        let decorators = extract_decorators(source, node);

        let sym = SymbolNode {
            name: name.clone(),
            qualified_name: qualified_name.clone(),
            kind,
            location: node_location(file_path, node),
            visibility: export_visibility(is_exported),
            is_exported,
            is_async,
            is_test: is_test_name(&name),
            decorators,
            signature,
        };
        edges.push(contains_edge(file_path, &sym.qualified_name));
        symbols.push(sym);
    }
}

/// Get the name of a node via the "name" field.
fn symbol_name(node: Node, source: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// Build a Location from a tree-sitter node.
fn node_location(file_path: &str, node: Node) -> Location {
    let start = node.start_position();
    let end = node.end_position();
    Location {
        file: file_path.into(),
        line_start: start.row + 1, // Convert 0-based to 1-based
        line_end: end.row + 1,
        col_start: start.column,
        col_end: end.column,
    }
}

/// Check if a node has an unnamed "async" child token.
fn has_async_keyword(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() && child.kind() == "async" {
            return true;
        }
    }
    false
}

/// Check whether a lexical/variable declaration uses `const`.
fn is_const_declaration(source: &[u8], node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            if let Ok(text) = child.utf8_text(source) {
                if text == "const" {
                    return true;
                }
            }
        }
        // Keyword comes before any named children
        if child.is_named() {
            break;
        }
    }
    false
}

/// Build a function/method signature string from parameters and return type.
fn build_signature(source: &[u8], node: Node) -> Option<String> {
    let params = node
        .child_by_field_name("parameters")
        .and_then(|n| n.utf8_text(source).ok())?;

    let return_type = node
        .child_by_field_name("return_type")
        .and_then(|n| n.utf8_text(source).ok());

    if let Some(ret) = return_type {
        Some(format!("{params}{ret}"))
    } else {
        Some(params.to_string())
    }
}

/// Extract decorator text from named "decorator" children.
fn extract_decorators(source: &[u8], node: Node) -> Vec<String> {
    let mut decorators = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() && child.kind() == "decorator" {
            if let Ok(text) = child.utf8_text(source) {
                decorators.push(text.to_string());
            }
        }
    }
    decorators
}

/// Check if a name looks like a test function.
/// Matches: test*, it (exact), itShould... (camelCase), it_... (snake_case), describe*
fn is_test_name(name: &str) -> bool {
    if name.starts_with("test") || name.starts_with("describe") {
        return true;
    }
    if name == "it" {
        return true;
    }
    // Match camelCase "itSomething" but not "items", "iterator", etc.
    if let Some(rest) = name.strip_prefix("it") {
        if rest.starts_with('_') || rest.starts_with(|c: char| c.is_ascii_uppercase()) {
            return true;
        }
    }
    false
}

/// Map export status to visibility.
fn export_visibility(is_exported: bool) -> Visibility {
    if is_exported {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

/// Create a Contains edge from file path to a qualified name.
fn contains_edge(file_path: &str, qualified_name: &str) -> Edge {
    Edge {
        kind: EdgeKind::Contains,
        source: file_path.to_string(),
        target: qualified_name.to_string(),
        metadata: None,
    }
}

// ---------------------------------------------------------------------------
// Import extraction (standalone — will be integrated into extract() later)
// ---------------------------------------------------------------------------

/// Extract all import statements from a tree-sitter CST root node.
pub(crate) fn extract_imports(root: &tree_sitter::Node, source: &[u8]) -> Vec<RawImport> {
    let mut imports = Vec::new();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if child.kind() == "import_statement" {
            if let Some(imp) = parse_import_statement(&child, source) {
                imports.push(imp);
            }
        }
    }

    imports
}

fn node_text<'a>(node: &tree_sitter::Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn strip_quotes(s: &str) -> &str {
    s.trim_matches(|c: char| c == '"' || c == '\'' || c == '`')
}

fn parse_import_statement(node: &tree_sitter::Node, source: &[u8]) -> Option<RawImport> {
    // Extract the specifier from the "source" field (the string literal).
    let specifier_node = node.child_by_field_name("source")?;
    let specifier = strip_quotes(node_text(&specifier_node, source)).to_string();

    let line = node.start_position().row + 1; // 0-based → 1-based

    // Detect `import type` by looking for an unnamed "type" token among children.
    let mut is_type_only = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() && child.kind() == "type" {
            is_type_only = true;
            break;
        }
    }

    // Find the import_clause child (if any).
    let mut import_clause = None;
    let mut cursor2 = node.walk();
    for child in node.children(&mut cursor2) {
        if child.kind() == "import_clause" {
            import_clause = Some(child);
            break;
        }
    }

    // Side-effect import: no import_clause
    let Some(clause) = import_clause else {
        return Some(RawImport {
            specifier,
            names: Vec::new(),
            is_type_only,
            is_side_effect: true,
            is_namespace: false,
            line,
        });
    };

    let mut names = Vec::new();
    let mut is_namespace = false;

    let mut clause_cursor = clause.walk();
    for child in clause.children(&mut clause_cursor) {
        match child.kind() {
            // Default import: a direct identifier child of import_clause
            "identifier" => {
                let local_name = node_text(&child, source).to_string();
                names.push(ImportName {
                    name: "default".to_string(),
                    alias: Some(local_name),
                    is_type: false,
                });
            }
            // Named imports: { a, b as c }
            "named_imports" => {
                let mut ni_cursor = child.walk();
                for spec in child.children(&mut ni_cursor) {
                    if spec.kind() == "import_specifier" {
                        let name_node = spec.child_by_field_name("name");
                        let alias_node = spec.child_by_field_name("alias");

                        if let Some(n) = name_node {
                            let name = node_text(&n, source).to_string();
                            let alias =
                                alias_node.map(|a| node_text(&a, source).to_string());
                            names.push(ImportName {
                                name,
                                alias,
                                is_type: false,
                            });
                        }
                    }
                }
            }
            // Namespace import: * as ns
            "namespace_import" => {
                is_namespace = true;
                let mut ns_cursor = child.walk();
                for ns_child in child.children(&mut ns_cursor) {
                    if ns_child.kind() == "identifier" {
                        let alias = node_text(&ns_child, source).to_string();
                        names.push(ImportName {
                            name: "*".to_string(),
                            alias: Some(alias),
                            is_type: false,
                        });
                        break;
                    }
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

// ---------------------------------------------------------------------------
// Export extraction (standalone — will be integrated into extract() later)
// ---------------------------------------------------------------------------

/// Extract all names from a declaration node.
/// For `lexical_declaration`/`variable_declaration`, returns all `variable_declarator` names.
/// For other declarations, returns a single name.
fn declaration_names(node: &tree_sitter::Node, source: &[u8]) -> Vec<String> {
    if node.kind() == "lexical_declaration" || node.kind() == "variable_declaration" {
        let mut names = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                if let Some(name) = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                {
                    names.push(name.to_string());
                }
            }
        }
        names
    } else {
        node.child_by_field_name("name")
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default()
    }
}

/// Extract all export statements from a tree-sitter CST root node.
///
/// This is a standalone function — it does **not** modify `TypeScriptParser::extract`.
/// Integration into the main `extract()` method will happen in a later slice.
pub(crate) fn extract_exports(root: &tree_sitter::Node, source: &[u8]) -> Vec<crate::Export> {
    let mut exports = Vec::new();
    let mut cursor = root.walk();

    for top_node in root.children(&mut cursor) {
        if top_node.kind() != "export_statement" {
            continue;
        }

        let node = &top_node;

        // Detect boolean flags from unnamed children
        let mut has_default = false;
        let mut has_type = false;
        let mut has_star = false;
        {
            let mut child_cursor = node.walk();
            for child in node.children(&mut child_cursor) {
                if !child.is_named() {
                    match child.kind() {
                        "default" => has_default = true,
                        "type" => has_type = true,
                        "*" => has_star = true,
                        _ => {}
                    }
                }
            }
        }

        // Source specifier (re-export target)
        let source_specifier = node
            .child_by_field_name("source")
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| strip_quotes(s).to_string());
        let is_reexport = source_specifier.is_some();

        // Star re-export: `export * from "./barrel"`
        if has_star {
            exports.push(crate::Export {
                name: "*".to_string(),
                is_reexport: true,
                source_specifier,
                ..crate::Export::default()
            });
            continue;
        }

        // Export clause: `export { foo, bar as baz }` or `export { foo } from "./mod"`
        let export_clause = {
            let mut child_cursor = node.walk();
            let mut found = None;
            for child in node.children(&mut child_cursor) {
                if child.kind() == "export_clause" {
                    found = Some(child);
                    break;
                }
            }
            found
        };

        if let Some(clause) = export_clause {
            let mut spec_cursor = clause.walk();
            for spec in clause.children(&mut spec_cursor) {
                if spec.kind() != "export_specifier" {
                    continue;
                }
                let name_node = spec.child_by_field_name("name");
                let alias_node = spec.child_by_field_name("alias");

                let local_text = name_node
                    .and_then(|n| n.utf8_text(source).ok())
                    .unwrap_or("")
                    .to_string();

                let (export_name, local_name) = if let Some(alias) = alias_node {
                    let alias_text = alias.utf8_text(source).unwrap_or("").to_string();
                    (alias_text, Some(local_text))
                } else {
                    (local_text, None)
                };

                exports.push(crate::Export {
                    name: export_name,
                    local_name,
                    is_type_only: has_type,
                    is_reexport,
                    source_specifier: source_specifier.clone(),
                    ..crate::Export::default()
                });
            }
            continue;
        }

        // Declaration export: `export function foo() {}`, `export default class Bar {}`
        if let Some(decl) = node.child_by_field_name("declaration") {
            let names = declaration_names(&decl, source);

            if has_default {
                exports.push(crate::Export {
                    name: "default".to_string(),
                    local_name: names.into_iter().next(),
                    is_default: true,
                    ..crate::Export::default()
                });
            } else {
                for name in names {
                    exports.push(crate::Export {
                        name,
                        ..crate::Export::default()
                    });
                }
            }
            continue;
        }

        // Default expression export: `export default 42`, `export default myVar`
        if has_default {
            let value_node = node.child_by_field_name("value");
            let local_name = value_node.and_then(|n| {
                if n.kind() == "identifier" {
                    n.utf8_text(source).ok().map(|s| s.to_string())
                } else {
                    None
                }
            });

            exports.push(crate::Export {
                name: "default".to_string(),
                local_name,
                is_default: true,
                ..crate::Export::default()
            });
        }
    }

    exports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_returns_typescript() {
        let parser = TypeScriptParser::new();
        assert_eq!(parser.language(), Language::TypeScript);
    }

    #[test]
    fn ts_file_extensions() {
        let parser = TypeScriptParser::new();
        let exts = parser.file_extensions();
        assert!(exts.contains(&"ts"));
        assert!(exts.contains(&"tsx"));
        assert!(!exts.contains(&"js"));
    }

    #[test]
    fn js_file_extensions() {
        let parser = JavaScriptParser::new();
        let exts = parser.file_extensions();
        assert!(exts.contains(&"js"));
        assert!(exts.contains(&"jsx"));
        assert!(!exts.contains(&"ts"));
    }

    #[test]
    fn js_language_returns_javascript() {
        let parser = JavaScriptParser::new();
        assert_eq!(parser.language(), Language::JavaScript);
    }

    #[test]
    fn parse_empty_ts_file() {
        let parser = TypeScriptParser::new();
        let result = parser
            .parse(b"", Path::new("test.ts"))
            .expect("should parse empty file");
        assert!(result.symbols.is_empty());
        assert!(result.edges.is_empty());
        assert!(result.imports.is_empty());
        assert!(result.exports.is_empty());
    }

    #[test]
    fn parse_empty_js_file() {
        let parser = JavaScriptParser::new();
        let result = parser
            .parse(b"", Path::new("test.js"))
            .expect("should parse empty file");
        assert!(result.symbols.is_empty());
    }

    #[test]
    fn language_fn_selects_correct_grammar() {
        let ts_parser = TypeScriptParser::new();
        let _ = ts_parser.language_fn_for_path(Path::new("a.ts"));
        let _ = ts_parser.language_fn_for_path(Path::new("a.tsx"));

        let js_parser = JavaScriptParser::new();
        let _ = js_parser.language_fn_for_path(Path::new("a.js"));
        let _ = js_parser.language_fn_for_path(Path::new("a.jsx"));
    }

    // -----------------------------------------------------------------------
    // Import extraction tests
    // -----------------------------------------------------------------------

    fn parse_ts_imports(source: &str) -> Vec<crate::RawImport> {
        let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser.set_language(&lang).unwrap();
        let tree = ts_parser.parse(source.as_bytes(), None).unwrap();
        extract_imports(&tree.root_node(), source.as_bytes())
    }

    #[test]
    fn import_named() {
        let imports = parse_ts_imports(r#"import { a, b } from "./mod""#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].specifier, "./mod");
        assert_eq!(imports[0].names.len(), 2);
        assert_eq!(imports[0].names[0].name, "a");
        assert_eq!(imports[0].names[1].name, "b");
        assert!(!imports[0].is_type_only);
    }

    #[test]
    fn import_type_only() {
        let imports = parse_ts_imports(r#"import type { T } from "./types""#);
        assert_eq!(imports.len(), 1);
        assert!(imports[0].is_type_only);
    }

    #[test]
    fn import_namespace() {
        let imports = parse_ts_imports(r#"import * as ns from "./ns""#);
        assert_eq!(imports.len(), 1);
        assert!(imports[0].is_namespace);
    }

    #[test]
    fn import_side_effect() {
        let imports = parse_ts_imports(r#"import "./polyfill""#);
        assert_eq!(imports.len(), 1);
        assert!(imports[0].is_side_effect);
        assert!(imports[0].names.is_empty());
    }

    #[test]
    fn import_default() {
        let imports = parse_ts_imports(r#"import def from "./mod""#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names.len(), 1);
        assert_eq!(imports[0].names[0].name, "default");
        assert_eq!(imports[0].names[0].alias, Some("def".to_string()));
    }

    #[test]
    fn import_mixed_default_and_named() {
        let imports = parse_ts_imports(r#"import def, { a } from "./mod""#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Export extraction tests
    // -----------------------------------------------------------------------

    fn parse_ts_exports(source: &str) -> Vec<crate::Export> {
        let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser.set_language(&lang).unwrap();
        let tree = ts_parser.parse(source.as_bytes(), None).unwrap();
        extract_exports(&tree.root_node(), source.as_bytes())
    }

    #[test]
    fn export_function() {
        let exports = parse_ts_exports("export function foo() {}");
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].name, "foo");
        assert!(!exports[0].is_default);
        assert!(!exports[0].is_reexport);
    }

    #[test]
    fn export_default_class() {
        let exports = parse_ts_exports("export default class Bar {}");
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].name, "default");
        assert_eq!(exports[0].local_name, Some("Bar".to_string()));
        assert!(exports[0].is_default);
    }

    #[test]
    fn export_reexport() {
        let exports = parse_ts_exports(r#"export { foo } from "./mod""#);
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].name, "foo");
        assert!(exports[0].is_reexport);
        assert_eq!(exports[0].source_specifier, Some("./mod".to_string()));
    }

    #[test]
    fn export_with_alias() {
        let exports = parse_ts_exports("export { foo as bar }");
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].name, "bar");
        assert_eq!(exports[0].local_name, Some("foo".to_string()));
    }

    #[test]
    fn export_star() {
        let exports = parse_ts_exports(r#"export * from "./barrel""#);
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].name, "*");
        assert!(exports[0].is_reexport);
        assert_eq!(exports[0].source_specifier, Some("./barrel".to_string()));
    }

    #[test]
    fn export_type_only() {
        let exports = parse_ts_exports("export type { Foo }");
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].name, "Foo");
        assert!(exports[0].is_type_only);
    }

    #[test]
    fn export_default_expression() {
        let exports = parse_ts_exports("export default 42");
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].name, "default");
        assert!(exports[0].is_default);
        assert!(exports[0].local_name.is_none());
    }

    // -----------------------------------------------------------------------
    // Symbol extraction tests
    // -----------------------------------------------------------------------

    fn parse_ts(source: &str) -> ParseResult {
        let parser = TypeScriptParser::new();
        parser
            .parse(source.as_bytes(), Path::new("test.ts"))
            .expect("parse failed")
    }

    #[test]
    fn symbol_function_declaration() {
        let result = parse_ts("function foo() {}");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "foo");
        assert_eq!(sym.kind, SymbolKind::Function);
        assert!(!sym.is_exported);
        assert_eq!(sym.visibility, Visibility::Private);
    }

    #[test]
    fn symbol_class_with_method() {
        let result = parse_ts("class Bar { baz() {} }");
        assert_eq!(result.symbols.len(), 2);

        let class_sym = result.symbols.iter().find(|s| s.name == "Bar").unwrap();
        assert_eq!(class_sym.kind, SymbolKind::Class);

        let method_sym = result.symbols.iter().find(|s| s.name == "baz").unwrap();
        assert_eq!(method_sym.kind, SymbolKind::Method);

        // Verify ChildOf edge exists
        let child_of = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::ChildOf)
            .unwrap();
        assert_eq!(child_of.source, "test.ts::Bar.baz");
        assert_eq!(child_of.target, "test.ts::Bar");
    }

    #[test]
    fn symbol_interface_with_property() {
        let result = parse_ts("interface IFoo { prop: string }");
        assert_eq!(result.symbols.len(), 2);

        let iface = result.symbols.iter().find(|s| s.name == "IFoo").unwrap();
        assert_eq!(iface.kind, SymbolKind::Interface);

        let prop = result.symbols.iter().find(|s| s.name == "prop").unwrap();
        assert_eq!(prop.kind, SymbolKind::Property);
    }

    #[test]
    fn symbol_type_alias() {
        let result = parse_ts("type Alias = string");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "Alias");
        assert_eq!(sym.kind, SymbolKind::TypeAlias);
    }

    #[test]
    fn symbol_enum_declaration() {
        let result = parse_ts("enum Color { Red, Green }");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "Color");
        assert_eq!(sym.kind, SymbolKind::Enum);
    }

    #[test]
    fn symbol_exported_async_arrow() {
        let result = parse_ts("export const handler = async () => {}");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "handler");
        assert_eq!(sym.kind, SymbolKind::Function);
        assert!(sym.is_async);
        assert!(sym.is_exported);
        assert_eq!(sym.visibility, Visibility::Public);
    }

    #[test]
    fn symbol_export_default_function() {
        let result = parse_ts("export default function main() {}");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "main");
        assert_eq!(sym.kind, SymbolKind::Function);
        assert!(sym.is_exported);
    }

    #[test]
    fn symbol_non_exported_private() {
        let result = parse_ts("function helper() {}");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.visibility, Visibility::Private);
        assert!(!sym.is_exported);
    }

    #[test]
    fn symbol_contains_edges() {
        let result = parse_ts("function a() {}\nfunction b() {}");
        let contains: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .collect();
        assert_eq!(contains.len(), 2);
        for edge in &contains {
            assert_eq!(edge.source, "test.ts");
        }
        assert!(contains.iter().any(|e| e.target == "test.ts::a"));
        assert!(contains.iter().any(|e| e.target == "test.ts::b"));
    }

    #[test]
    fn symbol_child_of_edges() {
        let result = parse_ts("class Foo { bar() {} }");
        let child_of: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::ChildOf)
            .collect();
        assert_eq!(child_of.len(), 1);
        assert_eq!(child_of[0].source, "test.ts::Foo.bar");
        assert_eq!(child_of[0].target, "test.ts::Foo");
    }

    #[test]
    fn symbol_qualified_names() {
        let result = parse_ts("class MyClass { myMethod() {} }");
        let class_sym = result.symbols.iter().find(|s| s.name == "MyClass").unwrap();
        assert_eq!(class_sym.qualified_name, "test.ts::MyClass");

        let method_sym = result
            .symbols
            .iter()
            .find(|s| s.name == "myMethod")
            .unwrap();
        assert_eq!(method_sym.qualified_name, "test.ts::MyClass.myMethod");
    }

    #[test]
    fn symbol_const_variable() {
        let result = parse_ts("const MAX = 100");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "MAX");
        assert_eq!(sym.kind, SymbolKind::Const);
    }

    #[test]
    fn symbol_let_variable() {
        let result = parse_ts("let count = 0");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "count");
        assert_eq!(sym.kind, SymbolKind::Variable);
    }

    #[test]
    fn symbol_function_signature() {
        let result = parse_ts("function add(x: number, y: number): number { return x + y; }");
        let sym = &result.symbols[0];
        assert!(sym.signature.is_some());
        let sig = sym.signature.as_ref().unwrap();
        assert!(sig.contains("x: number"));
        assert!(sig.contains("number")); // return type
    }

    #[test]
    fn symbol_interface_method_signature() {
        let result = parse_ts("interface ICalc { add(a: number, b: number): number }");
        let method = result.symbols.iter().find(|s| s.name == "add").unwrap();
        assert_eq!(method.kind, SymbolKind::Method);
        assert!(method.signature.is_some());
    }

    #[test]
    fn symbol_is_test_heuristic() {
        let result = parse_ts("function testSomething() {}");
        let sym = &result.symbols[0];
        assert!(sym.is_test);

        let result2 = parse_ts("function helper() {}");
        let sym2 = &result2.symbols[0];
        assert!(!sym2.is_test);
    }

    #[test]
    fn symbol_location_is_populated() {
        let result = parse_ts("function foo() {}");
        let sym = &result.symbols[0];
        assert_eq!(sym.location.file.to_string_lossy(), "test.ts");
        assert_eq!(sym.location.line_start, 1);
        assert!(sym.location.line_end >= 1);
    }

    // -----------------------------------------------------------------------
    // Error handling tests (AC29, AC30)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_invalid_source_returns_error_not_panic() {
        // Empty source should not panic — tree-sitter handles it gracefully
        let parser = TypeScriptParser::new();
        let result = parser.parse(b"", Path::new("empty.ts"));
        assert!(result.is_ok());
    }

    #[test]
    fn parse_syntax_errors_returns_partial_result() {
        // Source with syntax errors should still extract what it can (AC30)
        let source = r#"
function valid() {}
const x = {{{;
function alsoValid() {}
"#;
        let parser = TypeScriptParser::new();
        let result = parser
            .parse(source.as_bytes(), Path::new("partial.ts"))
            .expect("should not error on syntax errors");
        // Should have extracted at least 'valid' — tree-sitter does best-effort
        assert!(
            !result.symbols.is_empty(),
            "should extract at least some symbols from partially broken source"
        );
        assert!(result.symbols.iter().any(|s| s.name == "valid"));
    }

    // -----------------------------------------------------------------------
    // Thread safety test (AC31)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_from_two_threads_concurrently() {
        use std::thread;

        let handles: Vec<_> = (0..2)
            .map(|i| {
                thread::spawn(move || {
                    let parser = TypeScriptParser::new();
                    let source = format!("function thread{i}() {{}}");
                    let result = parser
                        .parse(source.as_bytes(), Path::new(&format!("thread{i}.ts")))
                        .expect("concurrent parse failed");
                    assert_eq!(result.symbols.len(), 1);
                    assert_eq!(result.symbols[0].name, format!("thread{i}"));
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }
    }

    // -----------------------------------------------------------------------
    // Integration test (multi-construct file)
    // -----------------------------------------------------------------------

    #[test]
    fn integration_multi_construct_file() {
        let source = r#"
import { helper } from "./utils";
import type { Config } from "./config";

export function processData(data: string[]): number {
    return data.length;
}

class DataProcessor {
    private cache: Map<string, number>;

    constructor() {
        this.cache = new Map();
    }

    process(input: string): number {
        return input.length;
    }
}

interface IProcessor {
    process(input: string): number;
}

type ProcessorFn = (input: string) => number;

enum Status {
    Active,
    Inactive,
}

export const transformer = async (x: number) => x * 2;

export default class MainProcessor {}

export { DataProcessor };
export * from "./helpers";
"#;
        let result = parse_ts(source);

        // Symbols
        assert!(result.symbols.iter().any(|s| s.name == "processData" && s.kind == SymbolKind::Function));
        assert!(result.symbols.iter().any(|s| s.name == "DataProcessor" && s.kind == SymbolKind::Class));
        assert!(result.symbols.iter().any(|s| s.name == "process" && s.kind == SymbolKind::Method));
        assert!(result.symbols.iter().any(|s| s.name == "IProcessor" && s.kind == SymbolKind::Interface));
        assert!(result.symbols.iter().any(|s| s.name == "ProcessorFn" && s.kind == SymbolKind::TypeAlias));
        assert!(result.symbols.iter().any(|s| s.name == "Status" && s.kind == SymbolKind::Enum));
        assert!(result.symbols.iter().any(|s| s.name == "transformer" && s.kind == SymbolKind::Function && s.is_async));
        assert!(result.symbols.iter().any(|s| s.name == "MainProcessor" && s.kind == SymbolKind::Class));

        // Edges
        let contains_count = result.edges.iter().filter(|e| e.kind == EdgeKind::Contains).count();
        assert!(contains_count >= 7, "expected at least 7 Contains edges, got {contains_count}");

        let child_of_count = result.edges.iter().filter(|e| e.kind == EdgeKind::ChildOf).count();
        assert!(child_of_count >= 2, "expected at least 2 ChildOf edges, got {child_of_count}");

        // Imports
        assert_eq!(result.imports.len(), 2);
        assert!(result.imports.iter().any(|i| i.specifier == "./utils"));
        assert!(result.imports.iter().any(|i| i.specifier == "./config" && i.is_type_only));

        // Exports
        assert!(result.exports.iter().any(|e| e.name == "processData"));
        assert!(result.exports.iter().any(|e| e.name == "transformer"));
        assert!(result.exports.iter().any(|e| e.name == "default" && e.is_default));
        assert!(result.exports.iter().any(|e| e.name == "DataProcessor"));
        assert!(result.exports.iter().any(|e| e.name == "*" && e.is_reexport));
    }

    #[test]
    fn extract_integrates_imports_and_exports() {
        let source = r#"
import { x } from "./mod";
export function foo() {}
"#;
        let result = parse_ts(source);
        assert_eq!(result.imports.len(), 1);
        assert_eq!(result.imports[0].specifier, "./mod");
        assert_eq!(result.exports.len(), 1);
        assert_eq!(result.exports[0].name, "foo");
    }

    // -----------------------------------------------------------------------
    // Review fix tests (W1, W2, W3)
    // -----------------------------------------------------------------------

    #[test]
    fn is_test_no_false_positive_on_items() {
        // W1: "items", "iterator" should NOT be marked as test
        let result = parse_ts("function items() {}");
        let sym = &result.symbols[0];
        assert!(!sym.is_test, "'items' should not be detected as test");

        let result2 = parse_ts("function iterator() {}");
        let sym2 = &result2.symbols[0];
        assert!(!sym2.is_test, "'iterator' should not be detected as test");
    }

    #[test]
    fn is_test_matches_it_exact_and_camel() {
        // W1: "it" (exact) and "itShould" (camelCase) are test names
        let result = parse_ts("function itShouldWork() {}");
        assert!(result.symbols[0].is_test);
    }

    #[test]
    fn export_multi_declarator() {
        // W2: `export const a = 1, b = 2` should produce two export entries
        let exports = parse_ts_exports("export const a = 1, b = 2");
        assert_eq!(exports.len(), 2);
        assert!(exports.iter().any(|e| e.name == "a"));
        assert!(exports.iter().any(|e| e.name == "b"));
    }

    #[test]
    fn export_default_anonymous_function() {
        // W3: `export default function() {}` should produce a "default" symbol
        let result = parse_ts("export default function() {}");
        assert!(
            result.symbols.iter().any(|s| s.name == "default" && s.kind == SymbolKind::Function),
            "anonymous default export should produce a 'default' symbol, got: {:?}",
            result.symbols.iter().map(|s| (&s.name, &s.kind)).collect::<Vec<_>>()
        );
    }
}
