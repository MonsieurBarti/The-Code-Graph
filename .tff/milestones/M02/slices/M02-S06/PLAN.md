# M02-S06: Clone Detection — Implementation Plan

> For agentic workers: execute task-by-task with TDD.

**Goal:** Detect structurally similar code (Type 1-2) via graph fingerprints + token Jaccard similarity, group into clusters, expose via `code-graph clones` CLI and duplication metrics in `code-graph stats`.

**Architecture:** Two-phase pipeline — Phase 1 fingerprints symbols into buckets (quantize-and-concatenate), Phase 2 refines candidates via token Jaccard similarity. Connected components for clustering. Hexagonal architecture: analysis → use case → CLI.

**Tech Stack:** Rust, clap (CLI), serde (serialization), SQLite (storage via existing GraphStore)

## File Structure

### New Files
| File | Responsibility |
|------|----------------|
| `crates/domain/src/analysis/clones.rs` | Pure functions: fingerprinting, bucketing, tokenization, Jaccard similarity, clustering |
| `crates/domain/src/use_cases/clones.rs` | `CloneUseCase<S, F>` orchestrating GraphStore + FileSystem + analysis algorithms |
| `crates/cli/src/commands/clones.rs` | CLI handler for `code-graph clones` command |

### Modified Files
| File | Change |
|------|--------|
| `crates/domain/src/model.rs` | Add 7 new types: `CloneType`, `StructuralFingerprint`, `BucketKey`, `CloneMatch`, `CloneCluster`, `CloneAnalysis`, `CloneConfig` + extend `GraphStats` with 3 optional clone fields |
| `crates/domain/src/analysis/mod.rs` | Add `pub mod clones;` |
| `crates/domain/src/use_cases/mod.rs` | Add `pub mod clones;` |
| `crates/cli/src/commands/mod.rs` | Add `pub mod clones;` + `Clones(ClonesArgs)` variant + `ClonesArgs` struct |
| `crates/cli/src/lib.rs` | Add dispatch arm `Commands::Clones(args) => commands::clones::run_clones(args, output_format)` |
| `crates/cli/src/output.rs` | `impl Displayable for CloneAnalysis` and `impl Displayable for Vec<CloneCluster>` |
| `crates/cli/src/commands/stats.rs` | Add on-demand clone metrics to stats output |
| `crates/domain/src/test_support.rs` | Update `InMemoryGraphStore::stats()` for new optional GraphStats fields |
| `crates/storage/src/graph_store.rs` | Update `SqliteStore::stats()` for new optional GraphStats fields |

---

## Wave 0 (sequential — T01 then T02; T02 depends on T01's type definitions)

### T01: Add clone detection model types to model.rs
**Files:** Modify `crates/domain/src/model.rs`, `crates/domain/src/test_support.rs`, `crates/storage/src/graph_store.rs`
**Traces to:** AC5, AC8, AC9, AC10

Append after the `FlowConfig` default impl (after line 356):

```rust
// ---------------------------------------------------------------------------
// Clone detection types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CloneType {
    /// Identical bucket key AND token Jaccard >= 0.95 on un-normalized tokens
    Type1,
    /// Token Jaccard >= threshold on normalized tokens, < 0.95 un-normalized
    Type2,
    /// Structural match only (cross-language or missing source)
    StructuralOnly,
}

#[derive(Debug, Clone)]
pub struct StructuralFingerprint {
    pub qualified_name: String,
    pub symbol_kind: SymbolKind,
    pub callee_count: usize,
    pub caller_count: usize,
    pub edge_kind_set: u32,
    pub body_line_count: usize,
    pub child_count: usize,
    pub language: Language,
    pub file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BucketKey {
    pub kind: SymbolKind,
    pub callee_bin: u8,
    pub caller_bin: u8,
    pub line_bin: u8,
    pub child_bin: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneMatch {
    pub source: String,
    pub target: String,
    pub similarity: f64,
    pub clone_type: CloneType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneCluster {
    pub id: usize,
    pub members: Vec<String>,
    pub avg_similarity: f64,
    pub clone_type: CloneType,
    pub representative: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneAnalysis {
    pub clusters: Vec<CloneCluster>,
    pub total_symbols_analyzed: usize,
    pub symbols_in_clones: usize,
    pub duplication_pct: f64,
    pub most_duplicated: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CloneConfig {
    pub threshold: f64,
    pub min_lines: usize,
    pub max_candidates_per_bucket: usize,
}

impl Default for CloneConfig {
    fn default() -> Self {
        Self {
            threshold: 0.7,
            min_lines: 5,
            max_candidates_per_bucket: 500,
        }
    }
}
```

Extend `GraphStats` with 3 new optional fields (after `avg_criticality`):
```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clone_clusters: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplication_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub most_duplicated: Option<String>,
```

