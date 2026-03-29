# Research — M02-S01: Execution Flows

## Summary

All spec'd types, algorithms, and integration points have clear homes in the existing architecture. No new dependencies required. The main engineering challenges are: (1) a new per-path-visited DFS distinct from the existing global-visited DFS, (2) Brandes' betweenness centrality as a new algorithm with no prior codebase precedent, and (3) extending `GraphStats` and config without breaking existing consumers.

---

## 1. Domain Model Integration

### Existing types to leverage

| Type | Location | Relevance |
|---|---|---|
| `SymbolNode` | `model.rs` | Entry point classification uses `kind`, `name`, `is_test`, `is_exported`, `visibility`, `decorators` |
| `Edge` | `model.rs` | Flow traversal filters on `edge.kind.confidence() == High` |
| `EdgeKind` | `model.rs` | High-confidence behavioral edges: `Calls`, `Extends`, `Implements`, `Embeds` |
| `Confidence` | `model.rs` | `PartialOrd` ordering: `Structural < Low < Medium < High` |
| `GraphStats` | `model.rs` | Needs two new optional fields: `entry_point_count`, `avg_criticality` |
| `Direction` | `model.rs` | `Forward` / `Backward` — used for backward BFS in `flows_through` optimization |
| `SymbolKind` | `model.rs` | 14 variants including `Function`, `Method`, `Class`, `Struct`, `Test` |
| `Visibility` | `model.rs` | `Public`, `Private`, `Crate` |

### New types to add in `model.rs`

All 6 types from the spec: `EntryPoint`, `EntryPointKind`, `ExecutionFlow`, `CriticalityScore`, `FlowAnalysis`, `FlowStats`, `FlowConfig`. These are pure domain types with no storage dependency — they are computed on-demand and never persisted.

### GraphStats extension

Current struct:
```rust
pub struct GraphStats {
    pub files: usize,
    pub symbols: usize,
    pub edges: usize,
}
```

Add two optional fields:
```rust
pub entry_point_count: Option<usize>,
pub avg_criticality: Option<f64>,
```

**Impact of change:**
- `crates/storage/src/graph_store.rs` — `stats()` currently returns the 3 counts. Must populate new fields (or set to `None` and let CLI compute on-demand).
- `crates/domain/src/test_support.rs` — `InMemoryGraphStore::stats()` returns hardcoded values; needs `None` defaults.
- `crates/cli/src/output.rs` — `Displayable` impl for `GraphStats` needs to render new fields.
- `crates/cli/src/commands/stats.rs` — must instantiate `FlowUseCase` to compute on-demand values.
- Serde: `#[serde(skip_serializing_if = "Option::is_none")]` for backward-compatible JSON.

---

## 2. Analysis Module Integration

### Existing analysis architecture

```
crates/domain/src/analysis/
  mod.rs            — pub mod blast_radius; change_detection; impact;
  blast_radius.rs   — BFS forward from targets, confidence-filtered
  change_detection.rs — DiffHunk → affected SymbolNodes
  impact.rs         — combines change_detection + blast_radius
```

### New module: `analysis/flow.rs`

Register via `pub mod flow;` in `analysis/mod.rs`. Contains three core algorithms:

1. **Entry point detection** — symbol scan + edge check, no graph traversal needed
2. **Brandes' betweenness centrality** — BFS from every node, O(V*E)
3. **Flow enumeration** — bounded DFS with per-path visited set

### Traversal: existing vs. new

The existing `InMemoryGraph` (in `traversal.rs`) provides:
- `bfs()` / `bfs_filtered()` — **global visited set**, returns `TraversalResult`
- `dfs()` — **global visited set**, returns `TraversalResult`

Flow enumeration needs a **per-path visited set** (same node allowed in different paths). This is a fundamentally different traversal strategy. Per the spec: implement a **new DFS** in `analysis/flow.rs` — do NOT modify existing `traversal.rs`.

Brandes' algorithm also needs its own BFS (tracking predecessor lists and sigma counts), which differs from the existing `bfs_inner`. Also implemented fresh in `analysis/flow.rs`.

### InMemoryGraph reuse

Both algorithms need the directed subgraph of behavioral edges. The existing `InMemoryGraph::from_edges()` pattern (used in `blast_radius.rs` and `use_cases/impact.rs`) builds adjacency maps from an edge iterator. However, we need to **filter to High-confidence edges only** before building the graph.

Pattern from `use_cases/impact.rs`:
```rust
let mut graph = InMemoryGraph::new();
store.edges_streaming(&mut |edge| {
    graph.add_edge(edge.source, edge.target, edge.kind);
    Ok(())
})?;
```

For flow analysis, filter during streaming:
```rust
store.edges_streaming(&mut |edge| {
    if edge.kind.confidence() == Confidence::High {
        graph.add_edge(edge.source, edge.target, edge.kind);
    }
    Ok(())
})?;
```

This is efficient — single pass, no intermediate allocation.

---

## 3. Use Case Layer

### Existing pattern

```rust
pub struct ImpactUseCase<S> {
    store: S,
}
impl<S: GraphStore> ImpactUseCase<S> {
    pub fn new(store: S) -> Self { Self { store } }
    pub fn blast_radius(&self, ...) -> Result<ImpactReport> { ... }
}
```

### New: `use_cases/flow.rs`

```rust
pub struct FlowUseCase<S: GraphStore> {
    store: Arc<S>,  // or S directly — existing uses S by value
}
```

**Decision: `S` vs `Arc<S>`?** Existing use cases take `S` by value (moved in). `ImpactUseCase<S>` stores `S` directly. Follow the same pattern — take `S` by value. The spec says `Arc<S>` but existing code doesn't use Arc in use cases. **Follow existing convention: store `S` by value.**

