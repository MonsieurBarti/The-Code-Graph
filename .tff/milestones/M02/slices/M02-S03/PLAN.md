# M02-S03: Community Detection — Implementation Plan

> For agentic workers: execute task-by-task with TDD.

**Goal:** Implement the Leiden algorithm to partition the symbol graph into communities. Expose via `code-graph communities` CLI command.
**Architecture:** Domain analysis module + use case + CLI command (hexagonal pattern matching flow/risk/clones)
**Tech Stack:** Rust, `rand` crate for deterministic RNG, serde for JSON output

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `crates/domain/src/analysis/community.rs` | Pure Leiden algorithm + tests |
| Create | `crates/domain/src/use_cases/community.rs` | CommunityUseCase + tests |
| Create | `crates/cli/src/commands/communities.rs` | CLI command handler |
| Modify | `crates/domain/Cargo.toml` | Add `rand = "0.9"` |
| Modify | `crates/domain/src/model.rs` | Community types + GraphStats fields |
| Modify | `crates/domain/src/analysis/mod.rs` | `pub mod community;` |
| Modify | `crates/domain/src/use_cases/mod.rs` | `pub mod community;` |
| Modify | `crates/cli/src/commands/mod.rs` | Communities command + CommunitiesArgs |
| Modify | `crates/cli/src/lib.rs` | Dispatch to run_communities |
| Modify | `crates/cli/src/config.rs` | CommunitiesConfig |
| Modify | `crates/cli/src/output.rs` | Displayable for CommunityAnalysis |

---

## Wave 0 (no dependencies)

### T01: Foundation — types, dependency, module registration

**Files:**
- Modify `crates/domain/Cargo.toml` — add `rand = "0.9"`
- Modify `crates/domain/src/model.rs` — add CommunityConfig, Community, CommunityAnalysis, CommunityStats, GraphStats fields
- Modify `crates/domain/src/analysis/mod.rs` — add `pub mod community;`
- Modify `crates/domain/src/use_cases/mod.rs` — add `pub mod community;`
- Create `crates/domain/src/analysis/community.rs` — empty module stub
- Create `crates/domain/src/use_cases/community.rs` — empty module stub

**Traces to:** AC4, AC5

**Steps:**

- [ ] Step 1: Add `rand` to domain dependencies

  In `crates/domain/Cargo.toml`, add after `tracing = "0.1"`:
  ```toml
  rand = "0.9"
  ```

- [ ] Step 2: Add community types to `crates/domain/src/model.rs`

  After `RiskAnalysis` struct (line ~567), add:
  ```rust
  // ── Community Detection ─────────────────────────────────────

  #[derive(Debug, Clone)]
  pub struct CommunityConfig {
      pub resolution: f64,
      pub min_community_size: usize,
      pub seed: Option<u64>,
  }

  impl Default for CommunityConfig {
      fn default() -> Self {
          Self {
              resolution: 1.0,
              min_community_size: 2,
              seed: None,
          }
      }
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct Community {
      pub id: usize,
      pub name: String,
      pub members: Vec<String>,
      pub modularity_contribution: f64,
      pub internal_edges: usize,
      pub boundary_edges: usize,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct CommunityAnalysis {
      pub communities: Vec<Community>,
      pub modularity: f64,
      pub stats: CommunityStats,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct CommunityStats {
      pub count: usize,
      pub avg_size: f64,
      pub largest_size: usize,
      pub isolated_nodes: usize,
  }
  ```

  Add to `GraphStats` struct after `p90_risk` field:
  ```rust
  #[serde(skip_serializing_if = "Option::is_none")]
  pub community_count: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub modularity: Option<f64>,
  ```

- [ ] Step 3: Register module stubs

  `crates/domain/src/analysis/mod.rs` — add line:
  ```rust
  pub mod community;
  ```

  `crates/domain/src/use_cases/mod.rs` — add line:
  ```rust
  pub mod community;
  ```

  Create `crates/domain/src/analysis/community.rs`:
  ```rust
  // Leiden community detection algorithm
  ```

  Create `crates/domain/src/use_cases/community.rs`:
  ```rust
  // Community detection use case
  ```

