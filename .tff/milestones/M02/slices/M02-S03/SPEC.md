# Spec — M02-S03: Community Detection

## Problem Statement

The Code Graph has no way to identify natural clusters of tightly-coupled symbols. Developers exploring unfamiliar codebases lack structural guidance on module boundaries. AI agents must process the entire graph without knowing which symbols form cohesive units.

**Solution**: Implement the Leiden algorithm to partition the symbol graph into communities — groups of symbols with dense internal connections and sparse external connections. Expose via `code-graph communities` CLI command with compact/table/json output.

**Who benefits**: Developers doing refactoring/code review, architects assessing modularity, AI coding agents needing scoped context windows.

## Approach & Algorithm

### Leiden Algorithm (Traag et al., 2019)

Three-phase iterative refinement:

**Phase 1 — Local Moving**: Visit each node in random order. For each node, compute the modularity gain of moving it to each neighbor's community. Move to the community with maximum positive gain. Repeat until no moves improve modularity.

**Phase 2 — Refinement**: For each community C from Phase 1:
1. Initialize every node in C as its own singleton sub-community
2. For each node v in C (random order), compute the modularity gain of merging v's sub-community with each *adjacent* sub-community within C (adjacency checked on the original non-aggregated graph restricted to C's members)
3. Move v to the best adjacent sub-community if gain > 0
4. The adjacency constraint ensures that every resulting sub-community is connected — this is the key Leiden invariant that prevents Louvain's "poorly connected community" problem
5. The refined sub-communities become the new communities for Phase 3

Reference: Traag, Waltman & van Eck (2019), Section 4 — "Refinement phase."

**Phase 3 — Aggregation**: Collapse each community into a single super-node. Edge weights between super-nodes = sum of inter-community edges. Self-loops = sum of intra-community edges. Return to Phase 1 on the aggregated graph.

**Termination**: When no Phase 1 moves improve modularity on the aggregated graph.

### Graph Construction

- **Nodes**: All symbols from `GraphStore::all_symbols()`
- **Edges**: High-confidence only (Calls, Extends, Implements, Embeds)
- **Undirected**: Each directed edge contributes weight 1.0 in both directions (standard for modularity optimization)
- **Initialization**: Each node starts in its own singleton community (community[i] = i). This ensures isolated nodes remain singletons and the connectivity invariant holds trivially
- **Resolution parameter** γ (default 1.0) controls community granularity: higher γ → smaller communities
- **Dependency**: `rand` crate with `StdRng::seed_from_u64` for deterministic node ordering in Phases 1 and 2

### Modularity Formula

Q = (1/2m) Σᵢⱼ [Aᵢⱼ - γ(kᵢkⱼ/2m)] δ(cᵢ, cⱼ)

Where: m = total edge weight, A = adjacency matrix, k = node degree, c = community assignment, δ = Kronecker delta.

## Architecture

### New Files

| File | Purpose |
|------|---------|
| `crates/domain/src/analysis/community.rs` | Pure Leiden algorithm + modularity computation |
| `crates/domain/src/use_cases/community.rs` | `CommunityUseCase<S: GraphStore>` orchestration |
| `crates/cli/src/commands/communities.rs` | CLI command + output formatting |

### Modified Files

| File | Change |
|------|--------|
| `crates/domain/src/analysis/mod.rs` | Add `pub mod community;` |
| `crates/domain/src/use_cases/mod.rs` | Add `pub mod community;` |
| `crates/domain/src/model.rs` | Add community types + `community_count`/`modularity` to `GraphStats` |
| `crates/cli/src/commands/mod.rs` | Add `Communities` variant to `Commands` enum |
| `crates/cli/src/config.rs` | Add `CommunitiesConfig` to `CodeGraphConfig` (field: `pub communities: Option<CommunitiesConfig>`) |
| `crates/cli/src/output.rs` | Implement `Displayable` for `CommunityAnalysis` |

### Key Types

```rust
// Config
pub struct CommunityConfig {
    pub resolution: f64,          // default 1.0
    pub min_community_size: usize, // default 2
    pub seed: Option<u64>,         // for reproducibility
}

// Output
pub struct Community {
    pub id: usize,
    pub name: String,                    // derived from common file path prefix
    pub members: Vec<String>,            // qualified names as strings (matches existing analysis types)
    pub modularity_contribution: f64,
    pub internal_edges: usize,
    pub boundary_edges: usize,
}

pub struct CommunityAnalysis {
    pub communities: Vec<Community>,
    pub modularity: f64,
    pub stats: CommunityStats,
}

pub struct CommunityStats {
    pub count: usize,
    pub avg_size: f64,
    pub largest_size: usize,
    pub isolated_nodes: usize,
}
```

### Use Case Interface

