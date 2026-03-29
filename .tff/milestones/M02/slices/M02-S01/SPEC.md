# M02-S01: Execution Flows — Design Spec

## Problem

Users need two capabilities from the code graph:
1. **Critical path understanding** — trace execution flows from entry points (HTTP handlers, main, CLI commands) through the codebase to understand what code participates in key paths.
2. **Review prioritization** — rank symbols by criticality (how many flows pass through them) to focus code review and risk assessment on high-impact code.

## Approach

**Betweenness centrality + bounded DFS flow enumeration.**

- **Criticality scoring:** Brandes' algorithm computes betweenness centrality from **all nodes** in the reachable subgraph (not just entry points). O(V·E), single-pass BFS per source. Scores normalized to [0.0, 1.0] using the directed graph normalization factor `(n-1)(n-2)`.
- **Flow enumeration:** DFS from entry points through behavioral edges (High confidence only: `Calls`, `Extends`, `Implements`, `Embeds`) with per-path cycle detection. Bounded by configurable depth limit (default 20), global flow count cap (default 1000), and per-entry-point visit budget (default 100,000 DFS steps).
- **Entry point detection:** Auto-detect from SymbolKind + naming conventions, with config overrides.

### Why not alternatives?
- **Forward BFS only:** Exponential path explosion on graphs with fan-out. Unusable for scoring.
- **PageRank-style:** No natural "flow" listing. Harder to interpret scores. Doesn't match the mental model of entry → exit paths.

## Domain Model Additions

### New types in `crates/domain/src/model.rs`

```rust
struct EntryPoint {
    qualified_name: String,
    kind: EntryPointKind,
    confidence: f64,        // 1.0 for explicit (main), 0.7 for heuristic (public-root)
}

enum EntryPointKind {
    Main,         // main(), #[tokio::main], if __name__ == "__main__"
    Test,         // #[test], test_ prefix, describe/it blocks
    HttpHandler,  // @Get, @Post, axum handlers, flask routes
    CliCommand,   // clap command handlers, argparse
    PublicRoot,   // exported public symbol with zero incoming Calls edges
}

struct ExecutionFlow {
    entry: String,          // entry point qualified_name
    path: Vec<String>,      // ordered qualified_names: entry at [0], terminal at [last]
    depth: usize,
    truncated: bool,        // true if entry point's visit budget was exhausted
}
// terminal() -> path.last() — no redundant field

struct CriticalityScore {
    qualified_name: String,
    betweenness: f64,       // normalized betweenness centrality [0.0, 1.0]
    flow_count: usize,      // number of flows passing through this node
    is_entry_point: bool,
}

struct FlowAnalysis {
    entry_points: Vec<EntryPoint>,
    flows: Vec<ExecutionFlow>,
    criticality: Vec<CriticalityScore>,
    stats: FlowStats,
}

struct FlowStats {
    total_entry_points: usize,
    total_flows: usize,
    max_depth: usize,
    avg_depth: f64,
}
```

## Analysis Algorithm

### New module: `crates/domain/src/analysis/flow.rs`

**Entry Point Detection:**
1. Load all symbols from GraphStore
2. Classify by SymbolKind + naming conventions → EntryPointKind
3. For PublicRoot: find exported public symbols with zero incoming Calls edges
4. Apply config overrides (extra / excluded entry points)

**Per-language matching rules:**
- **Main:** `name == "main"` for Rust/Go/Python, `decorators` contains `tokio::main` for Rust, source contains `if __name__ == "__main__"` pattern detected by parser as a top-level function call
- **Test:** `is_test == true` (already set by parsers), OR `name` starts with `test_` (Python), OR `kind == SymbolKind::Test`
- **HttpHandler:** `decorators` contains any of: `Get`, `Post`, `Put`, `Delete`, `Patch`, `app.route`, `router.get`, `router.post`, `api_view`, `route` (case-insensitive substring match on decorator strings)
- **CliCommand:** `decorators` contains `command`, `subcommand`, `clap`, OR parent symbol has decorator containing `clap` / `Command` / `Parser`
- **PublicRoot:** `is_exported == true` AND `visibility == Public` AND zero incoming `Calls` edges in the graph AND `kind` is one of `Function`, `Method`, `Class`, `Struct` (excludes types, constants, enums to avoid flooding). Capped at 50 PublicRoot entry points per project — if more are detected, take only the top 50 by outgoing edge count (most connected = most likely true entry points). Confidence 0.7 (heuristic).