- [ ] Step 4: Update `crates/domain/src/test_support.rs`

  In the `InMemoryGraphStore::stats()` method, add to the `GraphStats` construction:
  ```rust
  community_count: None,
  modularity: None,
  ```

  Run `cargo check -p domain`
  **Expect:** PASS — compiles with no errors

- [ ] Step 5: Commit
  ```
  feat(M02-S03/T01): add community types, rand dep, module stubs
  ```

---

## Wave 1 (depends on T01)

### T02: LeidenGraph construction + modularity computation

**Files:**
- Modify `crates/domain/src/analysis/community.rs` — LeidenGraph, Partition, modularity
- **Traces to:** AC5, AC14

- [ ] Step 1: Write failing tests

  In `crates/domain/src/analysis/community.rs`:
  ```rust
  use crate::model::{Edge, EdgeKind, Location, SymbolKind, SymbolNode};
  use rand::rngs::StdRng;
  use rand::SeedableRng;

  fn is_high_confidence(kind: &EdgeKind) -> bool {
      matches!(
          kind,
          EdgeKind::Calls | EdgeKind::Extends | EdgeKind::Implements | EdgeKind::Embeds
      )
  }

  struct LeidenGraph {
      n: usize,
      neighbors: Vec<Vec<(usize, f64)>>,
      degree: Vec<f64>,
      total_weight: f64,
      node_to_index: std::collections::HashMap<String, usize>,
      index_to_node: Vec<String>,
  }

  struct Partition {
      community: Vec<usize>,
      community_weight: Vec<f64>,
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      fn make_symbol(name: &str, qn: &str, kind: SymbolKind) -> SymbolNode {
          SymbolNode {
              name: name.to_string(),
              qualified_name: qn.to_string(),
              kind,
              location: crate::model::Location {
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
          // Each directed edge -> undirected: a-b and b-c, each counted in both directions
          // total_weight = number of undirected edges = 2
          assert!((graph.total_weight - 2.0).abs() < f64::EPSILON);
          // a: degree 1 (connected to b), b: degree 2 (connected to a and c), c: degree 1
          assert!((graph.degree[graph.node_to_index["m::a"]] - 1.0).abs() < f64::EPSILON);
          assert!((graph.degree[graph.node_to_index["m::b"]] - 2.0).abs() < f64::EPSILON);
      }

      #[test]
      fn graph_filters_non_high_confidence_edges() {
          let symbols = vec![
              make_symbol("a", "m::a", SymbolKind::Function),
              make_symbol("b", "m::b", SymbolKind::Function),
          ];
          let edges = vec![
              make_edge(EdgeKind::Contains, "m::a", "m::b"), // structural, filtered
          ];
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
          // Two directed edges between same pair -> weight 2.0 on undirected edge
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
          // Singleton partition: no intra-community edges -> Q <= 0
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
          // Merge all into community 0
          partition.move_node(1, 0, &graph);
          let q = compute_modularity(&graph, &partition, 1.0);
          // All in one community with edge: Q = 0 (no inter-community penalty but also no modularity gain)
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
  ```

- [ ] Step 2: Run `cargo test -p domain -- community`
  **Expect:** FAIL — `LeidenGraph::from_symbols_and_edges`, `Partition::singleton`, `Partition::move_node`, `compute_modularity` not implemented

- [ ] Step 3: Implement LeidenGraph, Partition, compute_modularity

  In `crates/domain/src/analysis/community.rs`, add implementations:
  - `LeidenGraph::from_symbols_and_edges(&[SymbolNode], &[Edge]) -> Self` — filter high-confidence edges, build undirected adjacency, compute degrees
  - `Partition::singleton(n) -> Self` — each node in its own community
  - `Partition::move_node(node, target_community, &graph)` — update community assignment and community_weight
  - `compute_modularity(&LeidenGraph, &Partition, gamma) -> f64` — standard modularity formula

- [ ] Step 4: Run `cargo test -p domain -- community`
  **Expect:** PASS — all 7 tests green

- [ ] Step 5: Commit
  ```
  feat(M02-S03/T02): LeidenGraph construction + modularity computation
  ```

---

## Wave 2 (depends on T02 — T03 and T04 are parallel)

### T03: Phase 1 — Local Moving

