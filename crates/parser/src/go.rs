use std::cell::RefCell;
use std::path::Path;

use tree_sitter::{Node, Parser};
use tree_sitter_language::LanguageFn;

use domain::error::CodeGraphError;
use domain::model::{Edge, EdgeKind, Language, Location, SymbolKind, SymbolNode, Visibility};

use crate::{ImportName, LanguageParser, ParseResult, RawImport};

thread_local! {
    static GO_PARSER: RefCell<Parser> = RefCell::new(Parser::new());
}

/// Parser for Go (.go) files.
pub struct GoParser {
    lang: LanguageFn,
}

impl GoParser {
    pub fn new() -> Self {
        Self {
            lang: tree_sitter_go::LANGUAGE,
        }
    }
}

impl Default for GoParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for GoParser {
    fn language(&self) -> Language {
        Language::Go
    }

    fn file_extensions(&self) -> &[&str] {
        &["go"]
    }

    fn parse(&self, source: &[u8], path: &Path) -> domain::error::Result<ParseResult> {
        let lang: tree_sitter::Language = self.lang.into();

        GO_PARSER.with(|parser_cell| {
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
// Core extraction
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
            "function_declaration" => {
                extract_function(source, &file_path, child, &mut symbols, &mut edges);
            }
            "method_declaration" => {
                extract_method(source, &file_path, child, &mut symbols, &mut edges);
            }
            "type_declaration" => {
                extract_type_declaration(source, &file_path, child, &mut symbols, &mut edges);
            }
            "const_declaration" => {
                extract_const_declaration(source, &file_path, child, &mut symbols, &mut edges);
            }
            "var_declaration" => {
                extract_var_declaration(source, &file_path, child, &mut symbols, &mut edges);
            }
            _ => {}
        }
    }

    let imports = extract_imports(source, &root);

    Ok(ParseResult {
        symbols,
        edges,
        imports,
        exports: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Symbol extraction helpers
// ---------------------------------------------------------------------------

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

/// Get node text as a string.
fn node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

/// Check if a name is exported (starts with uppercase letter).
fn go_visibility(name: &str) -> (Visibility, bool) {
    if name.starts_with(|c: char| c.is_ascii_uppercase()) {
        (Visibility::Public, true)
    } else {
        (Visibility::Private, false)
    }
}

/// Detect Go test functions (Test*, Bench*, Example*, Fuzz*).
fn is_go_test(name: &str) -> bool {
    name.starts_with("Test")
        || name.starts_with("Bench")
        || name.starts_with("Example")
        || name.starts_with("Fuzz")
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

/// Extract a `function_declaration` node.
fn extract_function(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };
    let name = node_text(name_node, source).to_string();
    if name.is_empty() {
        return;
    }
    let qualified_name = format!("{file_path}::{name}");
    let (visibility, is_exported) = go_visibility(&name);

    symbols.push(SymbolNode {
        name: name.clone(),
        qualified_name: qualified_name.clone(),
        kind: SymbolKind::Function,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async: false,
        is_test: is_go_test(&name),
        decorators: Vec::new(),
        signature: None,
    });
    edges.push(contains_edge(file_path, &qualified_name));
}

/// Extract the receiver type name from a method_declaration's receiver parameter list.
/// Handles: `(r Foo)`, `(r *Foo)`, `(Foo)`, `(*Foo)`.
fn extract_receiver_type(node: Node, source: &[u8]) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    // receiver is a parameter_list; find the first parameter_declaration
    let mut cursor = receiver.walk();
    for child in receiver.children(&mut cursor) {
        if child.kind() == "parameter_declaration" {
            // The type field holds the receiver type
            if let Some(type_node) = child.child_by_field_name("type") {
                return Some(unwrap_pointer_type(type_node, source));
            }
        }
    }
    None
}

/// Unwrap pointer type (`*Foo` → `Foo`) or return the type identifier as-is.
fn unwrap_pointer_type(node: Node, source: &[u8]) -> String {
    match node.kind() {
        "pointer_type" => {
            // pointer_type has one child: the pointed-to type
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    return node_text(child, source).to_string();
                }
            }
            node_text(node, source).trim_start_matches('*').to_string()
        }
        "qualified_type" => {
            // e.g. pkg.Bar — use the full text
            node_text(node, source).to_string()
        }
        _ => node_text(node, source).to_string(),
    }
}

/// Extract a `method_declaration` node.
fn extract_method(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };
    let method_name = node_text(name_node, source).to_string();
    if method_name.is_empty() {
        return;
    }

