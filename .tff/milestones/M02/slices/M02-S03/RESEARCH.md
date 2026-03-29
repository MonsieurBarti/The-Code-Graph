# Research -- M02-S03: Community Detection (Leiden Algorithm)

## 1. Leiden Algorithm -- Complete Implementation Details

### 1.1 Overview

The Leiden algorithm (Traag, Waltman & van Eck, 2019) is a three-phase iterative method for community detection that improves upon Louvain by guaranteeing well-connected communities. The three phases are: local moving, refinement, and aggregation. The algorithm repeats until convergence.

Reference implementation: https://github.com/CWTSLeiden/networkanalysis (Java, by the original authors)

### 1.2 Quality Function

The spec uses standard modularity (Newman-Girvan):

```
Q = (1/2m) * sum_ij [ A_ij - gamma * (k_i * k_j) / (2m) ] * delta(c_i, c_j)
```

Where:
- `m` = total edge weight (sum of all edge weights; for unweighted undirected graphs: number of edges)
- `A_ij` = adjacency matrix entry (edge weight between i and j)
- `k_i` = weighted degree of node i (sum of weights of all edges incident to i)
- `c_i` = community assignment of node i
- `gamma` = resolution parameter (default 1.0)
- `delta(c_i, c_j)` = 1 if c_i == c_j, 0 otherwise

**IMPORTANT**: The reference implementation uses CPM (Constant Potts Model) rather than modularity. For CPM:

```
qualityIncrement = edgeWeightToCluster - nodeWeight * clusterWeight * resolution
```

For standard modularity, the incremental gain when moving node i from community A to community B is:

```
delta_Q = [ k_{i,B} / m - gamma * (sigma_B * k_i) / (2m^2) ]
        - [ k_{i,A_removed} / m - gamma * (sigma_{A_removed} * k_i) / (2m^2) ]
```

Simplified form (moving isolated node i into community C):

```
delta_Q = k_{i,in} / (2m) - gamma * (sigma_tot * k_i) / (2m^2)
```

Where:
- `k_{i,in}` = sum of edge weights from node i to nodes in target community C
- `k_i` = weighted degree of node i
- `sigma_tot` = sum of weighted degrees of all nodes in community C
- `m` = total edge weight

**Efficient incremental computation**: We do NOT need to recompute full Q for each candidate move. We only compute delta_Q for moving node i to each neighboring community, and pick the maximum positive gain. This requires maintaining:
- `community_weight[c]` = sum of k_j for all j in community c (updated when nodes move)
- Per-move: iterate over neighbors of i, accumulate `k_{i,in}` for each neighboring community

### 1.3 Phase 1 -- Local Moving

**Pseudocode** (based on FastLocalMovingAlgorithm from the reference):

```
function local_moving(graph, partition, rng):
    node_order = random_permutation(0..n, rng)
    unstable = [true; n]   // all nodes initially unstable
    n_unstable = n
    idx = 0

    while n_unstable > 0:
        node = node_order[idx]
        idx = (idx + 1) % n

        if not unstable[node]:
            continue

        unstable[node] = false
        n_unstable -= 1

        current_community = partition.community[node]

        // Compute edge weight from node to each neighboring community
        edge_weight_per_community = {}
        for (neighbor, weight) in graph.neighbors(node):
            c = partition.community[neighbor]
            edge_weight_per_community[c] += weight

        // Remove node from its current community (update community_weight)
        partition.community_weight[current_community] -= graph.degree[node]

        // Find best community to move to
        best_community = current_community
        best_gain = 0.0

        for (c, k_i_in) in edge_weight_per_community:
            // delta_Q = k_i_in / (2m) - gamma * sigma_c * k_i / (2m^2)
            // Simplify: compare k_i_in - gamma * sigma_c * k_i / (2m)
            gain = k_i_in - gamma * partition.community_weight[c] * graph.degree[node] / (2.0 * graph.total_weight)
            if gain > best_gain:
                best_gain = gain
                best_community = c

        // Move node to best community
        partition.community[node] = best_community
        partition.community_weight[best_community] += graph.degree[node]

        // If node moved, mark neighbors in OTHER communities as unstable
        if best_community != current_community:
            for (neighbor, _) in graph.neighbors(node):
                if partition.community[neighbor] != best_community:
                    if not unstable[neighbor]:
                        unstable[neighbor] = true
                        n_unstable += 1

    return partition
```

