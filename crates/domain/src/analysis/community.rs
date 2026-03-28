// Leiden community detection algorithm

use crate::model::{Edge, EdgeKind, SymbolNode};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
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

#[allow(dead_code)]
fn local_moving(
    graph: &LeidenGraph,
    partition: &mut Partition,
    gamma: f64,
    rng: &mut StdRng,
) -> bool {
    if graph.n == 0 {
        return false;
    }
    let m2 = 2.0 * graph.total_weight;
    if m2 == 0.0 {
        return false;
    }
    let mut any_moved = false;
    let mut order: Vec<usize> = (0..graph.n).collect();
    order.shuffle(rng);

    let mut improved = true;
    while improved {
        improved = false;
        for &node in &order {
            let old_comm = partition.community[node];
            let ki = graph.degree[node];
            if ki == 0.0 {
                continue;
            }

            // Compute edge weight to each neighboring community
            let mut comm_edge_weight: HashMap<usize, f64> = HashMap::new();
            for &(neighbor, w) in &graph.neighbors[node] {
                *comm_edge_weight
                    .entry(partition.community[neighbor])
                    .or_default() += w;
            }

            // Compute delta for removing node from current community
            let w_old = comm_edge_weight.get(&old_comm).copied().unwrap_or(0.0);
            let sigma_old = partition.community_weight[old_comm] - ki;
            let remove_gain =
                -w_old / graph.total_weight + gamma * ki * sigma_old / (m2 * graph.total_weight);

            let mut best_comm = old_comm;
            let mut best_gain = 0.0;

            // Sort candidates for deterministic tie-breaking
            let mut candidates: Vec<(usize, f64)> = comm_edge_weight
                .iter()
                .filter(|(&c, _)| c != old_comm)
                .map(|(&c, &w)| (c, w))
                .collect();
            candidates.sort_by_key(|&(c, _)| c);

            for (target_comm, w_target) in candidates {
                let sigma_target = partition.community_weight[target_comm];
                let insert_gain = w_target / graph.total_weight
                    - gamma * ki * sigma_target / (m2 * graph.total_weight);
                let total_gain = remove_gain + insert_gain;
                if total_gain > best_gain {
                    best_gain = total_gain;
                    best_comm = target_comm;
                }
            }

            if best_comm != old_comm {
                partition.move_node(node, best_comm, graph);
                improved = true;
                any_moved = true;
            }
        }
    }
    any_moved
}

#[allow(dead_code)]
fn refinement(
    graph: &LeidenGraph,
    partition: &Partition,
    gamma: f64,
    rng: &mut StdRng,
) -> Partition {
    // Start from singletons — each node is its own sub-community
    let mut refined = Partition::singleton_with_graph(graph);

    let m2 = 2.0 * graph.total_weight;
    if m2 == 0.0 {
        return refined;
    }

    // Process each Phase 1 community separately
    let communities = partition.distinct_communities();
    for phase1_comm in communities {
        let members: Vec<usize> = (0..graph.n)
            .filter(|&i| partition.community[i] == phase1_comm)
            .collect();
        if members.len() <= 1 {
            continue;
        }

        // Randomize visit order within this community
        let mut order = members.clone();
        order.shuffle(rng);

        for &node in &order {
            let cur_sub = refined.community[node];
            let ki = graph.degree[node];
            if ki == 0.0 {
                continue;
            }

            // Find adjacent sub-communities within the same Phase 1 community
            let mut sub_edge_weight: HashMap<usize, f64> = HashMap::new();
            for &(neighbor, w) in &graph.neighbors[node] {
                if partition.community[neighbor] == phase1_comm {
                    let sub = refined.community[neighbor];
                    if sub != cur_sub {
                        *sub_edge_weight.entry(sub).or_default() += w;
                    }
                }
            }

            let w_old = {
                let mut w = 0.0;
                for &(neighbor, weight) in &graph.neighbors[node] {
                    if refined.community[neighbor] == cur_sub && neighbor != node {
                        w += weight;
                    }
                }
                w
            };
            let sigma_old = refined.community_weight[cur_sub] - ki;
            let remove_gain =
                -w_old / graph.total_weight + gamma * ki * sigma_old / (m2 * graph.total_weight);

            let mut best_sub = cur_sub;
            let mut best_gain = 0.0;

            let mut candidates: Vec<(usize, f64)> =
                sub_edge_weight.iter().map(|(&c, &w)| (c, w)).collect();
            candidates.sort_by_key(|&(c, _)| c);

            for (target_sub, w_target) in candidates {
                let sigma_target = refined.community_weight[target_sub];
                let insert_gain = w_target / graph.total_weight
                    - gamma * ki * sigma_target / (m2 * graph.total_weight);
                let total_gain = remove_gain + insert_gain;
                if total_gain > best_gain {
                    best_gain = total_gain;
                    best_sub = target_sub;
                }
            }

            if best_sub != cur_sub {
                refined.move_node(node, best_sub, graph);
            }
        }
    }
    refined
}

