use std::path::{Path, PathBuf};

use syn::{File, Item, ItemFn, ItemStruct, ItemImpl, UseTree};

#[derive(Debug, Clone)]
pub struct ParsedFunction {
    pub name: String,
    pub line: usize,
    pub is_public: bool,
    pub is_async: bool,
}

#[derive(Debug, Clone)]
pub struct ParsedStruct {
    pub name: String,
    pub line: usize,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedImport {
    pub path: String,
    pub line: usize,
}

#[derive(Debug, Default)]
pub struct ParseResult {
    pub functions: Vec<ParsedFunction>,
    pub structs: Vec<ParsedStruct>,
    pub imports: Vec<ParsedImport>,
    pub errors: Vec<String>,
}

pub fn parse_rust_file(path: &Path) -> ParseResult {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return ParseResult { errors: vec![e.to_string()], ..Default::default() },
    };
    let ast: File = match syn::parse_str(&source) {
        Ok(f) => f,
        Err(e) => return ParseResult { errors: vec![e.to_string()], ..Default::default() },
    };
    let mut result = ParseResult::default();
    for item in &ast.items {
        match item {
            Item::Fn(f) => result.functions.push(ParsedFunction {
                name: f.sig.ident.to_string(),
                line: 0,
                is_public: matches!(f.vis, syn::Visibility::Public(_)),
                is_async: f.sig.asyncness.is_some(),
            }),
            Item::Struct(s) => result.structs.push(ParsedStruct {
                name: s.ident.to_string(),
                line: 0,
                fields: extract_field_names(s),
            }),
            Item::Use(u) => {
                let path_str = use_tree_to_string(&u.tree);
                result.imports.push(ParsedImport { path: path_str, line: 0 });
            }
            _ => {}
        }
    }
    result
}

fn extract_field_names(s: &ItemStruct) -> Vec<String> {
    match &s.fields {
        syn::Fields::Named(named) => named.fields.iter().map(|f| f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default()).collect(),
        _ => vec![],
    }
}

fn use_tree_to_string(tree: &UseTree) -> String {
    match tree {
        UseTree::Path(p) => format!("{}::{}", p.ident, use_tree_to_string(&p.tree)),
        UseTree::Name(n) => n.ident.to_string(),
        UseTree::Glob(_) => "*".to_string(),
        UseTree::Group(g) => g.items.iter().map(use_tree_to_string).collect::<Vec<_>>().join(", "),
        UseTree::Rename(r) => format!("{} as {}", r.ident, r.rename),
    }
}
