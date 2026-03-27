use super::blast_radius::compute_blast_radius;
use super::change_detection::find_affected_symbols;
use crate::model::*;
use crate::traversal::InMemoryGraph;

/// Combine change detection with blast radius for a full diff impact report.
pub fn compute_diff_impact(
    graph: &InMemoryGraph,
    hunks: &[DiffHunk],
    symbols: &[SymbolNode],
    max_depth: usize,
) -> DiffImpactReport {
    let changed = find_affected_symbols(hunks, symbols);
    let targets: Vec<ImpactTarget> = changed
        .iter()
        .map(|s| ImpactTarget::Symbol(s.qualified_name.clone()))
        .collect();
    let impact = compute_blast_radius(graph, &targets, max_depth, Confidence::Structural);
    DiffImpactReport {
        changed_symbols: changed,
        impact,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traversal::InMemoryGraph;

    #[test]
    fn diff_impact_non_overlapping_returns_empty() {
        let graph = InMemoryGraph::from_edges(vec![]);
        let symbols: Vec<SymbolNode> = vec![];
        let hunks = vec![DiffHunk {
            file: "src/a.rs".into(),
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 1,
        }];
        let report = compute_diff_impact(&graph, &hunks, &symbols, 3);
        assert!(report.changed_symbols.is_empty());
        assert!(report.impact.affected.is_empty());
    }

    #[test]
    fn diff_impact_overlapping_hunk_produces_full_report() {
        let symbols = vec![SymbolNode {
            name: "foo".into(),
            qualified_name: "a.rs::foo".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "a.rs".into(),
                line_start: 10,
                line_end: 20,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }];
        let edges = vec![Edge {
            kind: EdgeKind::Calls,
            source: "a.rs::foo".into(),
            target: "b.rs::bar".into(),
            metadata: None,
        }];
        let graph = InMemoryGraph::from_edges(edges);
        let hunks = vec![DiffHunk {
            file: "a.rs".into(),
            old_start: 15,
            old_count: 3,
            new_start: 15,
            new_count: 3,
        }];
        let report = compute_diff_impact(&graph, &hunks, &symbols, 3);
        assert_eq!(report.changed_symbols.len(), 1);
        assert_eq!(report.changed_symbols[0].name, "foo");
        assert!(report
            .impact
            .affected
            .iter()
            .any(|n| n.qualified_name == "b.rs::bar"));
    }
}
