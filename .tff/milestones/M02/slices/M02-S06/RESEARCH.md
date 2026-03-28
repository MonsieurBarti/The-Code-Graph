# Research — M02-S06: Clone Detection

## 1. FileSystem Port Integration

**Trait:** `crates/domain/src/ports.rs:67` — `FileSystem: Send + Sync`
- `read_file(&self, path: &Path) -> Result<String>` — reads entire file as string
- `list_files(root, extensions)` and `file_hash(path)` also available but not needed

**Pattern for multi-port use case:** `IndexUseCase<S, P, F, G>` takes 4 generics. CloneUseCase needs only 2: `CloneUseCase<S: GraphStore, F: FileSystem>`.

**CLI instantiation:** `let fs = RealFileSystem;` (unit struct, no constructor args). Located in `crates/cli/src/adapters/fs.rs`.

**Test mock:** `MockFileSystem` in `crates/domain/src/test_support.rs:218-278` — in-memory file storage. Use this for Phase 2 token refinement tests.

**Decision:** CloneUseCase takes `<S: GraphStore, F: FileSystem>`. Phase 2 reads source via `self.fs.read_file(&symbol.location.file)`.

## 2. Model Types for Fingerprinting

**SymbolNode** (`model.rs:138-150`):
- `kind: SymbolKind` — 14 variants (Function, Class, Method, Struct, etc.)
- `location: Location` — has `line_start`, `line_end` for body_line_count computation
- `signature: Option<String>` — **None for Python/Go/TS/JS parsers**. Confirmed: only Rust populates this. Spec correctly excludes `param_count`.
- `qualified_name: String` — unique identifier, used as edge source/target

**Location** (`model.rs:122-129`): `file: PathBuf`, `line_start: usize`, `line_end: usize`, `col_start`, `col_end`. Body line count = `line_end - line_start + 1`.

**EdgeKind** (`model.rs:70-88`): 16 variants. High-confidence: `Calls`, `Extends`, `Implements`, `Embeds`. Confidence method at lines 90-110.

**GraphStats** (`model.rs:241-250`): Currently has `files`, `symbols`, `edges`, `entry_point_count: Option<usize>`, `avg_criticality: Option<f64>`. All optional fields use `#[serde(skip_serializing_if = "Option::is_none")]`. Clone fields follow same pattern.

## 3. Edge Query Strategy

**No single method for all edges of a symbol.** Must call both:
- `get_edges_from(qname)` → outgoing edges (symbol is source)
- `get_edges_to(qname)` → incoming edges (symbol is target)

**For fingerprinting all symbols at once:** Use `all_edges()` and build a HashMap keyed by symbol qualified_name. This is O(E) once, then O(1) per symbol lookup. Much more efficient than N × 2 individual queries.

```
let edges = store.all_edges()?;
let mut outgoing: HashMap<&str, Vec<&Edge>> = HashMap::new();
let mut incoming: HashMap<&str, Vec<&Edge>> = HashMap::new();
for edge in &edges {
    outgoing.entry(&edge.source).or_default().push(edge);
    incoming.entry(&edge.target).or_default().push(edge);
}
```

**Decision:** Use bulk `all_symbols()` + `all_edges()` in CloneUseCase::analyze(), build adjacency maps once, pass to fingerprinting function. Same pattern as FlowUseCase::analyze().

## 4. Implementation Patterns (from flow.rs reference)

**Analysis module** (`analysis/flow.rs`):
- Pure functions: `pub fn detect_entry_points(symbols: &[SymbolNode], edges: &[Edge], config: &FlowConfig) -> Vec<EntryPoint>`
- Private helpers: `fn classify_symbol(...)`, `fn is_high_confidence(...)`
- Takes slices/refs, returns owned values. No I/O.

**UseCase** (`use_cases/flow.rs`):
- `pub struct FlowUseCase<S> { store: S }`
- Constructor: `pub fn new(store: S) -> Self`
- Methods query store, call analysis functions, aggregate results
- `pub fn analyze(&self, config: &FlowConfig) -> Result<FlowAnalysis>`

**CLI command** (`commands/flows.rs`):
- `pub fn run_flows(args: &FlowsArgs, output_format: OutputFormat) -> Result<()>`
- Pattern: `open_graph()` → optional `load_config()` → `UseCase::new(store)` → call method → `print(&result, format)`

**Displayable trait** (`output.rs:35-39`):
- `fn fmt_compact(&self, w: &mut dyn Write)`, `fn fmt_table(...)`, `fn fmt_json(...)`
- JSON: `serde_json::to_string_pretty(self)` — types must derive `Serialize`
- `print<T: Displayable>(value: &T, format: OutputFormat)` dispatcher at line 41

**FlowsArgs in mod.rs:** Command args defined as clap struct in `commands/mod.rs`, dispatched in main command enum.

## 5. Performance Considerations

**Symbol counts in practice:** Typical repos: 1k-10k symbols. Large monorepos: 50k+.

**Bucketing reduces pairwise comparisons:**
- With 14 SymbolKinds × 4 callee bins × 4 caller bins × 4 line bins × 3 child bins = 2,688 possible buckets
- For 10k symbols: average ~3.7 symbols per bucket → pairwise within bucket is trivial
- For 50k symbols: average ~18.6 per bucket → still manageable (~171 pairs per bucket)
- Worst case: many symbols with identical fingerprints (e.g., simple getters) → capped at max_candidates_per_bucket

**Phase 2 (token reading) is the bottleneck:**
- File I/O for each candidate pair
- Mitigation: cache file contents by path (many symbols share the same file)
- Tokenization is CPU-bound but fast for individual functions

**Decision:** Add a file content cache (`HashMap<PathBuf, String>`) in the analysis function to avoid re-reading the same file for multiple symbols.

## 6. Key Integration Points Summary

| Integration Point | Location | What's Needed |
|---|---|---|
| New model types | `model.rs` | `StructuralFingerprint`, `CloneMatch`, `CloneCluster`, `CloneAnalysis`, `CloneConfig` + extend `GraphStats` |
| Analysis module | `analysis/clones.rs` (new) | Pure functions: fingerprint, bucket, tokenize, jaccard, cluster |
| Use case | `use_cases/clones.rs` (new) | `CloneUseCase<S, F>` with `analyze()` method |
| CLI command | `commands/clones.rs` (new) | `run_clones()` + `ClonesArgs` struct |
| Command registration | `commands/mod.rs` | Add `Clones(ClonesArgs)` variant to command enum |
| Output formatting | `output.rs` | `impl Displayable for CloneAnalysis` and `Vec<CloneCluster>` |
| Analysis module registration | `analysis/mod.rs` | Add `pub mod clones;` |
| Use case module registration | `use_cases/mod.rs` | Add `pub mod clones;` |
| Lib re-exports | `domain/src/lib.rs` | Re-export new clone types |
| Stats command | `commands/stats.rs` | Instantiate CloneUseCase for optional clone metrics |
| Test support | `test_support.rs` | Update `InMemoryGraphStore::stats()` for new optional fields |