    let receiver_type = extract_receiver_type(node, source);
    let (visibility, is_exported) = go_visibility(&method_name);

    let qualified_name = if let Some(ref rt) = receiver_type {
        format!("{file_path}::{rt}.{method_name}")
    } else {
        format!("{file_path}::{method_name}")
    };

    symbols.push(SymbolNode {
        name: method_name.clone(),
        qualified_name: qualified_name.clone(),
        kind: SymbolKind::Method,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async: false,
        is_test: is_go_test(&method_name),
        decorators: Vec::new(),
        signature: None,
    });
    edges.push(contains_edge(file_path, &qualified_name));

    // ChildOf edge: method → receiver type
    if let Some(ref rt) = receiver_type {
        let struct_qn = format!("{file_path}::{rt}");
        edges.push(Edge {
            kind: EdgeKind::ChildOf,
            source: qualified_name,
            target: struct_qn,
            metadata: None,
        });
    }
}

/// Extract a `type_declaration` node (may contain multiple `type_spec` or `type_alias`).
fn extract_type_declaration(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "type_spec" => {
                extract_type_spec(source, file_path, child, symbols, edges);
            }
            "type_alias" => {
                extract_type_alias(source, file_path, child, symbols, edges);
            }
            _ => {}
        }
    }
}

/// Extract a `type_spec` (struct, interface, or named type).
fn extract_type_spec(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };
    let name = node_text(name_node, source).to_string();
    if name.is_empty() {
        return;
    }

    let type_node = node.child_by_field_name("type");
    let kind = match type_node.as_ref().map(|n| n.kind()) {
        Some("struct_type") => SymbolKind::Struct,
        Some("interface_type") => SymbolKind::Interface,
        _ => SymbolKind::TypeAlias,
    };

    let qualified_name = format!("{file_path}::{name}");
    let (visibility, is_exported) = go_visibility(&name);

    symbols.push(SymbolNode {
        name: name.clone(),
        qualified_name: qualified_name.clone(),
        kind,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    });
    edges.push(contains_edge(file_path, &qualified_name));

    // For structs: scan for embedding
    if let Some(struct_node) = type_node.filter(|n| n.kind() == "struct_type") {
        extract_struct_embeddings(source, file_path, &qualified_name, struct_node, edges);
    }
}

/// Extract a `type_alias` (type Alias = Other).
fn extract_type_alias(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };
    let name = node_text(name_node, source).to_string();
    if name.is_empty() {
        return;
    }

    let qualified_name = format!("{file_path}::{name}");
    let (visibility, is_exported) = go_visibility(&name);

    symbols.push(SymbolNode {
        name: name.clone(),
        qualified_name: qualified_name.clone(),
        kind: SymbolKind::TypeAlias,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    });
    edges.push(contains_edge(file_path, &qualified_name));
}

