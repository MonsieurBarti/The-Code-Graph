use std::collections::HashMap;
use std::path::Path;

use crate::model::{Edge, EdgeKind, SymbolKind, SymbolNode};

// ---------------------------------------------------------------------------
// RRF Fusion
// ---------------------------------------------------------------------------

/// Reciprocal Rank Fusion: merge multiple ranked lists into a single ranking.
/// `k` is the smoothing constant (typically 60).
pub fn rrf_merge(lists: &[Vec<(String, f64)>], k: usize) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();
    for list in lists {
        for (rank, (qn, _)) in list.iter().enumerate() {
            *scores.entry(qn.clone()).or_default() += 1.0 / (k + rank + 1) as f64;
        }
    }
    let mut merged: Vec<_> = scores.into_iter().collect();
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    merged
}

// ---------------------------------------------------------------------------
// Text representation
// ---------------------------------------------------------------------------

/// Build a natural-language text representation of a symbol for embedding.
pub fn symbol_to_text(sym: &SymbolNode, edges: &[Edge]) -> String {
    let mut parts = vec![kind_to_str(sym.kind).to_string(), sym.name.clone()];
    parts.push(format!("in {}", file_stem(&sym.location.file)));
    if let Some(sig) = &sym.signature {
        parts.push(format!("signature: {sig}"));
    }
    let calls: Vec<_> = edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls && e.source == sym.qualified_name)
        .take(3)
        .map(|e| short_name(&e.target))
        .collect();
    if !calls.is_empty() {
        parts.push(format!("calls {}", calls.join(", ")));
    }
    let callers: Vec<_> = edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls && e.target == sym.qualified_name)
        .take(3)
        .map(|e| short_name(&e.source))
        .collect();
    if !callers.is_empty() {
        parts.push(format!("called by {}", callers.join(", ")));
    }
    parts.join(", ")
}

fn kind_to_str(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "Function",
        SymbolKind::Class => "Class",
        SymbolKind::Interface => "Interface",
        SymbolKind::Struct => "Struct",
        SymbolKind::Trait => "Trait",
        SymbolKind::Enum => "Enum",
        SymbolKind::TypeAlias => "TypeAlias",
        SymbolKind::Method => "Method",
        SymbolKind::Property => "Property",
        SymbolKind::Const => "Const",
        SymbolKind::Macro => "Macro",
        SymbolKind::Variable => "Variable",
        SymbolKind::Component => "Component",
        SymbolKind::Test => "Test",
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn short_name(qualified: &str) -> String {
    qualified
        .rsplit("::")
        .next()
        .unwrap_or(qualified)
        .to_string()
}

// ---------------------------------------------------------------------------
// Kind boosting
// ---------------------------------------------------------------------------

/// A boost hint derived from query shape heuristics.
pub struct KindBoost {
    pub kind: SymbolKind,
    pub multiplier: f64,
}

/// Detect likely symbol kinds from query shape to boost relevance scores.
/// Returns an empty vec when the query is a qualified name (contains `::`)
/// because qualified names already pinpoint the exact symbol.
pub fn detect_kind_boost(query: &str) -> Vec<KindBoost> {
    let mut boosts = Vec::new();
    if query.contains("::") {
        return boosts; // qualified name pattern — no kind boost
    }
    let first = query.chars().next().unwrap_or('a');
    if first.is_uppercase() && !query.contains('_') {
        // PascalCase → likely a type-level symbol
        for kind in [SymbolKind::Struct, SymbolKind::Trait, SymbolKind::Interface] {
            boosts.push(KindBoost {
                kind,
                multiplier: 1.5,
            });
        }
    } else if query.contains('_') && query.chars().all(|c| c.is_lowercase() || c == '_') {
        // snake_case → likely a function or method
        for kind in [SymbolKind::Function, SymbolKind::Method] {
            boosts.push(KindBoost {
                kind,
                multiplier: 1.5,
            });
        }
    }
    boosts
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, EdgeKind, Location, SymbolKind, SymbolNode, Visibility};
    use std::path::PathBuf;

    // ------------------------------------------------------------------
    // rrf_merge
    // ------------------------------------------------------------------

    #[test]
    fn rrf_merge_single_list() {
        let lists = vec![vec![("a".into(), 1.0), ("b".into(), 0.5)]];
        let merged = rrf_merge(&lists, 60);
        assert_eq!(merged[0].0, "a");
        assert!(merged[0].1 > merged[1].1);
    }

    #[test]
    fn rrf_merge_two_lists_boosts_overlap() {
        let l1 = vec![("a".into(), 1.0), ("b".into(), 0.5)];
        let l2 = vec![("b".into(), 1.0), ("c".into(), 0.5)];
        let merged = rrf_merge(&[l1, l2], 60);
        assert_eq!(merged[0].0, "b"); // appears in both lists
    }

    #[test]
    fn rrf_merge_empty_lists() {
        let merged = rrf_merge(&[], 60);
        assert!(merged.is_empty());
    }

    // ------------------------------------------------------------------
    // symbol_to_text
    // ------------------------------------------------------------------

    #[test]
    fn symbol_to_text_basic() {
        let sym = make_symbol(
            "foo",
            SymbolKind::Function,
            "src/lib.rs",
            Some("fn foo(x: i32) -> bool".into()),
        );
        let text = symbol_to_text(&sym, &[]);
        assert!(text.contains("Function"));
        assert!(text.contains("foo"));
        assert!(text.contains("lib"));
        assert!(text.contains("signature:"));
    }

    #[test]
    fn symbol_to_text_with_edges() {
        let sym = make_symbol("foo", SymbolKind::Function, "src/lib.rs", None);
        let edges = vec![
            make_call_edge("mod::foo", "mod::bar"), // foo calls bar
            make_call_edge("mod::baz", "mod::foo"), // baz calls foo
        ];
        let text = symbol_to_text(&sym, &edges);
        assert!(text.contains("calls bar"));
        assert!(text.contains("called by baz"));
    }

    // ------------------------------------------------------------------
    // detect_kind_boost
    // ------------------------------------------------------------------

    #[test]
    fn detect_kind_boost_pascal_case() {
        let boosts = detect_kind_boost("AuthService");
        assert!(!boosts.is_empty());
        assert!(boosts.iter().any(|b| b.kind == SymbolKind::Struct));
        assert!(boosts
            .iter()
            .all(|b| (b.multiplier - 1.5).abs() < f64::EPSILON));
    }

    #[test]
    fn detect_kind_boost_snake_case() {
        let boosts = detect_kind_boost("validate_token");
        assert!(!boosts.is_empty());
        assert!(boosts.iter().any(|b| b.kind == SymbolKind::Function));
    }

    #[test]
    fn detect_kind_boost_qualified() {
        let boosts = detect_kind_boost("auth::validate");
        assert!(boosts.is_empty()); // :: pattern has no kind boost, just exact match
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    fn make_symbol(name: &str, kind: SymbolKind, file: &str, sig: Option<String>) -> SymbolNode {
        SymbolNode {
            name: name.to_string(),
            qualified_name: format!("mod::{name}"),
            kind,
            location: Location {
                file: PathBuf::from(file),
                line_start: 1,
                line_end: 5,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: sig,
        }
    }

    fn make_call_edge(source: &str, target: &str) -> Edge {
        Edge {
            kind: EdgeKind::Calls,
            source: source.to_string(),
            target: target.to_string(),
            metadata: None,
        }
    }
}
