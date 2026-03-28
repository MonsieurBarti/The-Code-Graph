use crate::analysis::flow::brandes_betweenness;
use crate::model::{Confidence, Edge, SymbolNode};
use std::collections::{HashMap, HashSet};

/// Compute criticality scores by delegating to brandes_betweenness.
/// Returns normalized betweenness centrality [0.0, 1.0] per symbol.
pub fn compute_criticality_scores(symbols: &[SymbolNode], edges: &[Edge]) -> HashMap<String, f64> {
    let nodes: HashSet<String> = symbols.iter().map(|s| s.qualified_name.clone()).collect();
    brandes_betweenness(&nodes, edges)
}

/// Compute coupling scores via degree centrality over non-structural edges.
/// Only counts edges where both endpoints are in the symbol set.
/// Excludes structural edges (Contains, ChildOf, HasDecorator, TestedBy).
/// Normalizes by max_degree. Returns 0.0 for all if max_degree == 0.
pub fn compute_coupling_scores(symbols: &[SymbolNode], edges: &[Edge]) -> HashMap<String, f64> {
    let symbol_set: HashSet<&str> = symbols.iter().map(|s| s.qualified_name.as_str()).collect();

    // Filter to non-structural edges where both endpoints are symbols
    let relevant_edges: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.kind.confidence() != Confidence::Structural)
        .filter(|e| {
            symbol_set.contains(e.source.as_str()) && symbol_set.contains(e.target.as_str())
        })
        .collect();

    // Count degrees
    let mut degrees: HashMap<&str, usize> = HashMap::new();
    for name in &symbol_set {
        degrees.insert(name, 0);
    }
    for edge in &relevant_edges {
        *degrees.entry(edge.source.as_str()).or_default() += 1; // out-degree
        *degrees.entry(edge.target.as_str()).or_default() += 1; // in-degree
    }

    let max_degree = degrees.values().copied().max().unwrap_or(0);
    if max_degree == 0 {
        return symbols
            .iter()
            .map(|s| (s.qualified_name.clone(), 0.0))
            .collect();
    }

    symbols
        .iter()
        .map(|s| {
            let deg = degrees.get(s.qualified_name.as_str()).copied().unwrap_or(0);
            (s.qualified_name.clone(), deg as f64 / max_degree as f64)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, EdgeKind, Location, SymbolKind, SymbolNode, Visibility};

    fn make_symbol(name: &str, file: &str) -> SymbolNode {
        SymbolNode {
            name: name.split("::").last().unwrap_or(name).into(),
            qualified_name: name.into(),
            kind: SymbolKind::Function,
            location: Location {
                file: file.into(),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }
    }

    fn make_edge(source: &str, target: &str, kind: EdgeKind) -> Edge {
        Edge {
            kind,
            source: source.into(),
            target: target.into(),
            metadata: None,
        }
    }

    #[test]
    fn test_criticality_delegates_to_brandes() {
        // A -> B -> C chain: B should have highest betweenness
        let symbols = vec![
            make_symbol("a::A", "a.rs"),
            make_symbol("b::B", "b.rs"),
            make_symbol("c::C", "c.rs"),
        ];
        let edges = vec![
            make_edge("a::A", "b::B", EdgeKind::Calls),
            make_edge("b::B", "c::C", EdgeKind::Calls),
        ];
        let scores = compute_criticality_scores(&symbols, &edges);
        assert!(scores.get("b::B").unwrap_or(&0.0) >= scores.get("a::A").unwrap_or(&0.0));
        assert!(scores.get("b::B").unwrap_or(&0.0) >= scores.get("c::C").unwrap_or(&0.0));
    }

    #[test]
    fn test_coupling_excludes_structural_edges() {
        let symbols = vec![make_symbol("a::A", "a.rs"), make_symbol("b::B", "b.rs")];
        let edges = vec![
            make_edge("a::A", "b::B", EdgeKind::Calls), // non-structural
            make_edge("a::A", "b::B", EdgeKind::Contains), // structural — should be excluded
        ];
        let scores = compute_coupling_scores(&symbols, &edges);
        // Only the Calls edge should count: A has out-degree 1, B has in-degree 1
        // max_degree = 1, both get 1.0
        assert!((scores["a::A"] - 1.0).abs() < f64::EPSILON);
        assert!((scores["b::B"] - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coupling_both_endpoints_must_be_symbols() {
        let symbols = vec![make_symbol("a::A", "a.rs")];
        let edges = vec![
            make_edge("a::A", "file.rs", EdgeKind::Calls), // target not in symbol set
        ];
        let scores = compute_coupling_scores(&symbols, &edges);
        // Edge filtered out because "file.rs" is not a symbol
        assert!((scores["a::A"]).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coupling_max_degree_zero() {
        let symbols = vec![make_symbol("a::A", "a.rs"), make_symbol("b::B", "b.rs")];
        let edges: Vec<Edge> = vec![];
        let scores = compute_coupling_scores(&symbols, &edges);
        assert!((scores["a::A"]).abs() < f64::EPSILON);
        assert!((scores["b::B"]).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coupling_normalization() {
        // A calls B and C; B calls C. A has degree 2 (out), B has degree 2 (out+in), C has degree 2 (in+in)
        let symbols = vec![
            make_symbol("a::A", "a.rs"),
            make_symbol("b::B", "b.rs"),
            make_symbol("c::C", "c.rs"),
        ];
        let edges = vec![
            make_edge("a::A", "b::B", EdgeKind::Calls),
            make_edge("a::A", "c::C", EdgeKind::Calls),
            make_edge("b::B", "c::C", EdgeKind::Calls),
        ];
        let scores = compute_coupling_scores(&symbols, &edges);
        // A: out=2, in=0, degree=2
        // B: out=1, in=1, degree=2
        // C: out=0, in=2, degree=2
        // max_degree=2, all get 1.0
        assert!((scores["a::A"] - 1.0).abs() < f64::EPSILON);
        assert!((scores["b::B"] - 1.0).abs() < f64::EPSILON);
        assert!((scores["c::C"] - 1.0).abs() < f64::EPSILON);
    }
}
