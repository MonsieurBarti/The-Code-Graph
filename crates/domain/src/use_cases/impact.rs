use crate::error::Result;
use crate::model::*;
use crate::ports::GraphStore;
use crate::traversal::InMemoryGraph;
use crate::analysis::blast_radius::compute_blast_radius;
use crate::analysis::change_detection::find_affected_symbols;

pub struct ImpactUseCase<S> {
    store: S,
}

impl<S: GraphStore> ImpactUseCase<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn blast_radius(
        &self,
        targets: &[ImpactTarget],
        max_depth: usize,
        min_confidence: Confidence,
    ) -> Result<ImpactReport> {
        let edges = self.store.all_edges()?;
        let graph = InMemoryGraph::from_edges(edges);
        Ok(compute_blast_radius(&graph, targets, max_depth, min_confidence))
    }

    pub fn diff_impact(&self, hunks: &[DiffHunk], max_depth: usize) -> Result<DiffImpactReport> {
        let edges = self.store.all_edges()?;
        let symbols = self.store.all_symbols()?;
        let graph = InMemoryGraph::from_edges(edges);
        let changed = find_affected_symbols(hunks, &symbols);
        let targets: Vec<ImpactTarget> = changed.iter()
            .map(|s| ImpactTarget::Symbol(s.qualified_name.clone()))
            .collect();
        let impact = compute_blast_radius(&graph, &targets, max_depth, Confidence::Structural);
        Ok(DiffImpactReport { changed_symbols: changed, impact })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryGraphStore;
    use crate::model::*;

    #[test]
    fn blast_radius_with_mock_returns_transitive_closure() {
        let mut store = InMemoryGraphStore::new();
        store.insert_file(FileNode { path: "a.rs".into(), language: Language::Rust, hash: "h1".into() });
        store.insert_file(FileNode { path: "b.rs".into(), language: Language::Rust, hash: "h2".into() });
        store.insert_symbol(SymbolNode {
            name: "foo".into(), qualified_name: "a.rs::foo".into(),
            kind: SymbolKind::Function,
            location: Location { file: "a.rs".into(), line_start: 1, line_end: 10, col_start: 0, col_end: 0 },
            visibility: Visibility::Public,
            is_exported: true, is_async: false, is_test: false,
            decorators: vec![], signature: None,
        });
        store.insert_symbol(SymbolNode {
            name: "bar".into(), qualified_name: "b.rs::bar".into(),
            kind: SymbolKind::Function,
            location: Location { file: "b.rs".into(), line_start: 1, line_end: 10, col_start: 0, col_end: 0 },
            visibility: Visibility::Public,
            is_exported: true, is_async: false, is_test: false,
            decorators: vec![], signature: None,
        });
        store.insert_edge(Edge {
            kind: EdgeKind::Calls, source: "a.rs::foo".into(),
            target: "b.rs::bar".into(), metadata: None,
        });

        let uc = ImpactUseCase::new(store);
        let report = uc.blast_radius(
            &[ImpactTarget::Symbol("a.rs::foo".into())], 3, Confidence::Structural,
        ).unwrap();
        assert!(report.affected.iter().any(|n| n.qualified_name == "b.rs::bar"));
    }

    #[test]
    fn diff_impact_non_overlapping_returns_empty_report() {
        let store = InMemoryGraphStore::new();
        let uc = ImpactUseCase::new(store);
        let hunks = vec![DiffHunk {
            file: "nonexistent.rs".into(),
            old_start: 1, old_count: 1, new_start: 1, new_count: 1,
        }];
        let report = uc.diff_impact(&hunks, 3).unwrap();
        assert!(report.changed_symbols.is_empty());
    }

    #[test]
    fn diff_impact_overlapping_returns_affected_symbols() {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(SymbolNode {
            name: "foo".into(), qualified_name: "a.rs::foo".into(),
            kind: SymbolKind::Function,
            location: Location { file: "a.rs".into(), line_start: 10, line_end: 20, col_start: 0, col_end: 0 },
            visibility: Visibility::Public,
            is_exported: false, is_async: false, is_test: false,
            decorators: vec![], signature: None,
        });
        store.insert_edge(Edge {
            kind: EdgeKind::Calls, source: "a.rs::foo".into(),
            target: "b.rs::bar".into(), metadata: None,
        });

        let uc = ImpactUseCase::new(store);
        let hunks = vec![DiffHunk {
            file: "a.rs".into(), old_start: 15, old_count: 3, new_start: 15, new_count: 3,
        }];
        let report = uc.diff_impact(&hunks, 3).unwrap();
        assert_eq!(report.changed_symbols.len(), 1);
        assert_eq!(report.changed_symbols[0].name, "foo");
        assert!(report.impact.affected.iter().any(|n| n.qualified_name == "b.rs::bar"));
    }
}
