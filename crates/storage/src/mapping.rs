use domain::error::{CodeGraphError, Result};
use domain::model::*;

pub(crate) fn map_rusqlite_error(e: rusqlite::Error) -> CodeGraphError {
    CodeGraphError::Storage(e.to_string())
}

// ---------------------------------------------------------------------------
// Language
// ---------------------------------------------------------------------------

pub(crate) fn language_to_str(l: &Language) -> &'static str {
    match l {
        Language::TypeScript => "TypeScript",
        Language::JavaScript => "JavaScript",
        Language::Rust => "Rust",
        Language::Python => "Python",
        Language::Go => "Go",
    }
}

pub(crate) fn language_from_str(s: &str) -> Result<Language> {
    match s {
        "TypeScript" => Ok(Language::TypeScript),
        "JavaScript" => Ok(Language::JavaScript),
        "Rust" => Ok(Language::Rust),
        "Python" => Ok(Language::Python),
        "Go" => Ok(Language::Go),
        _ => Err(CodeGraphError::Storage(format!("unknown language: {s}"))),
    }
}

// ---------------------------------------------------------------------------
// SymbolKind
// ---------------------------------------------------------------------------

pub(crate) fn symbol_kind_to_str(k: &SymbolKind) -> &'static str {
    match k {
        SymbolKind::Function => "function",
        SymbolKind::Class => "class",
        SymbolKind::Interface => "interface",
        SymbolKind::Struct => "struct",
        SymbolKind::Trait => "trait",
        SymbolKind::Enum => "enum",
        SymbolKind::TypeAlias => "type_alias",
        SymbolKind::Method => "method",
        SymbolKind::Property => "property",
        SymbolKind::Const => "const",
        SymbolKind::Macro => "macro",
        SymbolKind::Variable => "variable",
        SymbolKind::Component => "component",
        SymbolKind::Test => "test",
    }
}

pub(crate) fn symbol_kind_from_str(s: &str) -> Result<SymbolKind> {
    match s {
        "function" => Ok(SymbolKind::Function),
        "class" => Ok(SymbolKind::Class),
        "interface" => Ok(SymbolKind::Interface),
        "struct" => Ok(SymbolKind::Struct),
        "trait" => Ok(SymbolKind::Trait),
        "enum" => Ok(SymbolKind::Enum),
        "type_alias" => Ok(SymbolKind::TypeAlias),
        "method" => Ok(SymbolKind::Method),
        "property" => Ok(SymbolKind::Property),
        "const" => Ok(SymbolKind::Const),
        "macro" => Ok(SymbolKind::Macro),
        "variable" => Ok(SymbolKind::Variable),
        "component" => Ok(SymbolKind::Component),
        "test" => Ok(SymbolKind::Test),
        _ => Err(CodeGraphError::Storage(format!("unknown symbol kind: {s}"))),
    }
}

// ---------------------------------------------------------------------------
// EdgeKind
// ---------------------------------------------------------------------------

pub(crate) fn edge_kind_to_str(k: &EdgeKind) -> &'static str {
    match k {
        EdgeKind::Contains => "contains",
        EdgeKind::ChildOf => "child_of",
        EdgeKind::Calls => "calls",
        EdgeKind::ImportsFrom => "imports_from",
        EdgeKind::Extends => "extends",
        EdgeKind::Implements => "implements",
        EdgeKind::TestedBy => "tested_by",
        EdgeKind::DependsOn => "depends_on",
        EdgeKind::BarrelReExportAll => "barrel_re_export_all",
        EdgeKind::ConditionalImport => "conditional_import",
        EdgeKind::SideEffectImport => "side_effect_import",
        EdgeKind::DotImport => "dot_import",
        EdgeKind::HasDecorator => "has_decorator",
        EdgeKind::Embeds => "embeds",
        EdgeKind::TypeReference => "type_reference",
        EdgeKind::ReExport => "re_export",
    }
}

pub(crate) fn edge_kind_from_str(s: &str) -> Result<EdgeKind> {
    match s {
        "contains" => Ok(EdgeKind::Contains),
        "child_of" => Ok(EdgeKind::ChildOf),
        "calls" => Ok(EdgeKind::Calls),
        "imports_from" => Ok(EdgeKind::ImportsFrom),
        "extends" => Ok(EdgeKind::Extends),
        "implements" => Ok(EdgeKind::Implements),
        "tested_by" => Ok(EdgeKind::TestedBy),
        "depends_on" => Ok(EdgeKind::DependsOn),
        "barrel_re_export_all" => Ok(EdgeKind::BarrelReExportAll),
        "conditional_import" => Ok(EdgeKind::ConditionalImport),
        "side_effect_import" => Ok(EdgeKind::SideEffectImport),
        "dot_import" => Ok(EdgeKind::DotImport),
        "has_decorator" => Ok(EdgeKind::HasDecorator),
        "embeds" => Ok(EdgeKind::Embeds),
        "type_reference" => Ok(EdgeKind::TypeReference),
        "re_export" => Ok(EdgeKind::ReExport),
        _ => Err(CodeGraphError::Storage(format!("unknown edge kind: {s}"))),
    }
}