/// Scan struct fields for embedded types and emit Embeds edges.
/// An embedded field has no `name` field — only a `type` field.
fn extract_struct_embeddings(
    source: &[u8],
    file_path: &str,
    struct_qn: &str,
    struct_node: Node,
    edges: &mut Vec<Edge>,
) {
    let mut cursor = struct_node.walk();
    for child in struct_node.children(&mut cursor) {
        if child.kind() != "field_declaration_list" {
            continue;
        }
        let mut list_cursor = child.walk();
        for field in child.children(&mut list_cursor) {
            if field.kind() != "field_declaration" {
                continue;
            }
            // Embedded field: has a type but no name
            if field.child_by_field_name("name").is_none() {
                if let Some(type_node) = field.child_by_field_name("type") {
                    let embedded_name = unwrap_pointer_type(type_node, source);
                    if !embedded_name.is_empty() {
                        let embedded_qn = format!("{file_path}::{embedded_name}");
                        edges.push(Edge {
                            kind: EdgeKind::Embeds,
                            source: struct_qn.to_string(),
                            target: embedded_qn,
                            metadata: None,
                        });
                    }
                }
            }
        }
    }
}

/// Extract a `const_declaration` node.
fn extract_const_declaration(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "const_spec" {
            extract_spec_names(source, file_path, child, SymbolKind::Const, symbols, edges);
        }
    }
}

/// Extract a `var_declaration` node.
fn extract_var_declaration(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "var_spec" {
            extract_spec_names(source, file_path, child, SymbolKind::Variable, symbols, edges);
        }
    }
}

/// Extract names from a `const_spec` or `var_spec` node.
/// Both have one or more `name` children (identifiers).
fn extract_spec_names(
    source: &[u8],
    file_path: &str,
    node: Node,
    kind: SymbolKind,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // Names are direct identifier children with field name "name"
        if child.kind() == "identifier" {
            let name = node_text(child, source).to_string();
            if name.is_empty() {
                continue;
            }
            let qualified_name = format!("{file_path}::{name}");
            let (visibility, is_exported) = go_visibility(&name);

            symbols.push(SymbolNode {
                name: name.clone(),
                qualified_name: qualified_name.clone(),
                kind,
                location: node_location(file_path, node),
                visibility,
                is_exported,
                is_async: false,
                is_test: false,
                decorators: Vec::new(),
                signature: None,
            });
            edges.push(contains_edge(file_path, &qualified_name));
        }
    }
}

// ---------------------------------------------------------------------------
// Import extraction
// ---------------------------------------------------------------------------

/// Extract all import declarations from the root node.
fn extract_imports(source: &[u8], root: &Node) -> Vec<RawImport> {
    let mut imports = Vec::new();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if child.kind() == "import_declaration" {
            extract_import_declaration(source, child, &mut imports);
        }
    }

    imports
}

