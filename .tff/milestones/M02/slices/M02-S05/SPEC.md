# Spec — M02-S05: Dead Code Detection

## Problem Statement

Codebases accumulate unused symbols over time — functions never called, types never referenced, constants never read. These increase cognitive load for developers and AI agents alike, waste indexing/analysis time, and obscure the real dependency structure.

**Who benefits:** Developers doing cleanup, AI agents that need a compact view of what matters, and teams tracking code health metrics.

**Constraint:** Must integrate with the existing hexagonal architecture (analysis module + use case + CLI command + Displayable output). Must reuse existing entry-point detection logic from `flow.rs` for consistency.

## Approach

**Bulk graph query** — load all symbols + edges via `all_symbols()` / `all_edges()`, build a `HashSet` of alive qualified names from usage-edges, then filter. Consistent with existing analysis pattern (risk, community, clones): bulk load → compute → structured result.

### Architecture

- **Analysis module:** `crates/domain/src/analysis/dead_code.rs` — pure function, no side effects
- **Use case:** `crates/domain/src/use_cases/dead_code.rs` — `DeadCodeUseCase<S: GraphStore>`, loads data from store
- **CLI command:** `crates/cli/src/commands/dead_code.rs` — `code-graph dead-code` with standard output formats
- **Config:** CLI flags + `[dead-code]` section in config.toml

### Exclusion Layers (applied in order; first match wins for `ExclusionReason`)

1. **Entry points** — Main, HttpHandler, CliCommand (from `detect_entry_points()`). **Note:** `detect_entry_points()` also returns `EntryPointKind::Test` entries — these are filtered OUT of the entry-point exclusion set so that layer 3 (`--include-tests`) controls test handling. The `PublicRoot` cap (`max_public_roots: 50`) is safe because layer 2 (`is_exported`) independently catches exported symbols.
2. **Exported symbols** — `is_exported = true`
3. **Test functions** — `is_test = true` (unless `--include-tests`)
4. **Migration files** — path matches `migrations/`, `migrate/`, `alembic/`, `diesel/migrations/`
5. **User-configured patterns** — glob on qualified name or file path

## Domain Model

### Config

```rust
pub struct DeadCodeConfig {
    pub exclude_patterns: Vec<String>,       // glob patterns on file path or qualified name
    pub entry_point_patterns: Vec<String>,    // additional entry-point name patterns
    pub include_tests: bool,                  // if true, test functions can be flagged as dead
    pub migration_patterns: Vec<String>,      // path patterns for migration files (defaults provided)
    pub kind_filter: Option<Vec<SymbolKind>>, // restrict to specific symbol kinds
}
```

### Result Types

```rust
pub struct DeadCodeAnalysis {
    pub dead_symbols: Vec<DeadSymbol>,
    pub summary: DeadCodeSummary,
}

pub struct DeadSymbol {
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub file_path: String,               // from Location.file.to_string_lossy()
    pub line: usize,                      // from Location.line_start
    pub visibility: Visibility,
}

pub struct DeadCodeSummary {
    pub total_symbols: usize,
    pub dead_count: usize,
    pub dead_percentage: f64,               // dead_count / total_symbols * 100
    pub excluded_count: usize,              // counted before kind_filter is applied
    pub dead_by_kind: HashMap<SymbolKind, usize>,
    pub dead_by_file: Vec<(String, usize)>,  // sorted by dead symbol count desc
}

pub enum ExclusionReason {
    EntryPoint,
    Exported,
    TestFunction,
    MigrationFile,
    UserPattern(String),
}
```

## Algorithm

`detect_dead_code(symbols, edges, config) -> DeadCodeAnalysis`

1. **Build alive set:** Iterate all edges, collect `target` qualified names where `edge.kind` is a usage edge. Result: `HashSet<String>`. A symbol is alive iff it appears as the target of at least one usage edge.

2. **Detect entry points:** Call `detect_entry_points(symbols, edges, &FlowConfig::default())`. Filter OUT `EntryPointKind::Test` entries — test exclusion is handled at layer 3 so `--include-tests` can override it. Remaining entry points become the exclusion set for layer 1.

3. **Build migration file set:** Collect file paths matching migration patterns (defaults + user-configured). Resolve `entry_point_patterns` from config: symbols whose `qualified_name` matches any pattern glob are added to the entry-point exclusion set.