**Betweenness Centrality (Brandes' algorithm):**
1. Build directed subgraph from behavioral edges only (High confidence: `Calls`, `Extends`, `Implements`, `Embeds`). Medium-confidence edges (`ImportsFrom`, `TypeReference`, etc.) are excluded from centrality computation because they represent structural coupling, not execution flow — following a `TypeReference` edge would produce phantom flows.
2. For **every node** in the reachable subgraph, run BFS to compute shortest paths (standard Brandes — not entry-point-only, which would compute a different metric)
3. Accumulate centrality contributions via back-propagation
4. Normalize by `(n-1)(n-2)` — the **directed graph** normalization factor (not `(n-1)(n-2)/2` which is for undirected graphs)
5. Disconnected components: nodes unreachable from any other node get betweenness 0.0 (mathematically correct; documented in output)

**Flow Enumeration (bounded DFS):**
1. From each entry point, DFS forward through behavioral edges (High confidence only)
2. Per-path visited set for cycle detection (same node allowed in different flows)
3. Terminal = node with no outgoing behavioral edges
4. **Three bounds** to prevent explosion:
   - Depth limit per flow (default 20)
   - Global flow count cap across all entry points (default 1000)
   - Per-entry-point visit budget (default 100,000 DFS steps) — caps total search work, not just output. If budget exhausted, entry point is marked as "truncated" in output.
5. Sort by entry point importance × path length
6. Note: the existing `InMemoryGraph::dfs()` uses a global visited set. Flow enumeration requires per-path visited tracking, so a **new DFS implementation** is needed in `analysis/flow.rs` (does not modify existing traversal code).

**`flows_through` optimization:**
1. First, backward BFS from target symbol to find which entry points can reach it
2. Then, DFS only from those reachable entry points, filtering to paths containing the target
3. This avoids running full flow enumeration when only a subset of entry points is relevant

**Edge types traversed (both betweenness and flow enumeration):**
- High (behavioral): Calls, Extends, Implements, Embeds
- NOT Medium (structural coupling): ImportsFrom, TypeReference, ReExport, etc. — these do not represent execution flow

## Use Case

### New: `crates/domain/src/use_cases/flow.rs`

```rust
struct FlowUseCase<S: GraphStore> {
    store: Arc<S>,
}

impl<S: GraphStore> FlowUseCase<S> {
    fn analyze(&self, config: &FlowConfig) -> Result<FlowAnalysis>;
    fn flows_through(&self, qualified_name: &str, config: &FlowConfig) -> Result<Vec<ExecutionFlow>>;
    fn criticality(&self) -> Result<Vec<CriticalityScore>>;
}

struct FlowConfig {
    max_depth: usize,                    // default 20
    max_flows: usize,                    // default 1000 (global cap)
    visit_budget: usize,                 // default 100_000 (per-entry-point DFS steps)
    max_public_roots: usize,             // default 50 (PublicRoot entry point cap)
    extra_entry_points: Vec<String>,     // from config
    excluded_entry_points: Vec<String>,
}
```

**No new port methods needed.** GraphStore already provides all_symbols(), all_edges(), edges_streaming().

**Stats integration:** GraphStats gains optional fields:
- `entry_point_count: Option<usize>`
- `avg_criticality: Option<f64>`

Stats computes these on-demand via a lightweight path: entry point detection (fast — symbol scan + edge check) runs always, but avg_criticality runs full Brandes' only if the graph has <= 5000 symbols. Above that threshold, avg_criticality displays as `None` / "N/A" and the user must run `code-graph flows --rank` explicitly.

**Additional files affected by stats integration:**
- `crates/storage/src/graph_store.rs` — update `stats()` to populate new fields
- `crates/domain/src/test_support.rs` — update mock `stats()` implementation
- `crates/cli/src/output.rs` — render new fields in Displayable for GraphStats
- `crates/cli/src/commands/stats.rs` — instantiate FlowUseCase for on-demand computation

## CLI

### New command: `code-graph flows`

```
code-graph flows                          # top flows (default limit 20)
code-graph flows --symbol UserService     # flows through a specific symbol
code-graph flows --rank                   # criticality ranking
code-graph flows --rank --limit 50        # top-50 by criticality
code-graph flows --depth 10              # override max depth
code-graph flows --json / --table        # output format
```

### Compact output

Default (top flows):
```
Entry points: 12 detected (3 main, 2 http, 7 public-root)
Flows: 847 total, showing top 20

[1] main → Config.load → Database.connect → Pool.init (depth 4)
[2] handle_request → AuthService.validate → TokenStore.verify → Database.query (depth 4)
```

--symbol (filter):
```
Flows through AuthService.validate: 23

[1] handle_request → AuthService.validate → TokenStore.verify → Database.query
[2] ws_connect → AuthService.validate → TokenStore.verify → Cache.get
```

--rank (criticality):
```
# Symbol                          Betweenness  Flows  Entry?
1 Database.query                   0.847        312    no
2 AuthService.validate             0.721        198    no
```

### Stats integration
```
Files: 234 | Symbols: 1,892 | Edges: 5,431
Entry points: 12 | Avg criticality: 0.034
```

## File Changes

| File | Change |
|------|--------|
| `crates/domain/src/model.rs` | Add EntryPoint, EntryPointKind, ExecutionFlow, CriticalityScore, FlowAnalysis, FlowStats, FlowConfig types |
| `crates/domain/src/model.rs` | Add entry_point_count + avg_criticality to GraphStats |
| `crates/domain/src/analysis/mod.rs` | Add `pub mod flow;` |
| `crates/domain/src/analysis/flow.rs` | NEW — entry point detection, betweenness centrality, flow enumeration |
| `crates/domain/src/use_cases/mod.rs` | Add `pub mod flow;` |
| `crates/domain/src/use_cases/flow.rs` | NEW — FlowUseCase with analyze/flows_through/criticality |
| `crates/domain/src/lib.rs` | Re-export new types |
| `crates/cli/src/commands/mod.rs` | Add Flows command + FlowsArgs |
| `crates/cli/src/commands/flows.rs` | NEW — run_flows() CLI handler |
| `crates/cli/src/commands/stats.rs` | Instantiate FlowUseCase for on-demand entry point count + avg criticality |
| `crates/cli/src/output.rs` | Render new GraphStats fields |
| `crates/cli/src/lib.rs` | Wire Flows command |
| `crates/storage/src/graph_store.rs` | Update stats() to populate new GraphStats fields |
| `crates/domain/src/test_support.rs` | Update mock stats() implementation |
| `.code-graph/config.toml` spec | Document [flows] section for entry point overrides |

## Acceptance Criteria

1. `code-graph flows` lists detected execution flows from auto-detected entry points
2. `code-graph flows --symbol <name>` filters flows passing through a specific symbol — every returned flow contains that symbol in its path
3. `code-graph flows --rank` outputs symbols ranked by descending betweenness centrality
4. Entry points auto-detected for all five EntryPointKind variants: Main (`main()`, `#[tokio::main]`, `if __name__`), Test (`#[test]`, `test_` prefix), HttpHandler (`@Get/@Post`, axum/flask route handlers), CliCommand (clap/argparse handlers), PublicRoot (exported public symbols with zero incoming Calls edges). A test fixture covering each variant must produce the expected classification.
5. Entry point overrides configurable via `.code-graph/config.toml` (`[flows] extra_entry_points`, `excluded_entry_points`)
6. Criticality scores computed via Brandes' betweenness centrality (all-pairs, not entry-point-only), normalized by `(n-1)(n-2)` (directed graph factor) so all values fall in [0.0, 1.0]. On a graph with >= 3 reachable nodes, the highest-centrality node has a score > 0.0.
7. Both flow enumeration and betweenness centrality traverse only High-confidence behavioral edges (`Calls`, `Extends`, `Implements`, `Embeds`). Medium-confidence edges (`ImportsFrom`, `TypeReference`, etc.) are excluded to avoid phantom flows.
8. Flow enumeration bounded by three limits: depth per flow (default 20), global flow count (default 1000), per-entry-point visit budget (default 100,000 DFS steps). Truncated entry points are marked in output.
9. Cycle detection prevents infinite loops (per-path visited set) — no node appears twice in a single flow's path
10. `code-graph stats` computes and displays entry point count and average criticality on-demand (no prior `flows` invocation required). If the graph has zero symbols, both display as 0.
11. All three output formats produce valid output: compact (default, as shown in CLI examples), `--table` (columnar with #, Symbol, Betweenness, Flows, Entry? columns for --rank; entry/path/depth columns for flows), `--json` (serialized FlowAnalysis struct, parseable by jq)
12. Running `code-graph flows` on the project's own codebase exits with code 0, detects >= 1 entry point, and produces >= 1 flow. `code-graph flows --rank` produces a non-empty ranked list.

## Non-Goals

- Dynamic/runtime flow tracing (static analysis only)
- Cross-repository flow detection
- Flow visualization / UI (v0.3)
- Persistence of flow results in SQLite (computed on-demand)
- Type inference for improved call resolution (separate concern)