/// Collapse communities into super-nodes, producing an aggregated graph.
/// Returns None if every node is already its own community (no aggregation possible).
#[allow(dead_code)]
fn aggregate(graph: &LeidenGraph, partition: &Partition) -> Option<(LeidenGraph, Vec<usize>)> {
    let communities = partition.distinct_communities();
    if communities.len() == graph.n {
        return None;
    }

    // Map old community IDs to dense indices
    let mut comm_ids: Vec<usize> = communities.into_iter().collect();
    comm_ids.sort();
    let comm_to_new: HashMap<usize, usize> = comm_ids
        .iter()
        .enumerate()
        .map(|(new_idx, &old_id)| (old_id, new_idx))
        .collect();
    let n_new = comm_ids.len();

    // mapping[original_node] = new super-node index
    let mapping: Vec<usize> = partition.community.iter().map(|c| comm_to_new[c]).collect();

    // Build aggregated inter-community edge weights
    let mut agg_edges: HashMap<(usize, usize), f64> = HashMap::new();
    for u in 0..graph.n {
        let cu = mapping[u];
        for &(v, w) in &graph.neighbors[u] {
            let cv = mapping[v];
            if cu < cv {
                *agg_edges.entry((cu, cv)).or_default() += w;
            }
        }
    }

    let mut neighbors: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n_new];
    // Degree of each super-node = sum of degrees of constituent nodes
    let mut degree = vec![0.0; n_new];
    for (i, &d) in graph.degree.iter().enumerate() {
        degree[mapping[i]] += d;
    }
    // total_weight is preserved (internal edges become self-loops conceptually)
    let total_weight = graph.total_weight;

    for (&(u, v), &w) in &agg_edges {
        neighbors[u].push((v, w));
        neighbors[v].push((u, w));
    }

    let mut index_to_node = vec![String::new(); n_new];
    let mut node_to_index = HashMap::new();
    for (new_idx, &old_comm) in comm_ids.iter().enumerate() {
        let name = format!("super_{old_comm}");
        index_to_node[new_idx] = name.clone();
        node_to_index.insert(name, new_idx);
    }

    let agg_graph = LeidenGraph {
        n: n_new,
        neighbors,
        degree,
        total_weight,
        node_to_index,
        index_to_node,
    };

    Some((agg_graph, mapping))
}