```rust
impl<S: GraphStore> CommunityUseCase<S> {
    pub fn analyze(&self, config: &CommunityConfig) -> Result<CommunityAnalysis>;
    pub fn community_of(&self, symbol: &str, config: &CommunityConfig) -> Result<Option<Community>>;
}
```

### Internal Algorithm Types (private to analysis/community.rs)

```rust
struct LeidenGraph {
    neighbors: Vec<Vec<(usize, f64)>>,  // adjacency list with weights
    degree: Vec<f64>,                    // weighted degree per node
    total_weight: f64,                   // sum of all edge weights
}

struct Partition {
    community: Vec<usize>,              // node → community mapping
    community_weights: Vec<f64>,        // sum of degrees per community
}
```

## CLI Interface

```
code-graph communities [OPTIONS] [COMMUNITY_ID]

Arguments:
  [COMMUNITY_ID]    Show details for a specific community

Options:
  --resolution <F>       Modularity resolution parameter [default: 1.0]
  --min-size <N>         Minimum community size to display [default: 2]
  --seed <N>             Random seed for reproducibility
  --symbol <QNAME>       Show which community a symbol belongs to
  --limit <N>            Maximum communities to display [default: 20]
```

Output format is controlled by the existing global `--json` / `--table` flags (same as all other commands). Default is compact.

### Output Examples

**Compact (listing)**:
```
Communities: 12 (modularity: 0.73)

 #1  auth (8 symbols, 15 internal / 3 boundary edges)
     src/auth.rs::AuthService, src/auth.rs::validate_token, ...
 #2  parser (6 symbols, 10 internal / 2 boundary edges)
     src/parser.rs::Parser, src/parser.rs::parse_expr, ...
 ...
```

**Compact (community detail)**:
```
Community #1: auth (8 symbols)
Modularity contribution: 0.12
Internal edges: 15 | Boundary edges: 3

Members:
  src/auth.rs::AuthService        (Function, 5 internal edges)
  src/auth.rs::validate_token     (Function, 4 internal edges)
  ...
```

**Compact (--symbol lookup)**:
```
src/auth.rs::validate_token → Community #1 (auth, 8 members)
```

**Table**:
```
 ID  Name     Size  Internal  Boundary  Modularity
  1  auth        8        15         3       0.12
  2  parser      6        10         2       0.09
  3  storage     5         8         4       0.07
```

### Community Naming

Algorithm: (1) find the most frequently occurring file among members; (2) extract the file stem (filename without extension); (3) if tied, use first alphabetically; (4) if the result is generic (e.g., "mod", "lib", "index", "main"), fall back to `community_<id>`.

## Configuration

In `.code-graph/config.toml`:

```toml
[communities]
resolution = 1.0
min_community_size = 2
seed = 42
```

## Acceptance Criteria

1. `code-graph communities` lists all detected communities with member counts, sorted by size descending
2. `code-graph communities <id>` shows full member list for a specific community
3. `code-graph communities --symbol <QNAME>` shows which community a symbol belongs to
4. All three output formats produce valid output: compact matches documented format, table includes columns (id, name, size, internal_edges, boundary_edges), json is valid JSON matching `CommunityAnalysis` schema
5. Global modularity score Q is reported. For default resolution (gamma=1.0), Q >= 0. Q > 0 is expected when the partition has more than one community, though not guaranteed for all graph topologies
6. Every community is internally connected (Leiden guarantee — no disconnected sub-groups within a community). Test: for each community, the induced subgraph using the same undirected representation as the algorithm forms a single connected component
7. For a carefully constructed test graph with clear multi-scale structure (e.g., 4 complete subgraphs of size 5 connected by single edges), communities at gamma=2.0 are more numerous than at gamma=0.5
8. `--seed` produces deterministic, reproducible results (identical output on repeated runs)
9. `--min-size` filters communities below the threshold from display output (does not affect modularity computation)
10. Values in `[communities]` config section (`resolution`, `min_community_size`, `seed`) are used as defaults when CLI flags are omitted; CLI flags override config values
11. Performance: handles 10k-symbol graphs with typical code graph density (avg degree < 20) in under 2 seconds
12. Returns a clear error message for nonexistent community IDs or unknown symbols; on an empty graph, reports zero communities without error
13. Community names are derived from the most common file path prefix among members; communities with no common prefix use `community_<id>` as fallback
14. Symbols with no high-confidence edges are reported as isolated in `CommunityStats.isolated_nodes` and placed in singleton communities (filtered by `--min-size`)

## Non-Goals

- Hierarchical/multi-level community output (single-level partition only)
- Overlapping communities (each symbol belongs to exactly one community)
- Persistent community storage in SQLite (computed on-demand like flows/risk/clones)
- File-level community view (can be derived but not a first-class feature)
- Visualization / graphical output
