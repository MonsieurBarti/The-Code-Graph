use std::cell::RefCell;
use std::path::Path;

use tree_sitter::{Node, Parser};
use tree_sitter_language::LanguageFn;

use domain::error::CodeGraphError;
use domain::model::{Edge, EdgeKind, Language, Location, SymbolKind, SymbolNode, Visibility};

use crate::{LanguageParser, ParseResult};

thread_local! {
    static RUST_PARSER: RefCell<Parser> = RefCell::new(Parser::new());
}

/// Parser for Rust (.rs) files.
pub struct RustParser {
    lang: LanguageFn,
}

impl RustParser {
    pub fn new() -> Self {
        Self {
            lang: tree_sitter_rust::LANGUAGE,
        }
    }
}

impl Default for RustParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for RustParser {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn file_extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn parse(&self, source: &[u8], path: &Path) -> domain::error::Result<ParseResult> {
        let lang: tree_sitter::Language = self.lang.into();

        RUST_PARSER.with(|parser_cell| {
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

    // Collect preceding attribute_item texts to attach to the next item.
    // In tree-sitter-rust, `#[test]` etc. are sibling nodes before the fn.
    let mut pending_attrs: Vec<String> = Vec::new();

    for child in root.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "attribute_item" => {
                // Collect attribute text — attached to the next named item
                if let Ok(text) = child.utf8_text(source) {
                    pending_attrs.push(text.to_string());
                }
                continue; // do not reset pending_attrs below
            }
            "function_item" => {
                if let Some(sym) = extract_function(source, &file_path, child, None, &pending_attrs) {
                    edges.push(contains_edge(&file_path, &sym.qualified_name));
                    symbols.push(sym);
                }
            }
            "struct_item" => {
                if let Some(sym) = extract_struct(source, &file_path, child) {
                    edges.push(contains_edge(&file_path, &sym.qualified_name));
                    symbols.push(sym);
                }
            }
            "enum_item" => {
                if let Some(sym) = extract_enum(source, &file_path, child) {
                    edges.push(contains_edge(&file_path, &sym.qualified_name));
                    symbols.push(sym);
                }
            }
            "trait_item" => {
                if let Some(sym) = extract_trait(source, &file_path, child) {
                    edges.push(contains_edge(&file_path, &sym.qualified_name));
                    symbols.push(sym);
                }
            }
            "type_item" => {
                if let Some(sym) = extract_type_alias(source, &file_path, child) {
                    edges.push(contains_edge(&file_path, &sym.qualified_name));
                    symbols.push(sym);
                }
            }
            "const_item" => {
                if let Some(sym) = extract_const(source, &file_path, child) {
                    edges.push(contains_edge(&file_path, &sym.qualified_name));
                    symbols.push(sym);
                }
            }
            "static_item" => {
                if let Some(sym) = extract_static(source, &file_path, child) {
                    edges.push(contains_edge(&file_path, &sym.qualified_name));
                    symbols.push(sym);
                }
            }
            "macro_definition" => {
                if let Some(sym) = extract_macro(source, &file_path, child) {
                    edges.push(contains_edge(&file_path, &sym.qualified_name));
                    symbols.push(sym);
                }
            }
            "impl_item" => {
                extract_impl(source, &file_path, child, &mut symbols, &mut edges);
            }
            _ => {}
        }
        // Consumed by the item above — reset for next
        pending_attrs.clear();
    }