#[allow(dead_code)]
pub(crate) fn leiden(graph: &LeidenGraph, gamma: f64, seed: Option<u64>) -> (Partition, f64) {
    use rand::SeedableRng;

    if graph.n == 0 {
        return (Partition::singleton(0), 0.0);
    }
    if graph.total_weight == 0.0 {
        return (Partition::singleton(graph.n), 0.0);
    }

    let mut rng = match seed {
        Some(s) => StdRng::seed_from_u64(s),
        None => StdRng::from_os_rng(),
    };

    let mut current_graph = None::<LeidenGraph>;
    // Track how original nodes map through aggregation levels
    let mut global_mapping: Vec<usize> = (0..graph.n).collect();

    let max_iterations = 20;
    for _ in 0..max_iterations {
        let g = current_graph.as_ref().unwrap_or(graph);
        let mut partition = Partition::singleton_with_graph(g);

        let moved = local_moving(g, &mut partition, gamma, &mut rng);
        if !moved {
            break;
        }

        let refined = refinement(g, &partition, gamma, &mut rng);

        // Update global mapping with refined partition
        for gm in global_mapping.iter_mut() {
            *gm = refined.community[*gm];
        }

        match aggregate(g, &refined) {
            Some((agg_graph, mapping)) => {
                // Update global mapping through aggregation
                for gm in global_mapping.iter_mut() {
                    *gm = mapping[*gm];
                }
                current_graph = Some(agg_graph);
            }
            None => break,
        }
    }

    // Build final partition from global mapping
    let mut final_partition = Partition {
        community: global_mapping,
        community_weight: vec![0.0; graph.n],
    };
    // Renumber communities to be 0..k-1
    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut next_id = 0;
    for c in final_partition.community.iter_mut() {
        let new_id = *remap.entry(*c).or_insert_with(|| {
            let id = next_id;
            next_id += 1;
            id
        });
        *c = new_id;
    }
    // Recompute community weights
    final_partition.community_weight = vec![0.0; next_id];
    for (i, &c) in final_partition.community.iter().enumerate() {
        final_partition.community_weight[c] += graph.degree[i];
    }

    let q = compute_modularity(graph, &final_partition, gamma);
    (final_partition, q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Location, SymbolKind};
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use std::collections::HashSet;

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

    // ---- T03: Local Moving tests ----

    /// Helper: Two K4 cliques connected by a single bridge edge
    fn build_two_cliques_bridge() -> (Vec<SymbolNode>, Vec<Edge>) {
        let mut symbols = Vec::new();
        let mut edges = Vec::new();
        for i in 0..4 {
            symbols.push(make_symbol(
                &format!("a{i}"),
                &format!("a::a{i}"),
                SymbolKind::Function,
            ));
            symbols.push(make_symbol(
                &format!("b{i}"),
                &format!("b::b{i}"),
                SymbolKind::Function,
            ));
        }
        // Clique A: all pairs
        for i in 0..4 {
            for j in (i + 1)..4 {
                edges.push(make_edge(
                    EdgeKind::Calls,
                    &format!("a::a{i}"),
                    &format!("a::a{j}"),
                ));
            }
        }
        // Clique B: all pairs
        for i in 0..4 {
            for j in (i + 1)..4 {
                edges.push(make_edge(
                    EdgeKind::Calls,
                    &format!("b::b{i}"),
                    &format!("b::b{j}"),
                ));
            }
        }
        // Bridge
        edges.push(make_edge(EdgeKind::Calls, "a::a0", "b::b0"));
        (symbols, edges)
    }

    #[test]
    fn local_moving_merges_triangle() {
        let symbols = vec![
            make_symbol("a", "m::a", SymbolKind::Function),
            make_symbol("b", "m::b", SymbolKind::Function),
            make_symbol("c", "m::c", SymbolKind::Function),
        ];
        let edges = vec![
            make_edge(EdgeKind::Calls, "m::a", "m::b"),
            make_edge(EdgeKind::Calls, "m::b", "m::c"),
            make_edge(EdgeKind::Calls, "m::a", "m::c"),
        ];
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        let mut partition = Partition::singleton_with_graph(&graph);
        let mut rng = StdRng::seed_from_u64(42);
        let moved = local_moving(&graph, &mut partition, 1.0, &mut rng);
        assert!(moved);
        assert_eq!(partition.community[0], partition.community[1]);
        assert_eq!(partition.community[1], partition.community[2]);
    }

    #[test]
    fn local_moving_separates_two_cliques() {
        let (symbols, edges) = build_two_cliques_bridge();
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        let mut partition = Partition::singleton_with_graph(&graph);
        let mut rng = StdRng::seed_from_u64(42);
        local_moving(&graph, &mut partition, 1.0, &mut rng);
        let distinct: std::collections::HashSet<usize> =
            partition.community.iter().copied().collect();
        assert!(
            distinct.len() >= 2,
            "expected at least 2 communities, got {}",
            distinct.len()
        );
    }

    #[test]
    fn local_moving_no_edges_no_moves() {
        let symbols = vec![
            make_symbol("a", "m::a", SymbolKind::Function),
            make_symbol("b", "m::b", SymbolKind::Function),
        ];
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &[]);
        let mut partition = Partition::singleton_with_graph(&graph);
        let mut rng = StdRng::seed_from_u64(42);
        let moved = local_moving(&graph, &mut partition, 1.0, &mut rng);
        assert!(!moved);
    }

    #[test]
    fn local_moving_deterministic_with_same_seed() {
        let (symbols, edges) = build_two_cliques_bridge();
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);

        let mut p1 = Partition::singleton_with_graph(&graph);
        let mut rng1 = StdRng::seed_from_u64(42);
        local_moving(&graph, &mut p1, 1.0, &mut rng1);

        let mut p2 = Partition::singleton_with_graph(&graph);
        let mut rng2 = StdRng::seed_from_u64(42);
        local_moving(&graph, &mut p2, 1.0, &mut rng2);

        assert_eq!(p1.community, p2.community);
    }

    // ---- T04: Refinement tests ----

    /// BFS connectivity check for a subset of nodes in the graph
    fn is_connected(graph: &LeidenGraph, members: &[usize]) -> bool {
        use std::collections::{HashSet, VecDeque};
        if members.is_empty() {
            return true;
        }
        let member_set: HashSet<usize> = members.iter().copied().collect();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(members[0]);
        queue.push_back(members[0]);
        while let Some(node) = queue.pop_front() {
            for &(neighbor, _) in &graph.neighbors[node] {
                if member_set.contains(&neighbor) && visited.insert(neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }
        visited.len() == members.len()
    }

    #[test]
    fn refinement_preserves_connectivity() {
        let (symbols, edges) = build_two_cliques_bridge();
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);

        // Simulate Phase 1 having merged everything into one community
        let mut partition = Partition::singleton_with_graph(&graph);
        for i in 1..graph.n {
            partition.move_node(i, 0, &graph);
        }

        let mut rng = StdRng::seed_from_u64(42);
        let refined = refinement(&graph, &partition, 1.0, &mut rng);

        for c in refined.distinct_communities() {
            let members: Vec<usize> = (0..graph.n)
                .filter(|&i| refined.community[i] == c)
                .collect();
            if members.len() > 1 {
                assert!(
                    is_connected(&graph, &members),
                    "Community {} with {} members is not connected",
                    c,
                    members.len()
                );
            }
        }
    }

    #[test]
    fn refinement_singletons_remain_singletons() {
        let symbols = vec![
            make_symbol("a", "m::a", SymbolKind::Function),
            make_symbol("b", "m::b", SymbolKind::Function),
        ];
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &[]);
        let partition = Partition::singleton_with_graph(&graph);
        let mut rng = StdRng::seed_from_u64(42);
        let refined = refinement(&graph, &partition, 1.0, &mut rng);
        assert_ne!(refined.community[0], refined.community[1]);
    }

    // ---- T05: Aggregation + Leiden loop tests ----

    /// 4 complete subgraphs K5 connected by single bridge edges
    fn build_multiscale_graph() -> (Vec<SymbolNode>, Vec<Edge>) {
        let mut symbols = Vec::new();
        let mut edges = Vec::new();
        for clique in 0..4 {
            for i in 0..5 {
                symbols.push(make_symbol(
                    &format!("c{clique}_n{i}"),
                    &format!("src/mod{clique}.rs::c{clique}_n{i}"),
                    SymbolKind::Function,
                ));
            }
            for i in 0..5 {
                for j in (i + 1)..5 {
                    edges.push(make_edge(
                        EdgeKind::Calls,
                        &format!("src/mod{clique}.rs::c{clique}_n{i}"),
                        &format!("src/mod{clique}.rs::c{clique}_n{j}"),
                    ));
                }
            }
        }
        for clique in 0..3 {
            edges.push(make_edge(
                EdgeKind::Calls,
                &format!("src/mod{clique}.rs::c{clique}_n0"),
                &format!("src/mod{}.rs::c{}_n0", clique + 1, clique + 1),
            ));
        }
        (symbols, edges)
    }

    #[test]
    fn leiden_two_cliques_finds_two_communities() {
        let (symbols, edges) = build_two_cliques_bridge();
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        let (partition, _) = leiden(&graph, 1.0, Some(42));
        let distinct: HashSet<usize> = partition.community.iter().copied().collect();
        assert_eq!(distinct.len(), 2);
    }

    #[test]
    fn leiden_triangle_finds_one_community() {
        let symbols = vec![
            make_symbol("a", "m::a", SymbolKind::Function),
            make_symbol("b", "m::b", SymbolKind::Function),
            make_symbol("c", "m::c", SymbolKind::Function),
        ];
        let edges = vec![
            make_edge(EdgeKind::Calls, "m::a", "m::b"),
            make_edge(EdgeKind::Calls, "m::b", "m::c"),
            make_edge(EdgeKind::Calls, "m::a", "m::c"),
        ];
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        let (partition, _) = leiden(&graph, 1.0, Some(42));
        let distinct: HashSet<usize> = partition.community.iter().copied().collect();
        assert_eq!(distinct.len(), 1);
    }

    #[test]
    fn leiden_empty_graph() {
        let graph = LeidenGraph::from_symbols_and_edges(&[], &[]);
        let (partition, modularity) = leiden(&graph, 1.0, Some(42));
        assert!(partition.community.is_empty());
        assert!((modularity - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn leiden_deterministic_with_seed() {
        let (symbols, edges) = build_multiscale_graph();
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        let (p1, q1) = leiden(&graph, 1.0, Some(42));
        let (p2, q2) = leiden(&graph, 1.0, Some(42));
        assert_eq!(p1.community, p2.community);
        assert!((q1 - q2).abs() < f64::EPSILON);
    }

    #[test]
    fn leiden_higher_resolution_more_communities() {
        let (symbols, edges) = build_multiscale_graph();
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        let (p_low, _) = leiden(&graph, 0.1, Some(42));
        let (p_high, _) = leiden(&graph, 5.0, Some(42));
        let n_low: HashSet<usize> = p_low.community.iter().copied().collect();
        let n_high: HashSet<usize> = p_high.community.iter().copied().collect();
        assert!(
            n_high.len() > n_low.len(),
            "gamma=5.0 should produce more communities than gamma=0.1: {} vs {}",
            n_high.len(),
            n_low.len()
        );
    }

    #[test]
    fn leiden_all_communities_connected() {
        let (symbols, edges) = build_multiscale_graph();
        let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
        let (partition, _) = leiden(&graph, 1.0, Some(42));
        for c in partition.distinct_communities() {
            let members: Vec<usize> = (0..graph.n)
                .filter(|&i| partition.community[i] == c)
                .collect();
            if members.len() > 1 {
                assert!(
                    is_connected(&graph, &members),
                    "Community {} with {} members is not connected",
                    c,
                    members.len()
                );
            }
        }
    }
}