**Files:**
- Modify `crates/domain/src/analysis/community.rs` — `local_moving()` function
- **Traces to:** AC5, AC7, AC8

- [ ] Step 1: Write failing tests

  ```rust
  #[test]
  fn local_moving_merges_triangle() {
      // K3 graph: a-b-c all connected
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
      let mut partition = Partition::singleton(graph.n);
      let mut rng = StdRng::seed_from_u64(42);
      let moved = local_moving(&graph, &mut partition, 1.0, &mut rng);
      // All 3 should end up in the same community
      assert!(moved);
      assert_eq!(partition.community[0], partition.community[1]);
      assert_eq!(partition.community[1], partition.community[2]);
  }

  #[test]
  fn local_moving_separates_two_cliques() {
      let (symbols, edges) = build_two_cliques_bridge();
      let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);
      let mut partition = Partition::singleton(graph.n);
      let mut rng = StdRng::seed_from_u64(42);
      local_moving(&graph, &mut partition, 1.0, &mut rng);
      // Should find 2 communities (the two cliques)
      let distinct: std::collections::HashSet<usize> = partition.community.iter().copied().collect();
      assert!(distinct.len() >= 2, "expected at least 2 communities, got {}", distinct.len());
  }

  #[test]
  fn local_moving_no_edges_no_moves() {
      let symbols = vec![
          make_symbol("a", "m::a", SymbolKind::Function),
          make_symbol("b", "m::b", SymbolKind::Function),
      ];
      let graph = LeidenGraph::from_symbols_and_edges(&symbols, &[]);
      let mut partition = Partition::singleton(graph.n);
      let mut rng = StdRng::seed_from_u64(42);
      let moved = local_moving(&graph, &mut partition, 1.0, &mut rng);
      assert!(!moved);
  }

  #[test]
  fn local_moving_deterministic_with_same_seed() {
      let (symbols, edges) = build_two_cliques_bridge();
      let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);

      let mut p1 = Partition::singleton(graph.n);
      let mut rng1 = StdRng::seed_from_u64(42);
      local_moving(&graph, &mut p1, 1.0, &mut rng1);

      let mut p2 = Partition::singleton(graph.n);
      let mut rng2 = StdRng::seed_from_u64(42);
      local_moving(&graph, &mut p2, 1.0, &mut rng2);

      assert_eq!(p1.community, p2.community);
  }

  /// Helper: Two K4 cliques connected by a single bridge edge
  fn build_two_cliques_bridge() -> (Vec<SymbolNode>, Vec<Edge>) {
      let mut symbols = Vec::new();
      let mut edges = Vec::new();
      for i in 0..4 {
          symbols.push(make_symbol(&format!("a{i}"), &format!("a::a{i}"), SymbolKind::Function));
          symbols.push(make_symbol(&format!("b{i}"), &format!("b::b{i}"), SymbolKind::Function));
      }
      // Clique A: all pairs
      for i in 0..4 {
          for j in (i + 1)..4 {
              edges.push(make_edge(EdgeKind::Calls, &format!("a::a{i}"), &format!("a::a{j}")));
          }
      }
      // Clique B: all pairs
      for i in 0..4 {
          for j in (i + 1)..4 {
              edges.push(make_edge(EdgeKind::Calls, &format!("b::b{i}"), &format!("b::b{j}")));
          }
      }
      // Bridge
      edges.push(make_edge(EdgeKind::Calls, "a::a0", "b::b0"));
      (symbols, edges)
  }
  ```

- [ ] Step 2: Run `cargo test -p domain -- community`
  **Expect:** FAIL — `local_moving` not found

- [ ] Step 3: Implement `local_moving`

  ```rust
  fn local_moving(
      graph: &LeidenGraph,
      partition: &mut Partition,
      gamma: f64,
      rng: &mut StdRng,
  ) -> bool {
      // Queue-based local moving (pseudocode from RESEARCH.md §1.3)
      // Random permutation of node visit order
      // For each node: compute edge weight to each neighboring community
      // Find best community with positive gain
      // If moved: mark neighbors in other communities as unstable
      // Return true if any node moved
  }
  ```

- [ ] Step 4: Run `cargo test -p domain -- community`
  **Expect:** PASS — all local moving tests green

