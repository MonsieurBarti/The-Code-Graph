use crate::model::*;
use crate::traversal::InMemoryGraph;

/// Compute blast radius from a set of impact targets.
/// For Symbol targets: BFS forward from the symbol.
/// For File targets: BFS forward from the file node (which typically has Contains edges to symbols).
pub fn compute_blast_radius(
    graph: &InMemoryGraph,
    targets: &[ImpactTarget],
    max_depth: usize,
    min_confidence: Confidence,
) -> ImpactReport {
    let mut all_affected = Vec::new();

    for target in targets {
        let start = match target {
            ImpactTarget::Symbol(s) => s.as_str(),
            ImpactTarget::File(p) => p.to_str().unwrap_or_default(),
        };
        let results = graph.bfs_filtered(start, Direction::Forward, max_depth, min_confidence);
        for r in results {
            all_affected.push(AffectedNode {
                qualified_name: r.node,
                depth: r.depth,
                confidence: r.edge_kind.confidence(),
                path: r.path,
            });
        }
    }

    ImpactReport {
        targets: targets.to_vec(),
        affected: all_affected,
        depth: max_depth,
        min_confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::traversal::InMemoryGraph;

    #[test]
    fn blast_radius_from_single_symbol() {
        let edges = vec![
            Edge {
                kind: EdgeKind::Calls,
                source: "a::foo".into(),
                target: "b::bar".into(),
                metadata: None,
            },
            Edge {
                kind: EdgeKind::Calls,
                source: "b::bar".into(),
                target: "c::baz".into(),
                metadata: None,
            },
        ];
        let graph = InMemoryGraph::from_edges(edges);
        let targets = vec![ImpactTarget::Symbol("a::foo".into())];
        let report = compute_blast_radius(&graph, &targets, 3, Confidence::Structural);
        assert!(!report.affected.is_empty());
        assert!(report.affected.iter().any(|n| n.qualified_name == "b::bar"));
        assert!(report.affected.iter().any(|n| n.qualified_name == "c::baz"));
    }

    #[test]
    fn blast_radius_from_file_target() {
        let edges = vec![
            Edge {
                kind: EdgeKind::Contains,
                source: "a.rs".into(),
                target: "a.rs::foo".into(),
                metadata: None,
            },
            Edge {
                kind: EdgeKind::Calls,
                source: "a.rs::foo".into(),
                target: "b.rs::bar".into(),
                metadata: None,
            },
        ];
        let graph = InMemoryGraph::from_edges(edges);
        let targets = vec![ImpactTarget::File("a.rs".into())];
        let report = compute_blast_radius(&graph, &targets, 3, Confidence::Structural);
        assert!(!report.targets.is_empty());
    }
}
