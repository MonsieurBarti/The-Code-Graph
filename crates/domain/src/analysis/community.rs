// Leiden community detection algorithm

use crate::model::{Edge, EdgeKind, SymbolNode};
use std::collections::HashMap;

#[allow(dead_code)]
fn is_high_confidence(kind: &EdgeKind) -> bool {
    matches!(
        kind,
        EdgeKind::Calls | EdgeKind::Extends | EdgeKind::Implements | EdgeKind::Embeds
    )
}

#[allow(dead_code)]
pub(crate) struct LeidenGraph {
    pub n: usize,
    pub neighbors: Vec<Vec<(usize, f64)>>,
    pub degree: Vec<f64>,
    pub total_weight: f64,
    pub node_to_index: HashMap<String, usize>,
    pub index_to_node: Vec<String>,
}

#[allow(dead_code)]
impl LeidenGraph {
    pub fn from_symbols_and_edges(symbols: &[SymbolNode], edges: &[Edge]) -> Self {
        let mut node_to_index = HashMap::new();
        let mut index_to_node = Vec::new();
        for s in symbols {
            let idx = index_to_node.len();
            node_to_index.insert(s.qualified_name.clone(), idx);
            index_to_node.push(s.qualified_name.clone());
        }
        let n = index_to_node.len();
        let mut neighbors: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        let mut edge_weights: HashMap<(usize, usize), f64> = HashMap::new();

        for e in edges {
            if !is_high_confidence(&e.kind) {
                continue;
            }
            let Some(&si) = node_to_index.get(&e.source) else {
                continue;
            };
            let Some(&ti) = node_to_index.get(&e.target) else {
                continue;
            };
            if si == ti {
                continue;
            }
            let (lo, hi) = if si < ti { (si, ti) } else { (ti, si) };
            *edge_weights.entry((lo, hi)).or_default() += 1.0;
        }

        let mut degree = vec![0.0; n];
        let mut total_weight = 0.0;
        for (&(u, v), &w) in &edge_weights {
            neighbors[u].push((v, w));
            neighbors[v].push((u, w));
            degree[u] += w;
            degree[v] += w;
            total_weight += w;
        }

        Self {
            n,
            neighbors,
            degree,
            total_weight,
            node_to_index,
            index_to_node,
        }
    }
}

#[allow(dead_code)]
pub(crate) struct Partition {
    pub community: Vec<usize>,
    pub community_weight: Vec<f64>,
}

#[allow(dead_code)]
impl Partition {
    pub fn singleton(n: usize) -> Self {
        Self {
            community: (0..n).collect(),
            community_weight: vec![0.0; n],
        }
    }

    pub fn singleton_with_graph(graph: &LeidenGraph) -> Self {
        Self {
            community: (0..graph.n).collect(),
            community_weight: graph.degree.clone(),
        }
    }

    pub fn move_node(&mut self, node: usize, target: usize, graph: &LeidenGraph) {
        let old = self.community[node];
        if old == target {
            return;
        }
        self.community_weight[old] -= graph.degree[node];
        self.community_weight[target] += graph.degree[node];
        self.community[node] = target;
    }

    pub fn distinct_communities(&self) -> std::collections::HashSet<usize> {
        self.community.iter().copied().collect()
    }
}