**Important:** Also update ALL existing `GraphStats` construction sites in the same task to prevent compilation failures:
- `crates/domain/src/test_support.rs` `InMemoryGraphStore::stats()` — add `clone_clusters: None, duplication_pct: None, most_duplicated: None`
- `crates/storage/src/graph_store.rs` `SqliteStore::stats()` — add `clone_clusters: None, duplication_pct: None, most_duplicated: None`
- Any `GraphStats` literals in tests within `model.rs`

**Language derivation for `StructuralFingerprint`:** The `language` field is computed via `Language::from_path(&sym.location.file)` in `compute_fingerprints()`. `SymbolNode` has no `language` field, but `Location.file` contains the file path from which language can be derived. No file-map join needed.

- **Run**: `cargo test -p domain --lib model && cargo build --workspace`
- **Expect**: PASS — existing model tests still pass, new types compile, all stores updated
- **Commit**: `feat(S06/T01): add clone detection model types`

### T02: Register modules and add stubs (depends on T01)
**Files:** Modify `crates/domain/src/analysis/mod.rs`, `crates/domain/src/use_cases/mod.rs`, `crates/cli/src/commands/mod.rs`, `crates/cli/src/lib.rs`
**Files:** Create `crates/domain/src/analysis/clones.rs`, `crates/domain/src/use_cases/clones.rs`, `crates/cli/src/commands/clones.rs`
**Traces to:** (plumbing — enables AC1-AC4)

`crates/domain/src/analysis/mod.rs` — add:
```rust
pub mod clones;
```

`crates/domain/src/use_cases/mod.rs` — add:
```rust
pub mod clones;
```

`crates/cli/src/commands/mod.rs` — add module declaration:
```rust
pub mod clones;
```

Add `Clones(ClonesArgs)` to `Commands` enum:
```rust
    /// Detect code clones across the codebase
    Clones(ClonesArgs),
```

Add `ClonesArgs` struct:
```rust
#[derive(clap::Args)]
pub struct ClonesArgs {
    /// Similarity threshold (0.0-1.0)
    #[arg(long, default_value = "0.7")]
    pub threshold: f64,
    /// Minimum symbol body lines
    #[arg(long, default_value = "5")]
    pub min_lines: usize,
    /// Show detailed members of a specific cluster
    #[arg(long)]
    pub cluster: Option<usize>,
}
```

`crates/cli/src/lib.rs` — add dispatch arm:
```rust
        Commands::Clones(args) => commands::clones::run_clones(args, output_format),
```

Create stub `crates/domain/src/analysis/clones.rs`:
```rust
// Clone detection analysis — implemented in T03-T06
```

Create stub `crates/domain/src/use_cases/clones.rs`:
```rust
// Clone use case — implemented in T07
```

Create stub `crates/cli/src/commands/clones.rs`:
```rust
use domain::error::Result;
use crate::output::OutputFormat;
use crate::commands::ClonesArgs;

pub fn run_clones(_args: &ClonesArgs, _output_format: OutputFormat) -> Result<()> {
    todo!("clone detection CLI — implemented in T08")
}
```

- **Run**: `cargo build --workspace`
- **Expect**: PASS — full workspace compiles with stubs
- **Commit**: `chore(S06/T02): register clone modules, add stubs, update stores`

---

## Wave 1a (parallel — core analysis primitives, depends on Wave 0)

### T03: Write tests + implement structural fingerprinting
**Files:** Modify `crates/domain/src/analysis/clones.rs`
**Traces to:** AC4, AC6, AC7

- [ ] Write failing tests for `compute_fingerprints`: verifies callee/caller/child counts, min_lines filtering, Contains edge counting
- [ ] Run `cargo test -p domain --lib analysis::clones`, verify FAIL
- [ ] Implement `compute_fingerprints(symbols, edges, config) -> Vec<StructuralFingerprint>` with O(E) adjacency map construction
- [ ] Run `cargo test -p domain --lib analysis::clones`, verify PASS
- [ ] Commit: `feat(S06/T03): structural fingerprinting with adjacency maps`

### T04: Write tests + implement bucketing
**Files:** Modify `crates/domain/src/analysis/clones.rs`
**Traces to:** AC6, AC7, AC10

- [ ] Write failing tests for `bucket_key` and `group_into_buckets`: same-bin grouping, different-kind separation, correct bucket counts
- [ ] Run `cargo test -p domain --lib analysis::clones`, verify FAIL
- [ ] Implement `count_bin`, `line_bin`, `child_bin`, `bucket_key`, `group_into_buckets` with quantize-and-concatenate strategy
- [ ] Run `cargo test -p domain --lib analysis::clones`, verify PASS
- [ ] Commit: `feat(S06/T04): quantize-and-concatenate bucketing`

### T05: Write tests + implement tokenization and Jaccard similarity
**Files:** Modify `crates/domain/src/analysis/clones.rs`
**Traces to:** AC8, AC9

