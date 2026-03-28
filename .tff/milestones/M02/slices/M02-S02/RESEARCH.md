# Research — M02-S02: Risk Scoring

## Integration Points

### 1. Betweenness Centrality Reuse

`brandes_betweenness()` in `crates/domain/src/analysis/flow.rs:215` is a public pure function:
```rust
pub fn brandes_betweenness(nodes: &HashSet<String>, edges: &[Edge]) -> HashMap<String, f64>
```
- Internally filters to high-confidence edges only (Calls, Extends, Implements, Embeds)
- Returns normalized [0.0, 1.0] scores
- Can be called directly from `analysis/risk.rs` — no wrapper needed
- Same loading pattern as FlowUseCase: `all_symbols()` → `HashSet<String>` conversion, `all_edges()`

### 2. Structural Edge Filtering

EdgeKind confidence levels (`model.rs:106-125`):
- **Structural**: `Contains`, `ChildOf`, `HasDecorator`, `TestedBy`
- Filter via: `edge.kind.confidence() != Confidence::Structural`
- BUT: `TestedBy` is structural AND needed for test gap — query it separately via `get_edges_to()`

For coupling, filter pattern:
```rust
edges.iter().filter(|e| e.kind.confidence() != Confidence::Structural)
```
Then additionally filter to edges where both endpoints are in the symbol set.

### 3. SymbolNode Fields

`model.rs:154-165`:
- `qualified_name: String` — primary key, format `"file.rs::SymbolName"`
- `decorators: Vec<String>` — populated by parser, used for security sensitivity
- `location: Location` where `location.file: PathBuf` — used for file-level aggregation
- `visibility: Visibility`, `is_exported: bool` — not needed for risk scoring

### 4. GraphStats Extension

`model.rs:257-271` — existing optional field pattern:
```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub avg_criticality: Option<f64>,
```
Add `avg_risk: Option<f64>` and `p90_risk: Option<f64>` following this pattern.

### 5. CLI Config

`config.rs` — `CodeGraphConfig` with optional sections:
```rust
pub struct CodeGraphConfig {
    pub flows: Option<FlowsConfig>,
    // Add: pub risk: Option<RiskCliConfig>,
}
```
Config loaded via `load_config(&root)?` in command handlers. Uses `toml::from_str`.

### 6. CLI Command Wiring

`commands/mod.rs` — Commands enum uses `#[derive(Subcommand)]`. Add:
```rust
Risk(RiskArgs),
```

Handler pattern from `flows.rs`:
1. `open_graph()` → `(SqliteStore, PathBuf)`
2. `load_config(&root)?`
3. Create use case, call method
4. Format output via `OutputFormat` enum + `Displayable` trait

### 7. Stats Integration

`stats.rs:16-47` — pattern: create use case with cloned store, call `analyze()` with default config, assign to optional stats fields. Guards with `if stats.symbols <= N` for expensive computations.

### 8. Test Support

- `test_support.rs:8-198` — `InMemoryGraphStore` with `insert_symbol()`, `insert_edge()`
- Flow tests in `analysis/flow.rs:402-865` — `make_symbol()`, `make_edge()` helpers
- Use case tests in `use_cases/flow.rs:126-285` — integration tests with InMemoryGraphStore

### 9. TestedBy Edge Reliability

- Created by parser for test → code relationships
- Used in `find.rs` to show test coverage for symbols
- Stored in SQLite via `mapping.rs:88` ("tested_by" string)
- Reliability depends on parser — may be incomplete for indirect test relationships

## Key Implementation Decisions

1. **Word-boundary matching** for security patterns: split `qualified_name` on `_`, `.`, `::`, and camelCase boundaries; match patterns against resulting segments
2. **Weight normalization**: divide each weight by sum at config load time — no existing precedent in FlowConfig, this is new behavior
3. **Performance guard**: brandes is O(V*(V+E)) — add a symbol count guard in stats.rs integration (like clone analysis uses `<= 10_000`)
4. **No new ports**: all data accessible via existing GraphStore methods

## Risks

- **brandes recomputation**: Risk scoring re-runs betweenness, duplicating FlowUseCase work. Acceptable for v0.2; cache/share later.
- **TestedBy sparsity**: If parser doesn't create many TestedBy edges, test_gap factor will read 1.0 for most symbols, reducing its discriminative value.