- [ ] Step 5: Commit
  ```
  feat(M02-S03/T03): Phase 1 local moving with queue-based optimization
  ```

---

### T04: Phase 2 — Refinement

**Files:**
- Modify `crates/domain/src/analysis/community.rs` — `refinement()` function
- **Traces to:** AC6

- [ ] Step 1: Write failing tests

  ```rust
  #[test]
  fn refinement_preserves_connectivity() {
      // Build a graph where Phase 1 might create a disconnected community
      // Two cliques connected by bridge
      let (symbols, edges) = build_two_cliques_bridge();
      let graph = LeidenGraph::from_symbols_and_edges(&symbols, &edges);

      // Simulate Phase 1 having merged everything into one community
      let mut partition = Partition::singleton(graph.n);
      for i in 1..graph.n {
          partition.move_node(i, 0, &graph);
      }

      let mut rng = StdRng::seed_from_u64(42);
      let refined = refinement(&graph, &partition, 1.0, &mut rng);

      // Every refined community must be internally connected
      for c in refined.distinct_communities() {
          let members: Vec<usize> = (0..graph.n)
              .filter(|&i| refined.community[i] == c)
              .collect();
          if members.len() > 1 {
              assert!(is_connected(&graph, &members),
                  "Community {} with {} members is not connected", c, members.len());
          }
      }
  }

  #[test]
  fn refinement_singletons_remain_singletons() {
      // Nodes with no edges should stay as singletons
      let symbols = vec![
          make_symbol("a", "m::a", SymbolKind::Function),
          make_symbol("b", "m::b", SymbolKind::Function),
      ];
      let graph = LeidenGraph::from_symbols_and_edges(&symbols, &[]);
      let partition = Partition::singleton(graph.n);
      let mut rng = StdRng::seed_from_u64(42);
      let refined = refinement(&graph, &partition, 1.0, &mut rng);
      // Each node should still be in its own community
      assert_ne!(refined.community[0], refined.community[1]);
  }

  /// BFS connectivity check for a subset of nodes in the graph
  fn is_connected(graph: &LeidenGraph, members: &[usize]) -> bool {
      use std::collections::{HashSet, VecDeque};
      if members.is_empty() { return true; }
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
  ```

- [ ] Step 2: Run `cargo test -p domain -- community`
  **Expect:** FAIL — `refinement`, `Partition::distinct_communities` not found

- [ ] Step 3: Implement `refinement`

  ```rust
  fn refinement(
      graph: &LeidenGraph,
      partition: &Partition,
      gamma: f64,
      rng: &mut StdRng,
  ) -> Partition {
      // Start from singletons within each Phase 1 community
      // For each node in random order within its community:
      //   find adjacent sub-communities within same Phase 1 community
      //   move to best if gain > 0
      // Adjacency constraint guarantees connectivity
  }
  ```

  Also add `Partition::distinct_communities() -> HashSet<usize>`.

- [ ] Step 4: Run `cargo test -p domain -- community`
  **Expect:** PASS — refinement tests green

- [ ] Step 5: Commit
  ```
  feat(M02-S03/T04): Phase 2 refinement with connectivity guarantee
  ```

---

## Wave 3 (depends on T03 + T04)

### T05: Phase 3 Aggregation + full Leiden loop

**Files:**
- Modify `crates/domain/src/analysis/community.rs` — `aggregate()`, `leiden()`
- **Traces to:** AC5, AC6, AC7, AC8, AC11, AC14

- [ ] Step 1: Write failing tests

  ```rust
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
      let (p_low, _) = leiden(&graph, 0.5, Some(42));
      let (p_high, _) = leiden(&graph, 2.0, Some(42));
      let n_low: HashSet<usize> = p_low.community.iter().copied().collect();
      let n_high: HashSet<usize> = p_high.community.iter().copied().collect();
      assert!(n_high.len() > n_low.len(),
          "gamma=2.0 should produce more communities than gamma=0.5: {} vs {}",
          n_high.len(), n_low.len());
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
              assert!(is_connected(&graph, &members),
                  "Community {} with {} members is not connected", c, members.len());
          }
      }
  }

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
  ```