/// Parse a single `import_declaration` node. It may contain one or more `import_spec`.
fn extract_import_declaration(source: &[u8], node: Node, imports: &mut Vec<RawImport>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_spec" => {
                if let Some(imp) = parse_import_spec(source, child) {
                    imports.push(imp);
                }
            }
            "import_spec_list" => {
                let mut list_cursor = child.walk();
                for spec in child.children(&mut list_cursor) {
                    if spec.kind() == "import_spec" {
                        if let Some(imp) = parse_import_spec(source, spec) {
                            imports.push(imp);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Strip leading/trailing double-quote characters from a Go string literal.
fn strip_go_quotes(s: &str) -> &str {
    s.trim_matches('"')
}

/// Parse a single `import_spec` node into a `RawImport`.
fn parse_import_spec(source: &[u8], node: Node) -> Option<RawImport> {
    let path_node = node.child_by_field_name("path")?;
    let raw_path = node_text(path_node, source);
    let specifier = strip_go_quotes(raw_path).to_string();
    let line = node.start_position().row + 1;

    // Check the optional name field
    let name_node = node.child_by_field_name("name");

    let (is_side_effect, is_namespace, alias) = match name_node {
        Some(n) => match n.kind() {
            "blank_identifier" => (true, false, None),
            "dot" => (false, true, None),
            "package_identifier" => {
                let alias_text = node_text(n, source).to_string();
                (false, false, Some(alias_text))
            }
            _ => (false, false, None),
        },
        None => (false, false, None),
    };

    let names = if is_side_effect || is_namespace {
        Vec::new()
    } else {
        let pkg = specifier.split('/').next_back().unwrap_or(&specifier).to_string();
        vec![ImportName {
            name: pkg,
            alias,
            is_type: false,
        }]
    };

    Some(RawImport {
        specifier,
        names,
        is_type_only: false,
        is_side_effect,
        is_namespace,
        line,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use domain::model::{EdgeKind, SymbolKind, Visibility};

    fn parse_go(source: &str) -> ParseResult {
        let parser = GoParser::new();
        parser
            .parse(source.as_bytes(), Path::new("test.go"))
            .expect("parse failed")
    }

    // -----------------------------------------------------------------------
    // AC22: function declaration → Function symbol
    // -----------------------------------------------------------------------

    #[test]
    fn ac22_function_declaration_exported() {
        let result = parse_go("package main\n\nfunc Foo() {}");
        let sym = result.symbols.iter().find(|s| s.name == "Foo").unwrap();
        assert_eq!(sym.kind, SymbolKind::Function);
        assert_eq!(sym.visibility, Visibility::Public);
        assert!(sym.is_exported);
        assert!(!sym.is_test);
    }

    #[test]
    fn ac22_function_declaration_unexported() {
        let result = parse_go("package main\n\nfunc helper() {}");
        let sym = result.symbols.iter().find(|s| s.name == "helper").unwrap();
        assert_eq!(sym.kind, SymbolKind::Function);
        assert_eq!(sym.visibility, Visibility::Private);
        assert!(!sym.is_exported);
    }

    #[test]
    fn ac22_function_contains_edge() {
        let result = parse_go("package main\n\nfunc Foo() {}");
        let edge = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Contains && e.target == "test.go::Foo")
            .expect("Contains edge missing");
        assert_eq!(edge.source, "test.go");
    }

    // -----------------------------------------------------------------------
    // AC23: method declaration → Method symbol + ChildOf edge
    // -----------------------------------------------------------------------

    #[test]
    fn ac23_method_declaration_value_receiver() {
        let result = parse_go(
            "package main\n\ntype Bar struct {}\n\nfunc (r Bar) Method() {}",
        );
        let sym = result.symbols.iter().find(|s| s.name == "Method").unwrap();
        assert_eq!(sym.kind, SymbolKind::Method);
        assert_eq!(sym.qualified_name, "test.go::Bar.Method");
    }

    #[test]
    fn ac23_method_declaration_pointer_receiver() {
        let result = parse_go(
            "package main\n\ntype Bar struct {}\n\nfunc (r *Bar) Method() {}",
        );
        let sym = result.symbols.iter().find(|s| s.name == "Method").unwrap();
        assert_eq!(sym.kind, SymbolKind::Method);
        assert_eq!(sym.qualified_name, "test.go::Bar.Method");
    }

    #[test]
    fn ac23_method_child_of_edge() {
        let result = parse_go(
            "package main\n\ntype Bar struct {}\n\nfunc (r *Bar) Method() {}",
        );
        let child_of = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::ChildOf)
            .expect("ChildOf edge missing");
        assert_eq!(child_of.source, "test.go::Bar.Method");
        assert_eq!(child_of.target, "test.go::Bar");
    }

    // -----------------------------------------------------------------------
    // AC24: struct with embedding → Struct symbol + Embeds edge
    // -----------------------------------------------------------------------

    #[test]
    fn ac24_struct_embedding() {
        let result = parse_go(
            "package main\n\ntype Base struct {}\n\ntype Derived struct { Base }",
        );
        let sym = result
            .symbols
            .iter()
            .find(|s| s.name == "Derived")
            .unwrap();
        assert_eq!(sym.kind, SymbolKind::Struct);

        let embeds = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Embeds)
            .expect("Embeds edge missing");
        assert_eq!(embeds.source, "test.go::Derived");
        assert_eq!(embeds.target, "test.go::Base");
    }

    #[test]
    fn ac24_struct_pointer_embedding() {
        let result = parse_go(
            "package main\n\ntype Base struct {}\n\ntype Derived struct { *Base }",
        );
        let embeds = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Embeds)
            .expect("Embeds edge missing for pointer embedding");
        assert_eq!(embeds.target, "test.go::Base");
    }

    #[test]
    fn ac24_struct_no_embedding_for_named_field() {
        let result = parse_go(
            "package main\n\ntype Foo struct { bar string }",
        );
        let embeds_count = result
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Embeds)
            .count();
        assert_eq!(embeds_count, 0, "named fields must not produce Embeds edges");
    }

    // -----------------------------------------------------------------------
    // AC25: interface declaration → Interface symbol
    // -----------------------------------------------------------------------

    #[test]
    fn ac25_interface_declaration() {
        let result = parse_go("package main\n\ntype Baz interface { Method() }");
        let sym = result.symbols.iter().find(|s| s.name == "Baz").unwrap();
        assert_eq!(sym.kind, SymbolKind::Interface);
        assert!(sym.is_exported);
    }

    // -----------------------------------------------------------------------
    // AC26: side-effect import → is_side_effect = true
    // -----------------------------------------------------------------------

    #[test]
    fn ac26_side_effect_import() {
        let result = parse_go(
            "package main\n\nimport _ \"lib/pq\"\n\nfunc main() {}",
        );
        let imp = result
            .imports
            .iter()
            .find(|i| i.specifier == "lib/pq")
            .expect("import not found");
        assert!(imp.is_side_effect);
        assert!(imp.names.is_empty());
    }

    // -----------------------------------------------------------------------
    // AC27: dot import → is_namespace = true
    // -----------------------------------------------------------------------

    #[test]
    fn ac27_dot_import() {
        let result = parse_go(
            "package main\n\nimport . \"fmt\"\n\nfunc main() {}",
        );
        let imp = result
            .imports
            .iter()
            .find(|i| i.specifier == "fmt")
            .expect("import not found");
        assert!(imp.is_namespace);
        assert!(imp.names.is_empty());
    }

    // -----------------------------------------------------------------------
    // AC28: visibility rules (uppercase = Public, lowercase = Private)
    // -----------------------------------------------------------------------

    #[test]
    fn ac28_uppercase_is_public() {
        let result = parse_go("package main\n\nfunc PublicFunc() {}");
        let sym = result
            .symbols
            .iter()
            .find(|s| s.name == "PublicFunc")
            .unwrap();
        assert_eq!(sym.visibility, Visibility::Public);
        assert!(sym.is_exported);
    }

    #[test]
    fn ac28_lowercase_is_private() {
        let result = parse_go("package main\n\nfunc privateFunc() {}");
        let sym = result
            .symbols
            .iter()
            .find(|s| s.name == "privateFunc")
            .unwrap();
        assert_eq!(sym.visibility, Visibility::Private);
        assert!(!sym.is_exported);
    }

    // -----------------------------------------------------------------------
    // AC49: Invalid/empty source → no panic
    // -----------------------------------------------------------------------

    #[test]
    fn ac49_empty_source_no_panic() {
        let parser = GoParser::new();
        let result = parser.parse(b"", Path::new("empty.go"));
        assert!(result.is_ok());
        let r = result.unwrap();
        assert!(r.symbols.is_empty());
    }

    // -----------------------------------------------------------------------
    // AC50: Source with errors → partial extraction
    // -----------------------------------------------------------------------

    #[test]
    fn ac50_partial_extraction_on_errors() {
        let source = "package main\n\nfunc Valid() {}\nconst {{{;\nfunc AlsoValid() {}";
        let result = parse_go(source);
        // tree-sitter is fault-tolerant; at least some symbols extracted
        assert!(
            result.symbols.iter().any(|s| s.name == "Valid"),
            "should extract 'Valid' even with syntax errors"
        );
    }

    // -----------------------------------------------------------------------
    // Type alias
    // -----------------------------------------------------------------------

    #[test]
    fn type_alias_declaration() {
        let result = parse_go("package main\n\ntype Alias = string");
        let sym = result.symbols.iter().find(|s| s.name == "Alias").unwrap();
        assert_eq!(sym.kind, SymbolKind::TypeAlias);
    }

    // -----------------------------------------------------------------------
    // Const and Var declarations
    // -----------------------------------------------------------------------

    #[test]
    fn const_declaration() {
        let result = parse_go("package main\n\nconst X = 1");
        let sym = result.symbols.iter().find(|s| s.name == "X").unwrap();
        assert_eq!(sym.kind, SymbolKind::Const);
        assert!(sym.is_exported);
    }

    #[test]
    fn var_declaration() {
        let result = parse_go("package main\n\nvar y string");
        let sym = result.symbols.iter().find(|s| s.name == "y").unwrap();
        assert_eq!(sym.kind, SymbolKind::Variable);
        assert!(!sym.is_exported);
    }

    // -----------------------------------------------------------------------
    // Test function detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_function_detection() {
        let result = parse_go("package main\n\nfunc TestFoo(t *testing.T) {}");
        let sym = result
            .symbols
            .iter()
            .find(|s| s.name == "TestFoo")
            .unwrap();
        assert!(sym.is_test);
    }

    #[test]
    fn bench_function_detection() {
        let result = parse_go("package main\n\nfunc BenchmarkFoo(b *testing.B) {}");
        let sym = result
            .symbols
            .iter()
            .find(|s| s.name == "BenchmarkFoo")
            .unwrap();
        assert!(sym.is_test);
    }

    #[test]
    fn regular_function_not_test() {
        let result = parse_go("package main\n\nfunc Process() {}");
        let sym = result.symbols.iter().find(|s| s.name == "Process").unwrap();
        assert!(!sym.is_test);
    }

    // -----------------------------------------------------------------------
    // Import: regular import, alias import, grouped imports
    // -----------------------------------------------------------------------

    #[test]
    fn regular_import() {
        let result = parse_go(
            "package main\n\nimport \"fmt\"\n\nfunc main() {}",
        );
        let imp = result
            .imports
            .iter()
            .find(|i| i.specifier == "fmt")
            .expect("fmt import not found");
        assert!(!imp.is_side_effect);
        assert!(!imp.is_namespace);
    }

    #[test]
    fn aliased_import() {
        let result = parse_go(
            "package main\n\nimport myfmt \"fmt\"\n\nfunc main() {}",
        );
        let imp = result
            .imports
            .iter()
            .find(|i| i.specifier == "fmt")
            .expect("fmt import not found");
        assert!(!imp.is_side_effect);
        assert!(!imp.is_namespace);
        assert_eq!(imp.names[0].alias, Some("myfmt".to_string()));
    }

    #[test]
    fn grouped_imports() {
        let source = r#"package main

import (
    "fmt"
    "os"
)

func main() {}
"#;
        let result = parse_go(source);
        assert!(result.imports.iter().any(|i| i.specifier == "fmt"));
        assert!(result.imports.iter().any(|i| i.specifier == "os"));
    }

    #[test]
    fn grouped_import_with_side_effect_and_alias() {
        let source = r#"package main

import (
    "fmt"
    _ "lib/pq"
    . "math"
    myfmt "fmt"
)

func main() {}
"#;
        let result = parse_go(source);
        assert!(result.imports.iter().any(|i| i.specifier == "lib/pq" && i.is_side_effect));
        assert!(result.imports.iter().any(|i| i.specifier == "math" && i.is_namespace));
        assert!(result.imports.iter().any(|i| i.specifier == "fmt"));
    }

    // -----------------------------------------------------------------------
    // Location
    // -----------------------------------------------------------------------

    #[test]
    fn location_populated() {
        let result = parse_go("package main\n\nfunc Foo() {}");
        let sym = result.symbols.iter().find(|s| s.name == "Foo").unwrap();
        assert_eq!(sym.location.file.to_string_lossy(), "test.go");
        assert!(sym.location.line_start >= 1);
    }

    // -----------------------------------------------------------------------
    // Qualified name format
    // -----------------------------------------------------------------------

    #[test]
    fn qualified_name_function() {
        let result = parse_go("package main\n\nfunc Foo() {}");
        let sym = result.symbols.iter().find(|s| s.name == "Foo").unwrap();
        assert_eq!(sym.qualified_name, "test.go::Foo");
    }

    #[test]
    fn qualified_name_method() {
        let result = parse_go(
            "package main\n\ntype MyStruct struct {}\n\nfunc (s *MyStruct) DoWork() {}",
        );
        let sym = result
            .symbols
            .iter()
            .find(|s| s.name == "DoWork")
            .unwrap();
        assert_eq!(sym.qualified_name, "test.go::MyStruct.DoWork");
    }

    // -----------------------------------------------------------------------
    // Integration test
    // -----------------------------------------------------------------------

    #[test]
    fn integration_multi_construct_file() {
        let source = r#"package main

import (
    "fmt"
    _ "lib/pq"
    . "math"
)

const MaxItems = 100

var globalVar string

type Animal struct {
    Name string
}

type Dog struct {
    Animal
    Breed string
}

type Speaker interface {
    Speak() string
}

type MyAlias = string

func NewDog(name string) *Dog {
    return &Dog{}
}

func (d *Dog) Bark() string {
    return "Woof"
}

func TestDog(t interface{}) {}
"#;
        let result = parse_go(source);

        // Symbols
        assert!(result.symbols.iter().any(|s| s.name == "MaxItems" && s.kind == SymbolKind::Const));
        assert!(result.symbols.iter().any(|s| s.name == "globalVar" && s.kind == SymbolKind::Variable));
        assert!(result.symbols.iter().any(|s| s.name == "Animal" && s.kind == SymbolKind::Struct));
        assert!(result.symbols.iter().any(|s| s.name == "Dog" && s.kind == SymbolKind::Struct));
        assert!(result.symbols.iter().any(|s| s.name == "Speaker" && s.kind == SymbolKind::Interface));
        assert!(result.symbols.iter().any(|s| s.name == "MyAlias" && s.kind == SymbolKind::TypeAlias));
        assert!(result.symbols.iter().any(|s| s.name == "NewDog" && s.kind == SymbolKind::Function));
        assert!(result.symbols.iter().any(|s| s.name == "Bark" && s.kind == SymbolKind::Method));

        // Test detection
        let test_sym = result.symbols.iter().find(|s| s.name == "TestDog").unwrap();
        assert!(test_sym.is_test);

        // Visibility
        let max = result.symbols.iter().find(|s| s.name == "MaxItems").unwrap();
        assert_eq!(max.visibility, Visibility::Public);
        let gvar = result.symbols.iter().find(|s| s.name == "globalVar").unwrap();
        assert_eq!(gvar.visibility, Visibility::Private);

        // Edges
        let child_of = result.edges.iter().filter(|e| e.kind == EdgeKind::ChildOf).count();
        assert!(child_of >= 1, "expected at least one ChildOf edge");

        let embeds = result.edges.iter().filter(|e| e.kind == EdgeKind::Embeds).count();
        assert_eq!(embeds, 1, "expected one Embeds edge for Dog embedding Animal");

        let contains = result.edges.iter().filter(|e| e.kind == EdgeKind::Contains).count();
        assert!(contains >= 5, "expected many Contains edges");

        // Imports
        assert!(result.imports.iter().any(|i| i.specifier == "fmt"));
        assert!(result.imports.iter().any(|i| i.specifier == "lib/pq" && i.is_side_effect));
        assert!(result.imports.iter().any(|i| i.specifier == "math" && i.is_namespace));
    }
}