**Termination**: Phase 1 ends when no unstable nodes remain (all nodes have been checked and none want to move).

**Key detail**: The gain comparison uses a relative form that avoids the 1/(2m) denominator. Since we compare gains against each other, we can multiply through by 2m and compare:
`gain_relative = k_{i,in} - gamma * sigma_c * k_i / (2m)`

### 1.4 Phase 2 -- Refinement

The refinement phase is the key Leiden innovation. It operates within each community found by Phase 1 and may split communities to ensure connectivity.

**Pseudocode** (based on LocalMergingAlgorithm from the reference):

```
function refinement(graph, partition_phase1, rng):
    // Initialize: every node starts as its own singleton sub-community
    refined = new Partition(singletons)

    for each community C in partition_phase1:
        nodes_in_C = { v : partition_phase1.community[v] == C }

        // Visit nodes in C in random order
        node_order = random_permutation(nodes_in_C, rng)

        for node in node_order:
            // Only consider merging with adjacent sub-communities WITHIN C
            // This adjacency check is on the original graph, restricted to C

            // Check: is node well-connected enough to leave its current sub-community?
            // Condition: external_edge_weight >= subcommunity_weight * (total_C_weight - subcommunity_weight) * resolution
            // (This ensures the sub-community remains well-connected after removal)

            // Compute edge weights to neighboring sub-communities within C
            edge_weight_per_sub = {}
            for (neighbor, weight) in graph.neighbors(node):
                if partition_phase1.community[neighbor] == C:  // neighbor must be in same Phase 1 community
                    sub = refined.community[neighbor]
                    if sub != refined.community[node]:  // different sub-community
                        edge_weight_per_sub[sub] += weight

            // Find best sub-community (probabilistic selection with theta parameter)
            // Deterministic version: pick max gain > 0
            best_sub = refined.community[node]  // stay put
            best_gain = 0.0

            for (sub, k_i_in) in edge_weight_per_sub:
                gain = k_i_in - gamma * refined.sub_weight[sub] * graph.degree[node] / (2 * graph.total_weight)
                if gain > best_gain:
                    best_gain = gain
                    best_sub = sub

            // Move node to best sub-community
            if best_sub != refined.community[node]:
                old_sub = refined.community[node]
                refined.sub_weight[old_sub] -= graph.degree[node]
                refined.community[node] = best_sub
                refined.sub_weight[best_sub] += graph.degree[node]

    return refined
```

**Connectivity guarantee**: The refinement phase ensures connected communities through TWO mechanisms:

1. **Singleton initialization**: Every node starts in its own sub-community within C. Merges only happen between adjacent sub-communities.

2. **Adjacency constraint**: A node can only move to a sub-community that contains at least one of its direct neighbors in the original graph (restricted to the Phase 1 community C). This means every merge operation connects two sub-communities that share an edge. By induction, the resulting sub-communities are connected subgraphs.

**Why this guarantees connectivity**: Start from singletons (trivially connected). Each merge step joins node v to a sub-community S only if v has a neighbor in S. So v is connected to S via that edge. Every sub-community grows by accreting adjacent nodes, maintaining a connected subgraph at each step.

**Note on the theta/randomness parameter**: The reference implementation uses a stochastic selection where `P(choosing sub c) proportional to exp(gain_c / theta)`. For our implementation, we use deterministic max-gain selection (theta = 0, equivalent to always picking the best). This is simpler and matches the spec which doesn't mention theta.

### 1.5 Phase 3 -- Aggregation

**Pseudocode**:

```
function aggregate(graph, partition):
    n_communities = count distinct communities in partition
    if n_communities == graph.n_nodes:
        return None  // no aggregation possible, algorithm terminates

    // Build aggregated graph
    agg = new Graph(n_communities nodes)

    for each edge (u, v, w) in graph:
        cu = partition.community[u]
        cv = partition.community[v]
        if cu == cv:
            agg.self_loop_weight[cu] += w  // intra-community edge -> self-loop
        else:
            agg.edge_weight[cu][cv] += w   // inter-community edge

    // Node weights in aggregated graph = sum of node weights in community
    for each community c:
        agg.degree[c] = partition.community_weight[c]

    return agg
```

**Self-loops**: Intra-community edges become self-loops on the super-node. These don't affect modularity gain calculations (they contribute equally regardless of which community the super-node is in) but are needed for correct total weight m.

**Edge weights**: Multiple inter-community edges are summed into a single weighted edge between the two super-nodes.

### 1.6 Outer Loop and Termination

```
function leiden(graph, gamma, seed):
    rng = StdRng::seed_from_u64(seed)
    partition = singleton_partition(graph)

    loop:
        partition = local_moving(graph, partition, rng)

        if no nodes moved in local_moving:
            break  // converged

        refined = refinement(graph, partition, rng)

        if refined.n_communities == graph.n_nodes:
            break  // all singletons, no further aggregation possible

        (aggregated_graph, community_map) = aggregate(graph, refined)
        graph = aggregated_graph
        partition = initial_partition_from(community_map)

    return final_partition  // map back to original nodes
```

**Termination conditions**:
1. Phase 1 makes no moves (partition is locally optimal)
2. After refinement, every node is its own community (no aggregation possible)
3. The aggregated graph has the same number of nodes as the input (no communities merged)

In practice, the algorithm converges in a small number of iterations (typically 2-5).

### 1.7 Undirected Graph Construction