- [ ] Step 2: Run `cargo test -p domain -- community`
  **Expect:** FAIL — `leiden`, `aggregate` not found

- [ ] Step 3: Implement `aggregate()` and `leiden()` outer loop

  ```rust
  fn aggregate(graph: &LeidenGraph, partition: &Partition) -> Option<(LeidenGraph, Vec<usize>)> {
      // Collapse communities into super-nodes
      // Return None if no aggregation possible (every node is its own community)
  }

  pub fn leiden(
      graph: &LeidenGraph,
      gamma: f64,
      seed: Option<u64>,
  ) -> (Partition, f64) {
      // Outer loop: local_moving -> refinement -> aggregate -> repeat
      // Flatten partition back to original node IDs
      // Return (partition, modularity)
  }
  ```

- [ ] Step 4: Run `cargo test -p domain -- community`
  **Expect:** PASS — all leiden tests green

- [ ] Step 5: Commit
  ```
  feat(M02-S03/T05): Phase 3 aggregation + full Leiden loop
  ```

---

## Wave 4 (depends on T05)

### T06: Community analysis assembly + naming

**Files:**
- Modify `crates/domain/src/analysis/community.rs` — `detect_communities()` public API, `derive_community_name()`
- **Traces to:** AC1, AC5, AC9, AC13, AC14

- [ ] Step 1: Write failing tests

  ```rust
  #[test]
  fn detect_communities_returns_sorted_by_size() {
      let (symbols, edges) = build_two_cliques_bridge();
      let config = CommunityConfig::default();
      let analysis = detect_communities(&symbols, &edges, &config);
      for i in 1..analysis.communities.len() {
          assert!(analysis.communities[i - 1].members.len() >= analysis.communities[i].members.len());
      }
  }

  #[test]
  fn detect_communities_min_size_filters() {
      let (symbols, edges) = build_two_cliques_bridge();
      let mut config = CommunityConfig::default();
      config.min_community_size = 100; // filter everything
      let analysis = detect_communities(&symbols, &edges, &config);
      assert!(analysis.communities.is_empty());
  }

  #[test]
  fn detect_communities_counts_isolated_nodes() {
      let symbols = vec![
          make_symbol("a", "m::a", SymbolKind::Function),
          make_symbol("b", "m::b", SymbolKind::Function),
          make_symbol("c", "m::c", SymbolKind::Function),
      ];
      // Only a-b connected, c is isolated
      let edges = vec![make_edge(EdgeKind::Calls, "m::a", "m::b")];
      let config = CommunityConfig { min_community_size: 1, ..CommunityConfig::default() };
      let analysis = detect_communities(&symbols, &edges, &config);
      assert_eq!(analysis.stats.isolated_nodes, 1);
  }

  #[test]
  fn derive_name_uses_most_common_file_stem() {
      let members = vec![
          "src/auth.rs::login".to_string(),
          "src/auth.rs::logout".to_string(),
          "src/auth.rs::verify".to_string(),
          "src/session.rs::create".to_string(),
      ];
      assert_eq!(derive_community_name(&members, 0), "auth");
  }

  #[test]
  fn derive_name_falls_back_for_generic_stems() {
      let members = vec![
          "src/mod.rs::foo".to_string(),
          "src/mod.rs::bar".to_string(),
      ];
      assert_eq!(derive_community_name(&members, 7), "community_7");
  }

  #[test]
  fn detect_communities_modularity_positive_for_multi_community() {
      let (symbols, edges) = build_two_cliques_bridge();
      let config = CommunityConfig::default();
      let analysis = detect_communities(&symbols, &edges, &config);
      assert!(analysis.communities.len() >= 2);
      assert!(analysis.modularity > 0.0);
  }
  ```

- [ ] Step 2: Run `cargo test -p domain -- community`
  **Expect:** FAIL — `detect_communities`, `derive_community_name` not found

- [ ] Step 3: Implement public API

  ```rust
  pub fn detect_communities(
      symbols: &[SymbolNode],
      edges: &[Edge],
      config: &CommunityConfig,
  ) -> CommunityAnalysis {
      // Build LeidenGraph
      // Run leiden()
      // Group nodes by community
      // Compute per-community stats (internal_edges, boundary_edges, modularity_contribution)
      // Derive community names
      // Filter by min_community_size
      // Sort by size descending
      // Count isolated nodes (degree-0 symbols)
      // Assemble CommunityAnalysis
  }

  fn derive_community_name(members: &[String], community_id: usize) -> String {
      // (1) count file occurrences, (2) extract file stem,
      // (3) break ties alphabetically, (4) filter generic names
  }
  ```