    Ok(ParseResult {
        symbols,
        edges,
        imports: Vec::new(),
        exports: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Symbol extraction helpers
// ---------------------------------------------------------------------------

/// Extract a top-level or method function_item node.
/// When `owner_name` is Some, this is a method inside an impl block.
/// `preceding_attrs` are sibling `attribute_item` texts collected before this node.
fn extract_function(
    source: &[u8],
    file_path: &str,
    node: Node,
    owner_name: Option<&str>,
    preceding_attrs: &[String],
) -> Option<SymbolNode> {
    let name = node_name(source, node)?;
    let qualified_name = match owner_name {
        Some(owner) => format!("{file_path}::{owner}.{name}"),
        None => format!("{file_path}::{name}"),
    };
    let visibility = extract_visibility(source, node);
    let is_exported = visibility == Visibility::Public;
    let is_async = is_async_fn(source, node);
    let is_test = attrs_contain_test(preceding_attrs);
    let signature = build_rust_signature(source, node);
    let kind = if owner_name.is_some() {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };

    Some(SymbolNode {
        name,
        qualified_name,
        kind,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async,
        is_test,
        decorators: Vec::new(),
        signature,
    })
}

fn extract_struct(source: &[u8], file_path: &str, node: Node) -> Option<SymbolNode> {
    let name = node_name(source, node)?;
    let qualified_name = format!("{file_path}::{name}");
    let visibility = extract_visibility(source, node);
    let is_exported = visibility == Visibility::Public;

    Some(SymbolNode {
        name,
        qualified_name,
        kind: SymbolKind::Struct,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    })
}

fn extract_enum(source: &[u8], file_path: &str, node: Node) -> Option<SymbolNode> {
    let name = node_name(source, node)?;
    let qualified_name = format!("{file_path}::{name}");
    let visibility = extract_visibility(source, node);
    let is_exported = visibility == Visibility::Public;

    Some(SymbolNode {
        name,
        qualified_name,
        kind: SymbolKind::Enum,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    })
}

fn extract_trait(source: &[u8], file_path: &str, node: Node) -> Option<SymbolNode> {
    let name = node_name(source, node)?;
    let qualified_name = format!("{file_path}::{name}");
    let visibility = extract_visibility(source, node);
    let is_exported = visibility == Visibility::Public;

    Some(SymbolNode {
        name,
        qualified_name,
        kind: SymbolKind::Trait,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    })
}

fn extract_type_alias(source: &[u8], file_path: &str, node: Node) -> Option<SymbolNode> {
    let name = node_name(source, node)?;
    let qualified_name = format!("{file_path}::{name}");
    let visibility = extract_visibility(source, node);
    let is_exported = visibility == Visibility::Public;

    Some(SymbolNode {
        name,
        qualified_name,
        kind: SymbolKind::TypeAlias,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    })
}

fn extract_const(source: &[u8], file_path: &str, node: Node) -> Option<SymbolNode> {
    let name = node_name(source, node)?;
    let qualified_name = format!("{file_path}::{name}");
    let visibility = extract_visibility(source, node);
    let is_exported = visibility == Visibility::Public;

    Some(SymbolNode {
        name,
        qualified_name,
        kind: SymbolKind::Const,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    })
}

fn extract_static(source: &[u8], file_path: &str, node: Node) -> Option<SymbolNode> {
    let name = node_name(source, node)?;
    let qualified_name = format!("{file_path}::{name}");
    let visibility = extract_visibility(source, node);
    let is_exported = visibility == Visibility::Public;

    Some(SymbolNode {
        name,
        qualified_name,
        kind: SymbolKind::Variable,
        location: node_location(file_path, node),
        visibility,
        is_exported,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    })
}

fn extract_macro(source: &[u8], file_path: &str, node: Node) -> Option<SymbolNode> {
    let name = node_name(source, node)?;
    let qualified_name = format!("{file_path}::{name}");

    Some(SymbolNode {
        name,
        qualified_name,
        kind: SymbolKind::Macro,
        location: node_location(file_path, node),
        visibility: Visibility::Public, // macro_rules! is always public-ish
        is_exported: false,
        is_async: false,
        is_test: false,
        decorators: Vec::new(),
        signature: None,
    })
}

/// Extract methods from an `impl_item` and generate edges.
fn extract_impl(
    source: &[u8],
    file_path: &str,
    node: Node,
    symbols: &mut Vec<SymbolNode>,
    edges: &mut Vec<Edge>,
) {
    // The implementing type is in the "type" field
    let type_name = match node.child_by_field_name("type") {
        Some(t) => match t.utf8_text(source) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        },
        None => return,
    };

    // Check if this is `impl Trait for Type`
    let trait_name = node
        .child_by_field_name("trait")
        .and_then(|t| t.utf8_text(source).ok())
        .map(|s| s.to_string());

    // If it's a trait impl, emit an Implements edge
    if let Some(ref tname) = trait_name {
        let trait_qn = format!("{file_path}::{tname}");
        let type_qn = format!("{file_path}::{type_name}");
        edges.push(Edge {
            kind: EdgeKind::Implements,
            source: type_qn,
            target: trait_qn,
            metadata: None,
        });
    }

    // Walk the body (declaration_list) for methods
    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return,
    };

    let mut body_cursor = body.walk();
    let mut pending_attrs: Vec<String> = Vec::new();
    for member in body.children(&mut body_cursor) {
        if !member.is_named() {
            continue;
        }
        if member.kind() == "attribute_item" {
            if let Ok(text) = member.utf8_text(source) {
                pending_attrs.push(text.to_string());
            }
            continue;
        }
        if member.kind() == "function_item" {
            if let Some(sym) = extract_function(source, file_path, member, Some(&type_name), &pending_attrs) {
                edges.push(contains_edge(file_path, &sym.qualified_name));
                // ChildOf edge: method → impl type
                let type_qn = format!("{file_path}::{type_name}");
                edges.push(Edge {
                    kind: EdgeKind::ChildOf,
                    source: sym.qualified_name.clone(),
                    target: type_qn,
                    metadata: None,
                });
                symbols.push(sym);
            }
        }
        pending_attrs.clear();
    }
}

// ---------------------------------------------------------------------------
// Low-level helpers
// ---------------------------------------------------------------------------

/// Get the text of the "name" field child.
fn node_name(source: &[u8], node: Node) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// Build a Location from a tree-sitter node (1-based lines).
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

/// Create a Contains edge from file path to a qualified name.
fn contains_edge(file_path: &str, qualified_name: &str) -> Edge {
    Edge {
        kind: EdgeKind::Contains,
        source: file_path.to_string(),
        target: qualified_name.to_string(),
        metadata: None,
    }
}

/// Extract Visibility from the first `visibility_modifier` child, if present.
fn extract_visibility(source: &[u8], node: Node) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = child.utf8_text(source).unwrap_or("");
            return if text.contains("crate") {
                Visibility::Crate
            } else {
                // "pub" (possibly with path like "pub(super)") → Public
                Visibility::Public
            };
        }
    }
    Visibility::Private
}