- [ ] Write failing tests for `tokenize`, `normalize_identifiers`, `jaccard_similarity`: identical/disjoint tokens, positional placeholder normalization, Type 2 detection after normalization, comment stripping
- [ ] Run `cargo test -p domain --lib analysis::clones`, verify FAIL
- [ ] Implement `tokenize` (split + strip comments), `normalize_identifiers` (positional `_N` placeholders preserving binding), `jaccard_similarity` (multiset intersection/union)
- [ ] Run `cargo test -p domain --lib analysis::clones`, verify PASS
- [ ] Commit: `feat(S06/T05): tokenization, normalization, and Jaccard similarity`

---

## Wave 1b (depends on T03, T04, T05 — comparison uses tokenization + bucketing)

### T06: Write tests + implement candidate comparison and clustering
**Files:** Modify `crates/domain/src/analysis/clones.rs`
**Traces to:** AC7, AC8, AC9, AC10

- [ ] Write failing tests for `compare_pair` (Type1/Type2/StructuralOnly classification) and `cluster_matches` (transitive clustering, separate components, empty input)
- [ ] Run `cargo test -p domain --lib analysis::clones`, verify FAIL
- [ ] Implement `compare_pair` (un-normalized ≥0.95 → Type1, normalized ≥threshold → Type2, cross-language → StructuralOnly) and `cluster_matches` (Union-Find connected components, sorted by size descending, 1-indexed IDs)
- [ ] Run `cargo test -p domain --lib analysis::clones`, verify PASS
- [ ] Commit: `feat(S06/T06): candidate comparison and connected-component clustering`

---

## Wave 2 (sequential — use case + CLI, depends on Wave 1b)

### T07: Write tests + implement CloneUseCase
**Files:** Modify `crates/domain/src/use_cases/clones.rs`
**Traces to:** AC1, AC2, AC3, AC4, AC5, AC7

- [ ] Write failing tests: `analyze_detects_type2_clones` (using InMemoryGraphStore + MockFileSystem with two renamed-variable functions), `analyze_filters_by_min_lines`, `analyze_empty_graph`
- [ ] Run `cargo test -p domain --lib use_cases::clones`, verify FAIL
- [ ] Implement `CloneUseCase<S: GraphStore, F: FileSystem>` with `root: PathBuf` field (for resolving `root.join(symbol.location.file)` to absolute paths for `FileSystem::read_file`) and `analyze(config) -> Result<CloneAnalysis>` orchestrating fingerprint → bucket → compare → cluster pipeline with file content cache
- [ ] Run `cargo test -p domain --lib use_cases::clones`, verify PASS
- [ ] Commit: `feat(S06/T07): CloneUseCase with two-phase pipeline`

### T08: Implement CLI command and output formatting
**Files:** Modify `crates/cli/src/commands/clones.rs`, `crates/cli/src/output.rs`
**Traces to:** AC1, AC2, AC3, AC4

- [ ] Implement `run_clones(args, output_format)`: open store + `RealFileSystem`, create `CloneUseCase::new(store, fs, root)`, handle `--cluster <id>` drill-down
- [ ] Add `impl Displayable for CloneAnalysis` (compact: summary + cluster list; table: header rows; json: serde)
- [ ] Add `impl Displayable for Vec<CloneCluster>` (compact: indented members; table: per-cluster tables; json: serde)
- [ ] Run `cargo build --workspace && cargo test -p cli`, verify PASS
- [ ] Commit: `feat(S06/T08): CLI command and output formatting for clone detection`

### T09: Add clone metrics to stats command
**Files:** Modify `crates/cli/src/commands/stats.rs`
**Traces to:** AC5

- [ ] Add `CloneUseCase` integration to `run_stats()`: instantiate with `RealFileSystem`, run analysis if ≤10k symbols, populate `stats.clone_clusters`, `stats.duplication_pct`, `stats.most_duplicated`
- [ ] Run `cargo build --workspace && cargo test -p cli`, verify PASS
- [ ] Commit: `feat(S06/T09): clone metrics in stats output`

---

## Wave 3 (depends on Wave 2 — CLI parse validation)

### T10: Add CLI parse tests for clones command
**Files:** Modify `crates/cli/src/commands/mod.rs`
**Traces to:** AC1, AC2, AC3, AC4

- [ ] Add `parse_clones_command` test and extend `all_subcommands_parse` with: `["code-graph", "clones"]`, `["code-graph", "clones", "--threshold", "0.8"]`, `["code-graph", "clones", "--min-lines", "10"]`, `["code-graph", "clones", "--cluster", "1"]`
- [ ] Run `cargo test -p cli --lib commands::tests`, verify PASS
- [ ] Commit: `test(S06/T10): CLI parse tests for clones command`