// ---------------------------------------------------------------------------
// Visibility
// ---------------------------------------------------------------------------

pub(crate) fn visibility_to_str(v: &Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Private => "private",
        Visibility::Crate => "crate",
    }
}

pub(crate) fn visibility_from_str(s: &str) -> Result<Visibility> {
    match s {
        "public" => Ok(Visibility::Public),
        "private" => Ok(Visibility::Private),
        "crate" => Ok(Visibility::Crate),
        _ => Err(CodeGraphError::Storage(format!("unknown visibility: {s}"))),
    }
}

// ---------------------------------------------------------------------------
// NonParsedKind
// ---------------------------------------------------------------------------

pub(crate) fn non_parsed_kind_to_str(k: &NonParsedKind) -> &'static str {
    match k {
        NonParsedKind::Doc => "doc",
        NonParsedKind::Config => "config",
        NonParsedKind::CI => "ci",
        NonParsedKind::Asset => "asset",
        NonParsedKind::Other => "other",
    }
}

pub(crate) fn non_parsed_kind_from_str(s: &str) -> Result<NonParsedKind> {
    match s {
        "doc" => Ok(NonParsedKind::Doc),
        "config" => Ok(NonParsedKind::Config),
        "ci" => Ok(NonParsedKind::CI),
        "asset" => Ok(NonParsedKind::Asset),
        "other" => Ok(NonParsedKind::Other),
        _ => Err(CodeGraphError::Storage(format!("unknown non-parsed kind: {s}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_roundtrip_all_variants() {
        let variants = [
            Language::TypeScript, Language::JavaScript,
            Language::Rust, Language::Python, Language::Go,
        ];
        for v in &variants {
            let s = language_to_str(v);
            let back = language_from_str(s).unwrap();
            assert_eq!(*v, back, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn symbol_kind_roundtrip_all_variants() {
        let variants = [
            SymbolKind::Function, SymbolKind::Class, SymbolKind::Interface,
            SymbolKind::Struct, SymbolKind::Trait, SymbolKind::Enum,
            SymbolKind::TypeAlias, SymbolKind::Method, SymbolKind::Property,
            SymbolKind::Const, SymbolKind::Macro, SymbolKind::Variable,
            SymbolKind::Component, SymbolKind::Test,
        ];
        for v in &variants {
            let s = symbol_kind_to_str(v);
            let back = symbol_kind_from_str(s).unwrap();
            assert_eq!(*v, back, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn edge_kind_roundtrip_all_16_variants() {
        let variants = [
            EdgeKind::Contains, EdgeKind::ChildOf, EdgeKind::Calls,
            EdgeKind::ImportsFrom, EdgeKind::Extends, EdgeKind::Implements,
            EdgeKind::TestedBy, EdgeKind::DependsOn, EdgeKind::BarrelReExportAll,
            EdgeKind::ConditionalImport, EdgeKind::SideEffectImport,
            EdgeKind::DotImport, EdgeKind::HasDecorator, EdgeKind::Embeds,
            EdgeKind::TypeReference, EdgeKind::ReExport,
        ];
        assert_eq!(variants.len(), 16);
        for v in &variants {
            let s = edge_kind_to_str(v);
            let back = edge_kind_from_str(s).unwrap();
            assert_eq!(*v, back, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn visibility_roundtrip_all_variants() {
        for v in &[Visibility::Public, Visibility::Private, Visibility::Crate] {
            let s = visibility_to_str(v);
            let back = visibility_from_str(s).unwrap();
            assert_eq!(*v, back);
        }
    }

    #[test]
    fn non_parsed_kind_roundtrip_all_variants() {
        let variants = [
            NonParsedKind::Doc, NonParsedKind::Config, NonParsedKind::CI,
            NonParsedKind::Asset, NonParsedKind::Other,
        ];
        for v in &variants {
            let s = non_parsed_kind_to_str(v);
            let back = non_parsed_kind_from_str(s).unwrap();
            assert_eq!(*v, back);
        }
    }

    #[test]
    fn unknown_string_returns_storage_error() {
        assert!(language_from_str("Haskell").is_err());
        assert!(symbol_kind_from_str("Widget").is_err());
        assert!(edge_kind_from_str("MagicLink").is_err());
        assert!(visibility_from_str("Protected").is_err());
        assert!(non_parsed_kind_from_str("Unknown").is_err());
    }
}