4. **Classify each symbol:**
   - In alive set → alive (skip)
   - Entry point → excluded (EntryPoint)
   - `is_exported` → excluded (Exported)
   - `is_test` and `!config.include_tests` → excluded (TestFunction)
   - File path matches migration pattern → excluded (MigrationFile)
   - Matches user exclude pattern → excluded (UserPattern)
   - Apply kind_filter if set (display-layer filter — does not affect `excluded_count`)
   - Otherwise → **dead**

5. **Aggregate:** Build `DeadCodeSummary` with counts by kind and by file. `excluded_count` is computed before `kind_filter` is applied. If `total_symbols == 0`, `dead_percentage = 0.0` (div-by-zero guard).

**Usage edge set:**

```rust
// All edges that represent actual symbol usage. Excludes:
// - Structural edges (Contains, ChildOf, HasDecorator): containment ≠ usage
// - TestedBy: a test referencing a symbol does not constitute production usage;
//   tested-but-otherwise-uncalled symbols are still dead code candidates
const USAGE_EDGES: &[EdgeKind] = &[
    // High confidence
    Calls, Extends, Implements, Embeds,
    // Medium confidence
    ImportsFrom, ReExport, BarrelReExportAll, TypeReference, DotImport,
    // Low confidence (module-level usage — still counts as alive)
    DependsOn, ConditionalImport, SideEffectImport,
];
```

**Complexity:** O(E + S) — single pass over edges, single pass over symbols.

## CLI Interface

**Command:** `code-graph dead-code [OPTIONS]`

**Flags:**
- `--json` / `--table` — output format (default: compact)
- `--verbose (-v)` — show exclusion reasons
- `--exclude-pattern <GLOB>` — additional exclusion patterns (repeatable)
- `--include-tests` — include test functions as candidates
- `--kind <KIND>` — filter to specific symbol kinds (repeatable)
- `--limit <N>` — max results (display-layer only, not in `DeadCodeConfig`)

**Pattern merge semantics:** CLI `--exclude-pattern` flags are unioned with `config.toml` `exclude_patterns`. CLI flags do not override config — both apply.

**Compact output:**
```
Dead code: 23 symbols (of 450 total, 5.1%)

  src/old_module.rs:42    fn deprecated_helper      Function
  src/old_module.rs:87    struct LegacyConfig        Struct
  src/utils.rs:15         fn unused_format           Function
```

**Table output:** Columns: File | Line | Symbol | Kind | Visibility

**JSON output:** Full `DeadCodeAnalysis` serialized.

**config.toml:**
```toml
[dead-code]
exclude_patterns = ["**/generated/**", "**/proto/**"]
migration_patterns = ["**/migrations/**"]
entry_point_patterns = ["*_handler", "*_endpoint"]
```

## Acceptance Criteria

- **AC1:** `code-graph dead-code` returns symbols with zero incoming usage-edges, excluding entry points, exported symbols, test functions, and migration files.
- **AC2:** Entry point detection reuses `detect_entry_points()` from flow analysis for consistency.
- **AC3:** `--exclude-pattern <glob>` excludes symbols matching the pattern (by qualified name or file path).
- **AC4:** `--include-tests` flag causes test functions to be included as dead code candidates.
- **AC5:** `--kind <kind>` filters results to specific symbol kinds.
- **AC6:** Output supports compact (default), `--table`, and `--json` formats via `Displayable` trait.
- **AC7:** `[dead-code]` section in config.toml allows persistent exclude/migration/entry-point patterns.
- **AC8:** Summary statistics include: total symbols, dead count, percentage, breakdown by kind and by file.

## Non-Goals

- **Reachability analysis** — tracing from entry points through the full call graph (separate, more complex feature)
- **Cross-repository dead code** — only analyzes the indexed codebase
- **Auto-removal** — detection only, no code modification
- **Dynamic dispatch resolution** — trait objects, virtual calls (static analysis limitation)

## Known Limitations

- **Trait implementations via dynamic dispatch:** A method `impl Foo for Bar { fn execute() }` called only through `dyn Foo` has no static `Calls` edge to `Bar::execute`. It will be flagged as dead code. This is inherent to static analysis without whole-program type inference.
- **PublicRoot cap:** `detect_entry_points()` caps PublicRoot candidates at 50 (by outgoing edge count). Additional public symbols are still protected by the `is_exported` exclusion at layer 2.