- [ ] Step 4: Run `cargo test -p domain -- community`
  **Expect:** PASS — all assembly tests green

- [ ] Step 5: Commit
  ```
  feat(M02-S03/T06): community analysis assembly + naming heuristic
  ```

---

## Wave 5 (depends on T06)

### T07: CommunityUseCase

**Files:**
- Modify `crates/domain/src/use_cases/community.rs` — full use case
- **Traces to:** AC1, AC2, AC3, AC12

- [ ] Step 1: Write failing tests

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::model::{*, Location};
      use crate::test_support::InMemoryGraphStore;

      fn build_test_store() -> InMemoryGraphStore {
          let mut store = InMemoryGraphStore::new();
          // Two groups of symbols with internal calls
          for i in 0..3 {
              store.insert_symbol(SymbolNode {
                  name: format!("a{i}"),
                  qualified_name: format!("src/auth.rs::a{i}"),
                  kind: SymbolKind::Function,
                  location: Location {
                      file: "src/auth.rs".into(),
                      line_start: i * 10 + 1, line_end: i * 10 + 10,
                      col_start: 0, col_end: 0,
                  },
                  visibility: Visibility::Public,
                  is_exported: true, is_async: false, is_test: false,
                  decorators: vec![], signature: None,
              });
          }
          for i in 0..3 {
              for j in (i + 1)..3 {
                  store.insert_edge(Edge {
                      kind: EdgeKind::Calls,
                      source: format!("src/auth.rs::a{i}"),
                      target: format!("src/auth.rs::a{j}"),
                      metadata: None,
                  });
              }
          }
          store
      }

      #[test]
      fn analyze_returns_communities() {
          let store = build_test_store();
          let uc = CommunityUseCase::new(store);
          let result = uc.analyze(&CommunityConfig::default()).unwrap();
          assert!(result.stats.count > 0 || result.stats.isolated_nodes > 0);
      }

      #[test]
      fn community_of_finds_symbol() {
          let store = build_test_store();
          let uc = CommunityUseCase::new(store);
          let c = uc.community_of("src/auth.rs::a0", &CommunityConfig::default()).unwrap();
          assert!(c.is_some());
          assert!(c.unwrap().members.contains(&"src/auth.rs::a0".to_string()));
      }

      #[test]
      fn community_of_returns_none_for_unknown() {
          let store = build_test_store();
          let uc = CommunityUseCase::new(store);
          let c = uc.community_of("nonexistent::symbol", &CommunityConfig::default()).unwrap();
          assert!(c.is_none());
      }
  }
  ```

- [ ] Step 2: Run `cargo test -p domain -- use_cases::community`
  **Expect:** FAIL — `CommunityUseCase` not implemented

- [ ] Step 3: Implement use case

  ```rust
  use crate::analysis::community::detect_communities;
  use crate::model::{CommunityAnalysis, CommunityConfig, Community};
  use crate::ports::GraphStore;
  use crate::error::Result;

  pub struct CommunityUseCase<S> {
      store: S,
  }

  impl<S: GraphStore> CommunityUseCase<S> {
      pub fn new(store: S) -> Self { Self { store } }

      pub fn analyze(&self, config: &CommunityConfig) -> Result<CommunityAnalysis> {
          let symbols = self.store.all_symbols()?;
          let edges = self.store.all_edges()?;
          Ok(detect_communities(&symbols, &edges, config))
      }

      pub fn community_of(&self, symbol: &str, config: &CommunityConfig) -> Result<Option<Community>> {
          let analysis = self.analyze(config)?;
          Ok(analysis.communities.into_iter().find(|c| c.members.contains(&symbol.to_string())))
      }
  }
  ```

- [ ] Step 4: Run `cargo test -p domain -- use_cases::community`
  **Expect:** PASS — all use case tests green

- [ ] Step 5: Commit
  ```
  feat(M02-S03/T07): CommunityUseCase with analyze + community_of
  ```

---

## Wave 6 (depends on T07)

### T08: CLI command + config + output formatting

**Files:**
- Create `crates/cli/src/commands/communities.rs` — command handler
- Modify `crates/cli/src/commands/mod.rs` — add Communities command + CommunitiesArgs
- Modify `crates/cli/src/lib.rs` — add dispatch
- Modify `crates/cli/src/config.rs` — add CommunitiesConfig
- Modify `crates/cli/src/output.rs` — Displayable for CommunityAnalysis
- **Traces to:** AC1, AC2, AC3, AC4, AC9, AC10, AC12, AC13

- [ ] Step 1: Add CommunitiesConfig to `crates/cli/src/config.rs`

  After `RiskCliConfig` struct (line ~42), add:
  ```rust
  #[derive(Debug, Clone, Default, Deserialize)]
  pub struct CommunitiesConfig {
      pub resolution: Option<f64>,
      pub min_community_size: Option<usize>,
      pub seed: Option<u64>,
  }
  ```

  In `CodeGraphConfig` struct, add field:
  ```rust
  pub communities: Option<CommunitiesConfig>,
  ```

- [ ] Step 2: Add CommunitiesArgs to `crates/cli/src/commands/mod.rs`

  Add `pub mod communities;` to module list.

  Add variant to `Commands` enum (after `Clones`):
  ```rust
  /// Detect communities of tightly-coupled symbols
  Communities(CommunitiesArgs),
  ```

  Add args struct:
  ```rust
  #[derive(clap::Args)]
  pub struct CommunitiesArgs {
      /// Show details for a specific community
      pub community_id: Option<usize>,
      /// Modularity resolution parameter
      #[arg(long)]
      pub resolution: Option<f64>,
      /// Minimum community size to display
      #[arg(long)]
      pub min_size: Option<usize>,
      /// Random seed for reproducibility
      #[arg(long)]
      pub seed: Option<u64>,
      /// Show which community a symbol belongs to
      #[arg(long)]
      pub symbol: Option<String>,
      /// Maximum communities to display
      #[arg(long, default_value = "20")]
      pub limit: usize,
  }
  ```

- [ ] Step 3: Add dispatch in `crates/cli/src/lib.rs`

  Add to match block:
  ```rust
  Commands::Communities(args) => commands::communities::run_communities(args, output_format),
  ```

- [ ] Step 4: Implement Displayable in `crates/cli/src/output.rs`

  ```rust
  impl Displayable for CommunityAnalysis {
      fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
          writeln!(w, "Communities: {} (modularity: {:.2})", self.stats.count, self.modularity)?;
          writeln!(w)?;
          for c in &self.communities {
              writeln!(w, " #{}  {} ({} symbols, {} internal / {} boundary edges)",
                  c.id, c.name, c.members.len(), c.internal_edges, c.boundary_edges)?;
              let preview: Vec<&str> = c.members.iter().take(3).map(|s| s.as_str()).collect();
              writeln!(w, "     {}{}", preview.join(", "),
                  if c.members.len() > 3 { ", ..." } else { "" })?;
          }
          Ok(())
      }

      fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
          writeln!(w, " ID  Name            Size  Internal  Boundary  Modularity")?;
          for c in &self.communities {
              writeln!(w, "{:>3}  {:<15} {:>4}  {:>8}  {:>8}  {:>10.2}",
                  c.id, c.name, c.members.len(), c.internal_edges, c.boundary_edges,
                  c.modularity_contribution)?;
          }
          Ok(())
      }

      fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
          let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
          writeln!(w, "{json}")
      }
  }
  ```

  Also implement for single community detail view:
  ```rust
  impl Displayable for Vec<Community> {
      fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
          for c in self {
              writeln!(w, "Community #{}: {} ({} symbols)", c.id, c.name, c.members.len())?;
              writeln!(w, "Modularity contribution: {:.2}", c.modularity_contribution)?;
              writeln!(w, "Internal edges: {} | Boundary edges: {}", c.internal_edges, c.boundary_edges)?;
              writeln!(w)?;
              writeln!(w, "Members:")?;
              for m in &c.members {
                  writeln!(w, "  {m}")?;
              }
          }
          Ok(())
      }

      fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
          for c in self {
              writeln!(w, "Community #{}: {} ({} symbols)", c.id, c.name, c.members.len())?;
              writeln!(w)?;
              writeln!(w, "Member")?;
              writeln!(w, "------")?;
              for m in &c.members {
                  writeln!(w, "{m}")?;
              }
          }
          Ok(())
      }

      fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
          let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
          writeln!(w, "{json}")
      }
  }
  ```

- [ ] Step 5: Create `crates/cli/src/commands/communities.rs`

  ```rust
  use crate::commands::CommunitiesArgs;
  use crate::config::load_config;
  use crate::output::{print, OutputFormat};
  use crate::commands::helpers::open_graph;
  use domain::error::Result;
  use domain::model::CommunityConfig;
  use domain::use_cases::community::CommunityUseCase;

  pub fn run_communities(args: &CommunitiesArgs, output_format: OutputFormat) -> Result<()> {
      let (store, root) = open_graph()?;
      let config = load_config(&root)?;

      let mut community_config = CommunityConfig::default();

      // Config file overrides (lowest priority after defaults)
      if let Some(cc) = &config.communities {
          if let Some(r) = cc.resolution { community_config.resolution = r; }
          if let Some(s) = cc.min_community_size { community_config.min_community_size = s; }
          if let Some(s) = cc.seed { community_config.seed = Some(s); }
      }

      // CLI flag overrides (highest priority)
      if let Some(r) = args.resolution { community_config.resolution = r; }
      if let Some(s) = args.min_size { community_config.min_community_size = s; }
      if let Some(s) = args.seed { community_config.seed = Some(s); }

      let uc = CommunityUseCase::new(store);

      if let Some(ref symbol) = args.symbol {
          match uc.community_of(symbol, &community_config)? {
              Some(c) => {
                  eprintln!("{} -> Community #{} ({}, {} members)",
                      symbol, c.id, c.name, c.members.len());
              }
              None => {
                  eprintln!("symbol '{}' not found in any community", symbol);
              }
          }
          return Ok(());
      }

      let mut analysis = uc.analyze(&community_config)?;

      if let Some(community_id) = args.community_id {
          if let Some(c) = analysis.communities.iter().find(|c| c.id == community_id) {
              print(&vec![c.clone()], output_format);
          } else {
              eprintln!("community {} not found ({} communities total)",
                  community_id, analysis.communities.len());
          }
      } else {
          analysis.communities.truncate(args.limit);
          print(&analysis, output_format);
      }
      Ok(())
  }
  ```

- [ ] Step 6: Run `cargo build -p cli`
  **Expect:** PASS — compiles with no errors

- [ ] Step 7: Add CLI parse test to `crates/cli/src/commands/mod.rs`

  In `all_subcommands_parse` test, add:
  ```rust
  vec!["code-graph", "communities"],
  vec!["code-graph", "communities", "--resolution", "1.5"],
  vec!["code-graph", "communities", "--seed", "42", "--min-size", "3"],
  vec!["code-graph", "communities", "1"],
  vec!["code-graph", "communities", "--symbol", "src/main.rs::main"],
  ```

  Add config test to `crates/cli/src/config.rs`:
  ```rust
  #[test]
  fn communities_config_parses() {
      let tmp = tempfile::tempdir().unwrap();
      let dir = tmp.path().join(".code-graph");
      std::fs::create_dir_all(&dir).unwrap();
      std::fs::write(dir.join("config.toml"), r#"
  [communities]
  resolution = 1.5
  min_community_size = 3
  seed = 42
  "#).unwrap();
      let config = load_config(tmp.path()).unwrap();
      let cc = config.communities.unwrap();
      assert!((cc.resolution.unwrap() - 1.5).abs() < f64::EPSILON);
      assert_eq!(cc.min_community_size.unwrap(), 3);
      assert_eq!(cc.seed.unwrap(), 42);
  }
  ```

- [ ] Step 8: Run `cargo test -p cli`
  **Expect:** PASS — all CLI tests green

- [ ] Step 9: Commit
  ```
  feat(M02-S03/T08): CLI command, config, output formatting for communities
  ```