#[allow(dead_code)]
fn compute_modularity(graph: &LeidenGraph, partition: &Partition, gamma: f64) -> f64 {
    if graph.total_weight == 0.0 {
        return 0.0;
    }
    let m = graph.total_weight;
    let m2 = 2.0 * m;

    // Per-community: L_c (internal edge weight) and K_c (total degree)
    let max_c = partition.community.iter().copied().max().unwrap_or(0) + 1;
    let mut internal_weight = vec![0.0; max_c];
    let mut comm_degree = vec![0.0; max_c];

    for u in 0..graph.n {
        let cu = partition.community[u];
        comm_degree[cu] += graph.degree[u];
        for &(v, w) in &graph.neighbors[u] {
            if partition.community[v] == cu && u < v {
                internal_weight[cu] += w;
            }
        }
    }

    // Q = Σ_c [ L_c/m - γ (K_c / 2m)² ]
    let mut q = 0.0;
    for c in 0..max_c {
        q += internal_weight[c] / m - gamma * (comm_degree[c] / m2).powi(2);
    }
    q
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Location, SymbolKind};
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn make_symbol(name: &str, qn: &str, kind: SymbolKind) -> SymbolNode {
        SymbolNode {
            name: name.to_string(),
            qualified_name: qn.to_string(),
            kind,
            location: Location {
                file: "src/lib.rs".into(),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: crate::model::Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }
    }

    fn make_edge(kind: EdgeKind, source: &str, target: &str) -> Edge {
        Edge {
            kind,
            source: source.to_string(),
            target: target.to_string(),
            metadata: None,
        }
    }

    #[test]
    fn graph_from_symbols_and_edges() {
        let symbols = vec![
            make_symbol("a", "m::a", SymbolKind::Function),
            make_symbol("b", "m::b", SymbolKind::Function),
            make_symbol("c", "m::c", SymbolKind::Function),
        ];
        let edges = vec![
            make_edge(EdgeKind::Calls, "m::a", "m::b"),
            make_edge(EdgeKind::Calls, "m::b", "m::c"),
        ];
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        assert_eq!(graph.n, 3);
        assert!((graph.total_weight - 2.0).abs() < f64::EPSILON);
        assert!((graph.degree[graph.node_to_index["m::a"]] - 1.0).abs() < f64::EPSILON);
        assert!((graph.degree[graph.node_to_index["m::b"]] - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn graph_filters_non_high_confidence_edges() {
        let symbols = vec![
            make_symbol("a", "m::a", SymbolKind::Function),
            make_symbol("b", "m::b", SymbolKind::Function),
        ];
        let edges = vec![make_edge(EdgeKind::Contains, "m::a", "m::b")];
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        assert_eq!(graph.n, 2);
        assert!((graph.total_weight - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn graph_deduplicates_bidirectional_edges() {
        let symbols = vec![
            make_symbol("a", "m::a", SymbolKind::Function),
            make_symbol("b", "m::b", SymbolKind::Function),
        ];
        let edges = vec![
            make_edge(EdgeKind::Calls, "m::a", "m::b"),
            make_edge(EdgeKind::Calls, "m::b", "m::a"),
        ];
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        assert!((graph.total_weight - 2.0).abs() < f64::EPSILON);
        assert!((graph.degree[graph.node_to_index["m::a"]] - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn modularity_singleton_partition_is_zero() {
        let symbols = vec![
            make_symbol("a", "m::a", SymbolKind::Function),
            make_symbol("b", "m::b", SymbolKind::Function),
        ];
        let edges = vec![make_edge(EdgeKind::Calls, "m::a", "m::b")];
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        let partition = Partition::singleton(graph.n);
        let q = compute_modularity(&graph, &partition, 1.0);
        assert!(q <= 0.0 + f64::EPSILON);
    }

    #[test]
    fn modularity_all_in_one_community() {
        let symbols = vec![
            make_symbol("a", "m::a", SymbolKind::Function),
            make_symbol("b", "m::b", SymbolKind::Function),
        ];
        let edges = vec![make_edge(EdgeKind::Calls, "m::a", "m::b")];
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        let mut partition = Partition::singleton(graph.n);
        partition.move_node(1, 0, &graph);
        let q = compute_modularity(&graph, &partition, 1.0);
        assert!(q.abs() < f64::EPSILON);
    }

    #[test]
    fn empty_graph_modularity_is_zero() {
        let graph = LeidenGraph::from_symbols_and_edges(&[], &[]);
        let partition = Partition::singleton(0);
        let q = compute_modularity(&graph, &partition, 1.0);
        assert!((q - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn isolated_nodes_have_zero_degree() {
        let symbols = vec![
            make_symbol("a", "m::a", SymbolKind::Function),
            make_symbol("b", "m::b", SymbolKind::Function),
            make_symbol("c", "m::c", SymbolKind::Function),
        ];
        let edges = vec![make_edge(EdgeKind::Calls, "m::a", "m::b")];
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        assert!((graph.degree[graph.node_to_index["m::c"]] - 0.0).abs() < f64::EPSILON);
    }
}
