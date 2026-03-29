use crate::analysis::flow::{brandes_betweenness, detect_entry_points, enumerate_flows};
use crate::error::Result;
use crate::model::*;
use crate::ports::GraphStore;
use std::collections::{HashMap, HashSet, VecDeque};

pub struct FlowUseCase<S> {
    store: S,
}

impl<S: GraphStore> FlowUseCase<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Full flow analysis: detect entry points, enumerate flows, compute criticality.
    pub fn analyze(&self, config: &FlowConfig) -> Result<FlowAnalysis> {
        let symbols = self.store.all_symbols()?;
        let edges = self.store.all_edges()?;

        let entry_points = detect_entry_points(&symbols, &edges, config);
        let flows = enumerate_flows(&entry_points, &edges, config);

        let nodes: HashSet<String> = symbols.iter().map(|s| s.qualified_name.clone()).collect();
        let betweenness = brandes_betweenness(&nodes, &edges);

        let entry_set: HashSet<&str> = entry_points
            .iter()
            .map(|e| e.qualified_name.as_str())
            .collect();

        // Count flows per node
        let mut flow_counts: HashMap<String, usize> = HashMap::new();
        for flow in &flows {
            for node in &flow.path {
                *flow_counts.entry(node.clone()).or_default() += 1;
            }
        }

        let mut criticality: Vec<CriticalityScore> = betweenness
            .iter()
            .map(|(name, &score)| CriticalityScore {
                qualified_name: name.clone(),
                betweenness: score,
                flow_count: flow_counts.get(name).copied().unwrap_or(0),
                is_entry_point: entry_set.contains(name.as_str()),
            })
            .collect();
        criticality.sort_by(|a, b| {
            b.betweenness
                .partial_cmp(&a.betweenness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let stats = FlowStats {
            total_entry_points: entry_points.len(),
            total_flows: flows.len(),
            max_depth: flows.iter().map(|f| f.depth).max().unwrap_or(0),
            avg_depth: if flows.is_empty() {
                0.0
            } else {
                flows.iter().map(|f| f.depth as f64).sum::<f64>() / flows.len() as f64
            },
        };

        Ok(FlowAnalysis {
            entry_points,
            flows,
            criticality,
            stats,
        })
    }

    /// Find flows passing through a specific symbol (optimized: backward BFS first).
    pub fn flows_through(
        &self,
        qualified_name: &str,
        config: &FlowConfig,
    ) -> Result<Vec<ExecutionFlow>> {
        let symbols = self.store.all_symbols()?;
        let edges = self.store.all_edges()?;
        let entry_points = detect_entry_points(&symbols, &edges, config);

        // Backward BFS from target through HIGH-CONFIDENCE edges only
        let high_edges: Vec<&Edge> = edges
            .iter()
            .filter(|e| e.kind.confidence() == Confidence::High)
            .collect();
        let mut reachable_entries = HashSet::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(qualified_name.to_string());
        visited.insert(qualified_name.to_string());
        while let Some(node) = queue.pop_front() {
            if entry_points.iter().any(|ep| ep.qualified_name == node) {
                reachable_entries.insert(node.clone());
            }
            for edge in &high_edges {
                if edge.target == node && !visited.contains(&edge.source) {
                    visited.insert(edge.source.clone());
                    queue.push_back(edge.source.clone());
                }
            }
        }

        // DFS only from reachable entry points, filter to paths containing target
        let filtered_entries: Vec<EntryPoint> = entry_points
            .into_iter()
            .filter(|ep| reachable_entries.contains(&ep.qualified_name))
            .collect();
        let all_flows = enumerate_flows(&filtered_entries, &edges, config);
        Ok(all_flows
            .into_iter()
            .filter(|f| f.path.contains(&qualified_name.to_string()))
            .collect())
    }

    /// Get criticality scores sorted descending by betweenness.
    pub fn criticality(&self) -> Result<Vec<CriticalityScore>> {
        let analysis = self.analyze(&FlowConfig::default())?;
        Ok(analysis.criticality)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryGraphStore;

    fn build_store() -> InMemoryGraphStore {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(SymbolNode {
            name: "main".into(),
            qualified_name: "src/main.rs::main".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/main.rs".into(),
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
        });
        store.insert_symbol(SymbolNode {
            name: "connect".into(),
            qualified_name: "src/db.rs::connect".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/db.rs".into(),
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
        });
        store.insert_edge(Edge {
            kind: EdgeKind::Calls,
            source: "src/main.rs::main".into(),
            target: "src/db.rs::connect".into(),
            metadata: None,
        });
        store
    }

    #[test]
    fn analyze_returns_flows_and_criticality() {
        let store = build_store();
        let uc = FlowUseCase::new(store);
        let analysis = uc.analyze(&FlowConfig::default()).unwrap();
        assert!(!analysis.entry_points.is_empty());
        assert!(!analysis.flows.is_empty());
        assert!(!analysis.criticality.is_empty());
    }

    #[test]
    fn flows_through_filters_correctly() {
        let store = build_store();
        let uc = FlowUseCase::new(store);
        let flows = uc
            .flows_through("src/db.rs::connect", &FlowConfig::default())
            .unwrap();
        for flow in &flows {
            assert!(flow.path.contains(&"src/db.rs::connect".to_string()));
        }
    }

    #[test]
    fn criticality_returns_sorted_scores() {
        let store = build_store();
        let uc = FlowUseCase::new(store);
        let scores = uc.criticality().unwrap();
        for w in scores.windows(2) {
            assert!(w[0].betweenness >= w[1].betweenness);
        }
    }

    #[test]
    fn flows_through_nonexistent_symbol_returns_empty() {
        let store = build_store();
        let uc = FlowUseCase::new(store);
        let flows = uc
            .flows_through("nonexistent::symbol", &FlowConfig::default())
            .unwrap();
        assert!(flows.is_empty());
    }

    #[test]
    fn flows_through_ignores_medium_confidence_reachability() {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(SymbolNode {
            name: "main".into(),
            qualified_name: "src/main.rs::main".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/main.rs".into(),
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
        });
        store.insert_symbol(SymbolNode {
            name: "util".into(),
            qualified_name: "src/util.rs::util".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/util.rs".into(),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Private,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        // Only Medium-confidence edge connecting them
        store.insert_edge(Edge {
            kind: EdgeKind::ImportsFrom,
            source: "src/main.rs::main".into(),
            target: "src/util.rs::util".into(),
            metadata: None,
        });
        let uc = FlowUseCase::new(store);
        let flows = uc
            .flows_through("src/util.rs::util", &FlowConfig::default())
            .unwrap();
        assert!(
            flows.is_empty(),
            "backward BFS must filter on High-confidence edges only"
        );
    }

    #[test]
    fn analyze_empty_graph_returns_zeros() {
        let store = InMemoryGraphStore::new();
        let uc = FlowUseCase::new(store);
        let analysis = uc.analyze(&FlowConfig::default()).unwrap();
        assert!(analysis.entry_points.is_empty());
        assert!(analysis.flows.is_empty());
        assert_eq!(analysis.stats.total_entry_points, 0);
        assert_eq!(analysis.stats.total_flows, 0);
    }
}