/// Check if a `function_item` node has the `async` modifier.
///
/// In tree-sitter-rust the `async` keyword lives inside a named `function_modifiers`
/// child (e.g. `async unsafe extern`), not as a bare unnamed token.
fn is_async_fn(source: &[u8], node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_modifiers" {
            let text = child.utf8_text(source).unwrap_or("");
            return text.split_whitespace().any(|w| w == "async");
        }
    }
    false
}

/// Check if any of the provided attribute texts contains "test".
///
/// Attributes in tree-sitter-rust are sibling `attribute_item` nodes that precede
/// the function in the parent's children list. Callers collect them and pass the
/// texts here.
fn attrs_contain_test(attrs: &[String]) -> bool {
    attrs.iter().any(|a| a.contains("test"))
}

/// Build a simplified function signature from `parameters` and optional `return_type`.
fn build_rust_signature(source: &[u8], node: Node) -> Option<String> {
    let params = node
        .child_by_field_name("parameters")
        .and_then(|n| n.utf8_text(source).ok())?;

    let return_type = node
        .child_by_field_name("return_type")
        .and_then(|n| n.utf8_text(source).ok());

    Some(match return_type {
        Some(ret) => format!("{params} {ret}"),
        None => params.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rust(source: &str) -> ParseResult {
        let parser = RustParser::new();
        parser
            .parse(source.as_bytes(), Path::new("test.rs"))
            .expect("parse failed")
    }

    // -----------------------------------------------------------------------
    // AC6: fn foo() {} → Function symbol, name="foo"
    // -----------------------------------------------------------------------

    #[test]
    fn ac6_function_item_extracts_function_symbol() {
        let result = parse_rust("fn foo() {}");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "foo");
        assert_eq!(sym.kind, SymbolKind::Function);
    }

    #[test]
    fn function_qualified_name_uses_file_path() {
        let result = parse_rust("fn foo() {}");
        assert_eq!(result.symbols[0].qualified_name, "test.rs::foo");
    }

    #[test]
    fn function_contains_edge_emitted() {
        let result = parse_rust("fn foo() {}");
        let contains: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .collect();
        assert_eq!(contains.len(), 1);
        assert_eq!(contains[0].source, "test.rs");
        assert_eq!(contains[0].target, "test.rs::foo");
    }

    // -----------------------------------------------------------------------
    // AC7: struct Bar {} → Struct symbol
    // -----------------------------------------------------------------------

    #[test]
    fn ac7_struct_item_extracts_struct_symbol() {
        let result = parse_rust("struct Bar {}");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "Bar");
        assert_eq!(sym.kind, SymbolKind::Struct);
    }

    // -----------------------------------------------------------------------
    // AC8: impl Foo { fn bar(&self) {} } → Method + ChildOf edge
    // -----------------------------------------------------------------------

    #[test]
    fn ac8_impl_item_extracts_method_symbol() {
        let result = parse_rust("struct Foo; impl Foo { fn bar(&self) {} }");
        let method = result
            .symbols
            .iter()
            .find(|s| s.name == "bar")
            .expect("method 'bar' not found");
        assert_eq!(method.kind, SymbolKind::Method);
        assert_eq!(method.qualified_name, "test.rs::Foo.bar");
    }

    #[test]
    fn ac8_impl_method_has_child_of_edge() {
        let result = parse_rust("struct Foo; impl Foo { fn bar(&self) {} }");
        let child_of = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::ChildOf)
            .expect("ChildOf edge not found");
        assert_eq!(child_of.source, "test.rs::Foo.bar");
        assert_eq!(child_of.target, "test.rs::Foo");
    }

    #[test]
    fn impl_method_also_has_contains_edge() {
        let result = parse_rust("struct Foo; impl Foo { fn bar(&self) {} }");
        let contains: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains && e.target == "test.rs::Foo.bar")
            .collect();
        assert_eq!(contains.len(), 1);
    }

    // -----------------------------------------------------------------------
    // AC9: trait Baz {} → Trait symbol
    // -----------------------------------------------------------------------

    #[test]
    fn ac9_trait_item_extracts_trait_symbol() {
        let result = parse_rust("trait Baz {}");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "Baz");
        assert_eq!(sym.kind, SymbolKind::Trait);
    }

    // -----------------------------------------------------------------------
    // AC10: enum Color { Red, Green } → Enum symbol
    // -----------------------------------------------------------------------

    #[test]
    fn ac10_enum_item_extracts_enum_symbol() {
        let result = parse_rust("enum Color { Red, Green }");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "Color");
        assert_eq!(sym.kind, SymbolKind::Enum);
    }

    // -----------------------------------------------------------------------
    // AC14: Visibility extraction
    // -----------------------------------------------------------------------

    #[test]
    fn ac14_pub_fn_is_public() {
        let result = parse_rust("pub fn visible() {}");
        let sym = &result.symbols[0];
        assert_eq!(sym.visibility, Visibility::Public);
        assert!(sym.is_exported);
    }

    #[test]
    fn ac14_pub_crate_fn_is_crate() {
        let result = parse_rust("pub(crate) fn crate_fn() {}");
        let sym = &result.symbols[0];
        assert_eq!(sym.visibility, Visibility::Crate);
        assert!(!sym.is_exported);
    }

    #[test]
    fn ac14_private_fn_is_private() {
        let result = parse_rust("fn private_fn() {}");
        let sym = &result.symbols[0];
        assert_eq!(sym.visibility, Visibility::Private);
        assert!(!sym.is_exported);
    }

    #[test]
    fn pub_struct_is_exported() {
        let result = parse_rust("pub struct MyStruct {}");
        let sym = &result.symbols[0];
        assert_eq!(sym.visibility, Visibility::Public);
        assert!(sym.is_exported);
    }

    // -----------------------------------------------------------------------
    // AC49: Invalid/empty source → no panic
    // -----------------------------------------------------------------------

    #[test]
    fn ac49_empty_source_does_not_panic() {
        let parser = RustParser::new();
        let result = parser.parse(b"", Path::new("empty.rs"));
        assert!(result.is_ok());
        let r = result.unwrap();
        assert!(r.symbols.is_empty());
        assert!(r.edges.is_empty());
    }

    // -----------------------------------------------------------------------
    // AC50: Source with errors → partial extraction
    // -----------------------------------------------------------------------

    #[test]
    fn ac50_partial_extraction_from_broken_source() {
        let source = r#"
fn valid() {}
fn broken( {{{
fn also_valid() {}
"#;
        let parser = RustParser::new();
        let result = parser
            .parse(source.as_bytes(), Path::new("broken.rs"))
            .expect("should not error on syntax errors");
        // tree-sitter does best-effort — at least valid() should be found
        assert!(
            result.symbols.iter().any(|s| s.name == "valid"),
            "should find 'valid' function in broken source"
        );
    }

    // -----------------------------------------------------------------------
    // Trait impl → Implements edge
    // -----------------------------------------------------------------------

    #[test]
    fn trait_impl_emits_implements_edge() {
        let source = "trait Display {} struct Foo; impl Display for Foo {}";
        let result = parse_rust(source);
        let implements = result
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Implements)
            .expect("Implements edge not found");
        assert_eq!(implements.source, "test.rs::Foo");
        assert_eq!(implements.target, "test.rs::Display");
    }

    // -----------------------------------------------------------------------
    // Additional symbol kinds
    // -----------------------------------------------------------------------

    #[test]
    fn type_alias_is_extracted() {
        let result = parse_rust("type MyAlias = u32;");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "MyAlias");
        assert_eq!(result.symbols[0].kind, SymbolKind::TypeAlias);
    }

    #[test]
    fn const_item_is_extracted() {
        let result = parse_rust("const MAX: u32 = 100;");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "MAX");
        assert_eq!(result.symbols[0].kind, SymbolKind::Const);
    }

    #[test]
    fn static_item_is_extracted_as_variable() {
        let result = parse_rust(r#"static GREETING: &str = "hello";"#);
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "GREETING");
        assert_eq!(result.symbols[0].kind, SymbolKind::Variable);
    }

    #[test]
    fn macro_definition_is_extracted() {
        let result = parse_rust("macro_rules! my_macro { () => {} }");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "my_macro");
        assert_eq!(result.symbols[0].kind, SymbolKind::Macro);
    }

    // -----------------------------------------------------------------------
    // is_async
    // -----------------------------------------------------------------------

    #[test]
    fn async_fn_is_flagged() {
        let result = parse_rust("async fn fetch() {}");
        assert!(result.symbols[0].is_async);
    }

    #[test]
    fn sync_fn_is_not_async() {
        let result = parse_rust("fn sync_fn() {}");
        assert!(!result.symbols[0].is_async);
    }

    // -----------------------------------------------------------------------
    // is_test via #[test] attribute
    // -----------------------------------------------------------------------

    #[test]
    fn test_attribute_sets_is_test() {
        let result = parse_rust("#[test]\nfn my_test() {}");
        assert!(result.symbols[0].is_test);
    }

    #[test]
    fn no_test_attribute_is_not_test() {
        let result = parse_rust("fn regular_fn() {}");
        assert!(!result.symbols[0].is_test);
    }

    // -----------------------------------------------------------------------
    // Signature
    // -----------------------------------------------------------------------

    #[test]
    fn function_signature_includes_params_and_return_type() {
        let result = parse_rust("fn add(a: i32, b: i32) -> i32 { a + b }");
        let sig = result.symbols[0].signature.as_ref().expect("no signature");
        assert!(sig.contains("a: i32"));
        assert!(sig.contains("b: i32"));
        assert!(sig.contains("i32")); // return type
    }

    #[test]
    fn function_signature_without_return_type() {
        let result = parse_rust("fn greet(name: &str) {}");
        let sig = result.symbols[0].signature.as_ref().expect("no signature");
        assert!(sig.contains("name: &str"));
    }

    // -----------------------------------------------------------------------
    // Location
    // -----------------------------------------------------------------------

    #[test]
    fn location_is_one_based_line_numbers() {
        let result = parse_rust("fn foo() {}");
        let loc = &result.symbols[0].location;
        assert_eq!(loc.file.to_string_lossy(), "test.rs");
        assert_eq!(loc.line_start, 1);
        assert!(loc.line_end >= 1);
    }

    // -----------------------------------------------------------------------
    // Multiple top-level items
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_top_level_items_all_extracted() {
        let source = r#"
fn foo() {}
struct Bar {}
enum Baz { A }
trait Qux {}
"#;
        let result = parse_rust(source);
        assert_eq!(result.symbols.len(), 4);
        assert!(result.symbols.iter().any(|s| s.name == "foo" && s.kind == SymbolKind::Function));
        assert!(result.symbols.iter().any(|s| s.name == "Bar" && s.kind == SymbolKind::Struct));
        assert!(result.symbols.iter().any(|s| s.name == "Baz" && s.kind == SymbolKind::Enum));
        assert!(result.symbols.iter().any(|s| s.name == "Qux" && s.kind == SymbolKind::Trait));
        let contains_count = result.edges.iter().filter(|e| e.kind == EdgeKind::Contains).count();
        assert_eq!(contains_count, 4);
    }

    // -----------------------------------------------------------------------
    // impl with multiple methods
    // -----------------------------------------------------------------------

    #[test]
    fn impl_with_multiple_methods() {
        let source = r#"
struct Counter;
impl Counter {
    fn new() -> Self { Counter }
    fn increment(&mut self) {}
    fn value(&self) -> u32 { 0 }
}
"#;
        let result = parse_rust(source);
        let methods: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert_eq!(methods.len(), 3);
        let child_of_count = result.edges.iter().filter(|e| e.kind == EdgeKind::ChildOf).count();
        assert_eq!(child_of_count, 3);
    }

    // -----------------------------------------------------------------------
    // Integration: full real-world snippet
    // -----------------------------------------------------------------------

    #[test]
    fn integration_realistic_module() {
        let source = r#"
use std::fmt;

pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    pub fn distance(&self, other: &Point) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

pub trait Shape {
    fn area(&self) -> f64;
}

pub enum Color {
    Red,
    Green,
    Blue,
}

pub const MAX_POINTS: usize = 1000;

pub async fn fetch_data() -> Vec<Point> {
    vec![]
}

#[test]
fn test_distance() {
    let a = Point { x: 0.0, y: 0.0 };
    let b = Point { x: 3.0, y: 4.0 };
    assert_eq!(a.distance(&b), 5.0);
}
"#;
        let result = parse_rust(source);

        // Symbols
        assert!(result.symbols.iter().any(|s| s.name == "Point" && s.kind == SymbolKind::Struct));
        assert!(result.symbols.iter().any(|s| s.name == "Shape" && s.kind == SymbolKind::Trait));
        assert!(result.symbols.iter().any(|s| s.name == "Color" && s.kind == SymbolKind::Enum));
        assert!(result.symbols.iter().any(|s| s.name == "MAX_POINTS" && s.kind == SymbolKind::Const));
        assert!(result.symbols.iter().any(|s| s.name == "fetch_data" && s.is_async));
        assert!(result.symbols.iter().any(|s| s.name == "test_distance" && s.is_test));

        // Methods from impl Point
        assert!(result.symbols.iter().any(|s| s.name == "new" && s.kind == SymbolKind::Method && s.qualified_name == "test.rs::Point.new"));
        assert!(result.symbols.iter().any(|s| s.name == "distance" && s.kind == SymbolKind::Method));

        // Method from impl Display for Point
        assert!(result.symbols.iter().any(|s| s.name == "fmt" && s.kind == SymbolKind::Method));

        // Visibility
        let point_sym = result.symbols.iter().find(|s| s.name == "Point").unwrap();
        assert_eq!(point_sym.visibility, Visibility::Public);
        assert!(point_sym.is_exported);

        // Implements edge for Display
        let implements = result
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Implements)
            .collect::<Vec<_>>();
        assert!(!implements.is_empty(), "should have at least one Implements edge");

        // ChildOf edges
        let child_of_count = result.edges.iter().filter(|e| e.kind == EdgeKind::ChildOf).count();
        assert!(child_of_count >= 3, "expected at least 3 ChildOf edges, got {child_of_count}");
    }
}