Per the spec, directed code graph edges are converted to undirected:
- Each directed edge (u -> v) with weight 1.0 contributes weight 1.0 to both directions
- Duplicate edges (u->v and v->u from two different directed edges) sum to weight 2.0
- Self-loops are ignored (a symbol calling itself is unusual and doesn't affect community structure)
- Only high-confidence edges: Calls, Extends, Implements, Embeds

The adjacency list stores `neighbors: Vec<Vec<(usize, f64)>>` where each entry is (neighbor_index, weight). For undirected representation, if there's an edge u->v, we add v to u's neighbors AND u to v's neighbors.

## 2. Codebase Integration Points

### 2.1 Loading Edges from the Store

All existing analysis modules follow the same pattern:

```rust
// In use case (e.g., FlowUseCase, RiskUseCase, CloneUseCase):
let symbols = self.store.all_symbols()?;
let edges = self.store.all_edges()?;
// Then pass to pure analysis functions
```

The `GraphStore` trait (in `ports.rs`) provides:
- `all_symbols() -> Result<Vec<SymbolNode>>` -- all symbol nodes
- `all_edges() -> Result<Vec<Edge>>` -- all edges
- `get_symbol(qualified_name) -> Result<Option<SymbolNode>>` -- single symbol lookup

The community use case will follow the same pattern: load all symbols and edges, then call pure analysis functions.

### 2.2 High-Confidence Edge Filter

Already exists in `analysis/flow.rs`:

```rust
fn is_high_confidence(kind: &EdgeKind) -> bool {
    matches!(
        kind,
        EdgeKind::Calls | EdgeKind::Extends | EdgeKind::Implements | EdgeKind::Embeds
    )
}
```

The community module should define its own copy (or we extract a shared utility), since it needs the same filter. The spec confirms: "Edges: High-confidence only (Calls, Extends, Implements, Embeds)".

### 2.3 Config Pattern

Configs follow a consistent pattern:

```rust
// In model.rs -- domain config with defaults
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
```

```rust
// In cli/config.rs -- TOML-deserialized config (all fields Option)
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CommunitiesCliConfig {
    pub resolution: Option<f64>,
    pub min_community_size: Option<usize>,
    pub seed: Option<u64>,
}

// Added to CodeGraphConfig:
pub communities: Option<CommunitiesCliConfig>,
```

```rust
// In CLI command -- merge config.toml defaults with CLI flags
let mut config = CommunityConfig::default();
if let Some(cc) = &file_config.communities {
    if let Some(r) = cc.resolution { config.resolution = r; }
    if let Some(s) = cc.min_community_size { config.min_community_size = s; }
    if let Some(s) = cc.seed { config.seed = s; }
}
// CLI flags override:
if let Some(r) = args.resolution { config.resolution = r; }
// etc.
```

The risk command (`cli/commands/risk.rs`) demonstrates the full pattern: load config from TOML, merge with defaults, override with CLI args.

### 2.4 Use Case Pattern

All use cases follow this structure:

```rust
pub struct CommunityUseCase<S> {
    store: S,
}

impl<S: GraphStore> CommunityUseCase<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn analyze(&self, config: &CommunityConfig) -> Result<CommunityAnalysis> {
        let symbols = self.store.all_symbols()?;
        let edges = self.store.all_edges()?;
        // Call pure analysis functions...
    }
}
```

Key observations from existing use cases:
- `FlowUseCase<S>` takes only `store: S`
- `CloneUseCase<S, F>` takes `store: S, fs: F, root: PathBuf` (needs filesystem for reading source)
- `RiskUseCase<S>` takes only `store: S`

The community use case only needs the store (no filesystem access needed), so it follows the FlowUseCase/RiskUseCase pattern.

### 2.5 Displayable Trait

The `Displayable` trait in `cli/output.rs` has three methods:

```rust
pub trait Displayable {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()>;
    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()>;
    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()>;
}
```

Implementations exist for: `IndexStats`, `GraphStats`, `FlowAnalysis`, `Vec<CriticalityScore>`, `ImpactReport`, `DiffImpactReport`, `CloneAnalysis`, `Vec<CloneCluster>`, `RiskAnalysis`, `Vec<RiskScore>`, `RiskScoreDetail`.

We need implementations for:
- `CommunityAnalysis` -- listing mode (summary + top communities)
- `Vec<Community>` -- single community detail (or could be `Community`)
- Possibly a `CommunityLookup` wrapper for the `--symbol` mode

The JSON output always delegates to serde_json, so all output types must derive `Serialize`.

### 2.6 CLI Command Pattern

From `commands/clones.rs` (simplest reference):

```rust
pub fn run_clones(args: &ClonesArgs, output_format: OutputFormat) -> Result<()> {
    let (store, root) = open_graph()?;
    let fs = RealFileSystem;
    let uc = CloneUseCase::new(store, fs, root);
    let config = CloneConfig { ... };
    let analysis = uc.analyze(&config)?;

    if let Some(cluster_id) = args.cluster {
        // Detail mode: find specific cluster
        if let Some(cluster) = analysis.clusters.iter().find(|c| c.id == cluster_id) {
            print(&vec![cluster.clone()], output_format);
        } else {
            eprintln!("cluster {cluster_id} not found ...");
        }
    } else {
        print(&analysis, output_format);
    }
    Ok(())
}
```

The communities command will have three modes:
1. `code-graph communities` -- list all communities (default)
2. `code-graph communities <id>` -- detail for specific community
3. `code-graph communities --symbol <QNAME>` -- lookup which community a symbol belongs to

### 2.7 Commands Enum and Args

In `commands/mod.rs`, add a new variant to `Commands`:

```rust
/// Detect communities of tightly-coupled symbols
Communities(CommunitiesArgs),
```

And define the args struct:

```rust
#[derive(clap::Args)]
pub struct CommunitiesArgs {
    /// Show details for a specific community
    pub community_id: Option<usize>,
    /// Modularity resolution parameter
    #[arg(long)]
    pub resolution: Option<f64>,
    /// Minimum community size to display
    #[arg(long, default_value = "2")]
    pub min_size: usize,
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

### 2.8 GraphStats Extension

`GraphStats` in `model.rs` already has optional fields for flow, clone, and risk analysis. Need to add:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub community_count: Option<usize>,
#[serde(skip_serializing_if = "Option::is_none")]
pub modularity: Option<f64>,
```

The `InMemoryGraphStore::stats()` in `test_support.rs` and `SqliteStore::stats()` will both need updated `GraphStats` construction. Following the existing pattern, the new fields default to `None` and are populated by the stats command when community analysis is available.

## 3. Rust Ecosystem

### 3.1 `rand` Crate Dependency

The `rand` crate is NOT currently in any Cargo.toml in the workspace. It must be added to `crates/domain/Cargo.toml`:

```toml
[dependencies]
rand = "0.8"
```

Usage: `use rand::rngs::StdRng; use rand::SeedableRng; use rand::seq::SliceRandom;`

- `StdRng::seed_from_u64(seed)` for deterministic RNG
- `slice.shuffle(&mut rng)` for random permutation of node visit order
- When `seed` is `None` in config, use a default seed (e.g., 42) for reproducibility, or use `StdRng::from_entropy()`. The spec says `--seed` produces deterministic results, implying that without `--seed`, results may vary. But for simplicity, a default seed of 42 ensures reproducibility even without the flag. The spec's AC8 says "--seed produces deterministic, reproducible results" -- this is satisfied as long as the same seed gives the same output.

### 3.2 Existing Rust Leiden Crates

No established Rust Leiden crate was found on crates.io. This is expected -- the spec says we implement from scratch.

For reference only (not as dependencies):
- Python: `leidenalg` (wraps C++ igraph implementation)
- Java: `networkanalysis` by CWTSLeiden (reference implementation by the paper authors)
- NetworkX: has Louvain but not Leiden in the standard library

### 3.3 Union-Find Pattern

The codebase has a Union-Find implementation in `analysis/clones.rs::cluster_matches()`:

```rust
let mut parent: Vec<usize> = (0..n).collect();
let mut rank: Vec<usize> = vec![0; n];

fn find(parent: &mut [usize], x: usize) -> usize {
    if parent[x] != x {
        parent[x] = find(parent, parent[x]);
    }
    parent[x]
}

fn union(parent: &mut [usize], rank: &mut [usize], x: usize, y: usize) { ... }
```

We may NOT need Union-Find for Leiden (it uses direct community assignment vectors). However, for the connectivity verification test (AC6), we could use BFS/DFS per community to verify the induced subgraph is connected.

## 4. Testing Strategy

### 4.1 Existing Test Patterns

**Analysis-level tests** (in `analysis/flow.rs`, `analysis/clones.rs`):
- Define helper functions: `make_symbol(...)`, `make_edge(...)`
- Build small test graphs inline
- Call pure analysis functions directly
- Assert on outputs

**Use-case-level tests** (in `use_cases/flow.rs`, `use_cases/clones.rs`):
- Use `InMemoryGraphStore` from `test_support.rs`
- Build store with `store.insert_symbol(...)` and `store.insert_edge(...)`
- Create use case: `let uc = FlowUseCase::new(store);`
- Call use case methods and assert on results

**Key test infrastructure**:
- `InMemoryGraphStore::new()` -- empty in-memory store
- `InMemoryGraphStore::insert_symbol(SymbolNode)` -- add a symbol
- `InMemoryGraphStore::insert_edge(Edge)` -- add an edge
- `MockFileSystem::new(files)` -- for source reading (not needed for communities)

### 4.2 Test Graph Builders for Community Detection

We need several carefully constructed test graphs:

**Triangle graph** (3 nodes, 3 edges forming K3):
- Expected: all in one community

**Two triangles connected by bridge** (6 nodes: K3 + K3 with one edge between them):
- Expected: two communities at default resolution

**Karate club equivalent** (4 cliques of size 5 connected by single edges):
- This is the AC7 multi-scale test graph
- At gamma=2.0: should detect ~4 communities (the individual cliques)
- At gamma=0.5: should detect fewer communities (cliques merge)
- Construction: 4 groups of 5 fully-connected nodes, one bridge edge between adjacent groups

**Star graph** (1 center + N leaves):
- Tests isolated node handling
- Center has high degree, leaves have degree 1

**Empty graph** (nodes but no edges):
- All nodes should be singletons
- `CommunityStats.isolated_nodes` == total nodes

**Single node**:
- One community (singleton), modularity = 0

### 4.3 Testing Connectivity Guarantee (AC6)

For each community in the result:

```rust
fn assert_community_connected(community_members: &[String], edges: &[Edge]) {
    // Build undirected adjacency list restricted to community members
    let member_set: HashSet<&str> = community_members.iter().map(|s| s.as_str()).collect();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for m in &member_set { adj.entry(m).or_default(); }

    for edge in edges {
        if is_high_confidence(&edge.kind)
            && member_set.contains(edge.source.as_str())
            && member_set.contains(edge.target.as_str())
        {
            adj.entry(edge.source.as_str()).or_default().push(edge.target.as_str());
            adj.entry(edge.target.as_str()).or_default().push(edge.source.as_str());
        }
    }

    // BFS from first member
    let start = community_members[0].as_str();
    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    visited.insert(start);
    queue.push_back(start);
    while let Some(node) = queue.pop_front() {
        for &neighbor in adj.get(node).unwrap_or(&vec![]) {
            if visited.insert(neighbor) {
                queue.push_back(neighbor);
            }
        }
    }

    assert_eq!(visited.len(), community_members.len(),
        "Community is not connected: reached {} of {} members",
        visited.len(), community_members.len());
}
```

This test should be run on EVERY non-singleton community returned by the algorithm, using graphs that would cause Louvain to produce disconnected communities (e.g., graphs where nodes can be pulled into communities they're not directly connected to).

### 4.4 Multi-Scale Test Graph for AC7

Construction of the "4 complete subgraphs of size 5 connected by single edges":

```rust
fn build_multiscale_graph() -> (Vec<SymbolNode>, Vec<Edge>) {
    let mut symbols = Vec::new();
    let mut edges = Vec::new();

    // 4 cliques of 5 nodes each
    for clique in 0..4 {
        for i in 0..5 {
            let name = format!("c{clique}_n{i}");
            let qn = format!("src/mod{clique}.rs::{name}");
            symbols.push(make_symbol(&name, &qn, SymbolKind::Function));
        }
        // Fully connect within clique
        for i in 0..5 {
            for j in (i+1)..5 {
                let src = format!("src/mod{clique}.rs::c{clique}_n{i}");
                let tgt = format!("src/mod{clique}.rs::c{clique}_n{j}");
                edges.push(make_edge(EdgeKind::Calls, &src, &tgt));
            }
        }
    }

    // Single bridge edges between adjacent cliques
    for clique in 0..3 {
        let src = format!("src/mod{clique}.rs::c{clique}_n0");
        let tgt = format!("src/mod{}.rs::c{}_n0", clique + 1, clique + 1);
        edges.push(make_edge(EdgeKind::Calls, &src, &tgt));
    }

    (symbols, edges)
}
```

Test assertion:

```rust
#[test]
fn higher_resolution_yields_more_communities() {
    let (symbols, edges) = build_multiscale_graph();

    let result_low = leiden(&symbols, &edges, 0.5, Some(42));
    let result_high = leiden(&symbols, &edges, 2.0, Some(42));

    assert!(result_high.communities.len() > result_low.communities.len(),
        "gamma=2.0 should produce more communities than gamma=0.5: got {} vs {}",
        result_high.communities.len(), result_low.communities.len());
}
```

### 4.5 Determinism Test (AC8)

```rust
#[test]
fn same_seed_produces_identical_results() {
    let (symbols, edges) = build_multiscale_graph();

    let result1 = leiden(&symbols, &edges, 1.0, Some(42));
    let result2 = leiden(&symbols, &edges, 1.0, Some(42));

    assert_eq!(result1.communities.len(), result2.communities.len());
    for (c1, c2) in result1.communities.iter().zip(result2.communities.iter()) {
        assert_eq!(c1.members, c2.members);
    }
    assert!((result1.modularity - result2.modularity).abs() < f64::EPSILON);
}
```

### 4.6 Empty Graph and Edge Cases

```rust
#[test]
fn empty_graph_returns_zero_communities() {
    let result = leiden(&[], &[], 1.0, Some(42));
    assert!(result.communities.is_empty());
    assert_eq!(result.stats.count, 0);
    assert_eq!(result.stats.isolated_nodes, 0);
}

#[test]
fn single_node_is_singleton() {
    let symbols = vec![make_symbol("a", "a.rs::a", SymbolKind::Function)];
    let result = leiden(&symbols, &[], 1.0, Some(42));
    assert_eq!(result.stats.isolated_nodes, 1);
    // With min_community_size = 2, no communities displayed
    // But the node exists as a singleton internally
}

#[test]
fn isolated_nodes_counted_correctly() {
    let symbols = vec![
        make_symbol("a", "a.rs::a", SymbolKind::Function),
        make_symbol("b", "b.rs::b", SymbolKind::Function),
        make_symbol("c", "c.rs::c", SymbolKind::Function),
    ];
    let edges = vec![make_edge(EdgeKind::Calls, "a.rs::a", "b.rs::b")];
    let result = leiden(&symbols, &edges, 1.0, Some(42));
    // c has no high-confidence edges, so it's isolated
    assert_eq!(result.stats.isolated_nodes, 1);
}
```

## 5. Implementation Architecture Summary

### 5.1 New Files

| File | Purpose |
|------|---------|
| `crates/domain/src/analysis/community.rs` | Pure Leiden algorithm: `LeidenGraph`, `Partition`, `leiden()`, `modularity()`, connectivity check |
| `crates/domain/src/use_cases/community.rs` | `CommunityUseCase<S: GraphStore>` with `analyze()` and `community_of()` |
| `crates/cli/src/commands/communities.rs` | `run_communities()` CLI handler |

### 5.2 Modified Files

| File | Change |
|------|--------|
| `crates/domain/src/analysis/mod.rs` | Add `pub mod community;` |
| `crates/domain/src/use_cases/mod.rs` | Add `pub mod community;` |
| `crates/domain/src/model.rs` | Add `CommunityConfig`, `Community`, `CommunityAnalysis`, `CommunityStats`; extend `GraphStats` |
| `crates/domain/Cargo.toml` | Add `rand = "0.8"` to `[dependencies]` |
| `crates/cli/src/commands/mod.rs` | Add `pub mod communities;` and `Communities(CommunitiesArgs)` to enum |
| `crates/cli/src/config.rs` | Add `CommunitiesCliConfig` and field to `CodeGraphConfig` |
| `crates/cli/src/output.rs` | Implement `Displayable` for `CommunityAnalysis` and supporting types |
| `crates/domain/src/test_support.rs` | Update `InMemoryGraphStore::stats()` to include new `GraphStats` fields |

### 5.3 Internal Algorithm Types

```rust
// Private to analysis/community.rs

/// Adjacency-list representation optimized for modularity computation
struct LeidenGraph {
    n: usize,
    neighbors: Vec<Vec<(usize, f64)>>,  // adjacency list with weights
    degree: Vec<f64>,                    // weighted degree per node
    total_weight: f64,                   // sum of all edge weights (m in formula)
}

/// Node-to-community assignment with cached community weights
struct Partition {
    n: usize,
    community: Vec<usize>,              // node -> community ID
    community_weight: Vec<f64>,         // sum of degrees per community (sigma_tot)
}
```

### 5.4 Community Naming Algorithm

```rust
fn derive_community_name(members: &[String], community_id: usize) -> String {
    let generic_names = ["mod", "lib", "index", "main"];

    // Count file occurrences
    let mut file_counts: HashMap<&str, usize> = HashMap::new();
    for member in members {
        // qualified_name format: "path/to/file.rs::SymbolName"
        if let Some(file) = member.split("::").next() {
            *file_counts.entry(file).or_default() += 1;
        }
    }

    // Find most common file
    let best_file = file_counts.into_iter()
        .max_by_key(|(name, count)| (*count, std::cmp::Reverse(*name)))
        .map(|(name, _)| name);

    if let Some(file) = best_file {
        // Extract stem
        let stem = Path::new(file)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if !stem.is_empty() && !generic_names.contains(&stem) {
            return stem.to_string();
        }
    }

    format!("community_{community_id}")
}
```

### 5.5 Performance Considerations

For AC11 (10k symbols, avg degree < 20, under 2 seconds):

- The Leiden algorithm is O(n * avg_degree * iterations) per outer iteration
- With n=10k, avg_degree=20, and ~3 iterations, that's ~600k operations per outer iteration
- Typically 3-5 outer iterations, so ~3M operations total
- Well within 2 seconds for Rust

Key optimizations:
- Use `Vec<Vec<(usize, f64)>>` adjacency list (cache-friendly)
- Maintain `community_weight` incrementally (O(1) per move)
- Use index-based node IDs (not string-based) for the internal algorithm
- Map qualified_name strings to `usize` indices at the boundary only

### 5.6 Modularity Computation for Output

After the algorithm converges, compute the final modularity score for reporting:

```rust
fn compute_modularity(graph: &LeidenGraph, partition: &Partition, gamma: f64) -> f64 {
    let m2 = 2.0 * graph.total_weight;
    if m2 == 0.0 { return 0.0; }

    let mut q = 0.0;
    for u in 0..graph.n {
        for &(v, w) in &graph.neighbors[u] {
            if partition.community[u] == partition.community[v] {
                q += w - gamma * graph.degree[u] * graph.degree[v] / m2;
            }
        }
    }
    q / m2
}
```

Note: Since the graph is stored as undirected (each edge appears in both directions), each pair (u,v) is counted twice. The `1/(2m)` factor in the modularity formula accounts for this.

## 6. Key Risks and Decisions

### 6.1 CPM vs Modularity

The reference implementation uses CPM (Constant Potts Model), not standard modularity. The incremental gain formulas differ:
- **CPM**: `gain = k_{i,in} - gamma * k_i * sigma_C` (simple, no 2m denominator)
- **Modularity**: `gain = k_{i,in} / (2m) - gamma * sigma_C * k_i / (2m^2)`

The spec explicitly uses the modularity formula. For the incremental gain comparison, since we're comparing gains to find the best community (not computing absolute Q), we can factor out the common `1/(2m)` term:

```
gain_compare = k_{i,in} - gamma * sigma_C * k_i / (2m)
```

This is the form we should use in both Phase 1 and Phase 2.

### 6.2 Theta Parameter (Stochastic Refinement)

The original Leiden uses a stochastic selection in the refinement phase (controlled by theta). The spec does not mention theta, and the `CommunityConfig` struct doesn't include it. We should use deterministic max-gain selection (equivalent to theta -> 0). This simplifies the implementation and is consistent with the spec.

### 6.3 Node Weight in Undirected Graph

For an undirected graph built from directed edges where each directed edge contributes weight 1.0 in both directions:
- `degree[v]` = sum of weights of all edges incident to v in the undirected representation
- If node v has 3 outgoing Calls edges and 2 incoming Calls edges (from different nodes), its undirected degree = 5 * 1.0 = 5.0
- If there's a bidirectional edge (u->v and v->u), it appears once as undirected edge (u,v) with weight 2.0, contributing 2.0 to both u's and v's degree

### 6.4 Self-Loop Handling

The spec says to start with singleton communities. In the initial graph, there should be no self-loops (a symbol doesn't typically call itself). Self-loops only appear in the aggregated graph to represent intra-community edges. The `total_weight` (m) should include self-loop weight.

### 6.5 Well-Connected Check in Refinement

The reference implementation has an additional check before allowing a node to leave its sub-community in refinement: the node's external edge weight must be sufficient relative to the sub-community weight. This prevents breaking apart well-connected sub-communities. For simplicity, and since our singleton initialization already provides the connectivity guarantee through adjacency-only merges, we can omit this check initially. If needed, it can be added later.