Three methods: `analyze()`, `flows_through()`, `criticality()`. All build InMemoryGraph from store, run analysis, return results.

### Port methods needed

The spec confirms: **no new GraphStore methods needed**. `all_symbols()`, `all_edges()`, and `edges_streaming()` are sufficient.

---

## 4. CLI Integration

### Command registration pattern

1. Add `FlowsArgs` struct in `commands/mod.rs`
2. Add `Flows(FlowsArgs)` variant to `Commands` enum
3. Create `commands/flows.rs` with `run_flows()`
4. Wire in `lib.rs::run()` match arm
5. Implement `Displayable` for `FlowAnalysis` / `Vec<CriticalityScore>` in `output.rs`

### FlowsArgs design (from spec)

```rust
#[derive(clap::Args)]
pub struct FlowsArgs {
    /// Filter flows through a specific symbol
    #[arg(long)]
    pub symbol: Option<String>,
    /// Show criticality ranking instead of flows
    #[arg(long)]
    pub rank: bool,
    /// Maximum flow depth
    #[arg(long, default_value = "20")]
    pub depth: usize,
    /// Maximum number of results to display
    #[arg(long, default_value = "20")]
    pub limit: usize,
}
```

Global `--json` / `--table` flags already handled by `OutputFormat::from_flags()`.

### Stats integration

`commands/stats.rs` currently:
```rust
let uc = QueryUseCase::new(store.clone(), store);
let stats = uc.stats()?;
```

After integration, it needs to also instantiate `FlowUseCase` to compute entry point count and (conditionally) avg criticality. The 5000-symbol threshold for auto-computing criticality lives in the CLI layer (not domain).

### Output formatting

Three output modes per the spec:
- **Default flows**: entry point summary + top N flows as `[i] a -> b -> c (depth N)`
- **`--symbol` filter**: flows through symbol + filtered list
- **`--rank`**: table with `# Symbol Betweenness Flows Entry?` columns

Each needs `fmt_compact`, `fmt_table`, `fmt_json` implementations.

---

## 5. Configuration

### Current config structure

```rust
pub struct CodeGraphConfig {
    pub index: Option<IndexConfig>,
    pub search: Option<SearchConfig>,
    pub watch: Option<WatchConfig>,
}
```

### New: `FlowConfig` section

Add to `CodeGraphConfig`:
```rust
pub flows: Option<FlowsConfig>,
```

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FlowsConfig {
    pub extra_entry_points: Option<Vec<String>>,
    pub excluded_entry_points: Option<Vec<String>>,
}
```

The algorithmic defaults (max_depth=20, max_flows=1000, visit_budget=100_000, max_public_roots=50) live in `FlowConfig::default()` in the domain layer — not in the TOML config. Only entry point overrides are user-configurable.

---

## 6. Risks and Mitigations

### R1: Performance of Brandes' algorithm — O(V*E)

On a graph with 5000 symbols and ~15000 edges, this is ~75M operations. Rust should handle this in <1s. For larger graphs, the spec caps auto-computation at 5000 symbols in stats; explicit `--rank` runs regardless.

**Mitigation:** The 5000-symbol threshold in stats is a soft guard. Profile on the project's own codebase during implementation.

### R2: Flow enumeration explosion

Fan-out from entry points can produce combinatorial paths. Three bounds prevent this: depth (20), global flow cap (1000), per-entry visit budget (100K steps).

**Mitigation:** The truncation flag on entry points communicates when results are incomplete. Test with high-fan-out fixtures.

### R3: GraphStats backward compatibility

Adding optional fields to `GraphStats` changes its serde output. Existing consumers parsing `--json` output may break if they use strict deserialization.

**Mitigation:** `#[serde(skip_serializing_if = "Option::is_none")]` ensures old fields remain unchanged when new fields are `None`. Document the addition in changelog.

### R4: Per-path vs. global visited semantics

The new DFS must use per-path visited tracking — a node visited in path A must be visitable in path B. This is fundamentally different from the existing DFS. Confusing the two would produce incorrect results.

**Mitigation:** Implement as a separate function in `analysis/flow.rs`, clearly documented. Never modify `traversal.rs`.

---

## 7. Dependencies

**No new crate dependencies required.** All algorithms (Brandes, DFS, entry point detection) are implementable with stdlib data structures (`HashMap`, `HashSet`, `VecDeque`, `Vec`).

---

## 8. File Change Summary

| File | Change | Risk |
|---|---|---|
| `crates/domain/src/model.rs` | Add 7 new types + extend GraphStats | Low — additive |
| `crates/domain/src/analysis/mod.rs` | Add `pub mod flow;` | None |
| `crates/domain/src/analysis/flow.rs` | **NEW** — 3 algorithms (~400-500 lines) | Medium — core logic |
| `crates/domain/src/use_cases/mod.rs` | Add `pub mod flow;` | None |
| `crates/domain/src/use_cases/flow.rs` | **NEW** — FlowUseCase (~100 lines) | Low |
| `crates/domain/src/lib.rs` | Re-export new types | None |
| `crates/cli/src/commands/mod.rs` | Add Flows variant + FlowsArgs | Low |
| `crates/cli/src/commands/flows.rs` | **NEW** — CLI handler (~80 lines) | Low |
| `crates/cli/src/commands/stats.rs` | Instantiate FlowUseCase for on-demand fields | Low |
| `crates/cli/src/output.rs` | Displayable impls for flow types + GraphStats extension | Medium — formatting |
| `crates/cli/src/config.rs` | Add FlowsConfig section | Low |
| `crates/cli/src/lib.rs` | Wire Flows command | None |
| `crates/storage/src/graph_store.rs` | Update stats() for new GraphStats fields | Low |
| `crates/domain/src/test_support.rs` | Update mock stats() | Low |
