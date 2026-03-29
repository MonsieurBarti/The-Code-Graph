# Research — M02-S05: Dead Code Detection

## Codebase Integration Points

### Analysis Layer (`crates/domain/src/analysis/`)

**Pattern:** Pure functions taking `(symbols: &[SymbolNode], edges: &[Edge], config: &Config) -> Result`. No side effects, no store access.

**Existing modules:** `risk.rs`, `community.rs`, `clones.rs`, `flow.rs`, `impact.rs`, `blast_radius.rs`, `change_detection.rs`, `search.rs`

**Registration:** Add `pub mod dead_code;` to `analysis/mod.rs` (8 modules currently).

**Key function signature to follow:**
```rust
pub fn detect_dead_code(
    symbols: &[SymbolNode],
    edges: &[Edge],
    config: &DeadCodeConfig,
) -> DeadCodeAnalysis
```

### Use Case Layer (`crates/domain/src/use_cases/`)

**Pattern:** `struct FooUseCase<S> { store: S }` with `impl<S: GraphStore> FooUseCase<S>`. Constructor: `new(store: S) -> Self`. Methods take `&self` + config, return `Result<T>`.

**Data loading:** Most use cases call `self.store.all_symbols()` + `self.store.all_edges()` then delegate to analysis functions. This is the right pattern for dead code (bulk query, compute, return).

**Registration:** Add `pub mod dead_code;` to `use_cases/mod.rs` (8 modules currently).

### CLI Command Layer (`crates/cli/src/commands/`)

**Pattern:** `DeadCodeArgs` struct with `#[derive(clap::Args)]`, `Commands::DeadCode(DeadCodeArgs)` variant, dispatch in `lib.rs` to `commands::dead_code::run_dead_code(args, output_format)`.

**Global flags available:** `--json`, `--table`, `-v` (from `Cli` struct). Command-specific flags defined in the args struct.

**Registration requires changes to:**
1. `commands/mod.rs` — add `pub mod dead_code;`, `DeadCodeArgs` struct, `Commands::DeadCode` variant, parser test entries
2. `lib.rs` — add match arm

### Output (`crates/cli/src/output.rs`)

**Pattern:** `Displayable` trait with `fmt_compact`, `fmt_table`, `fmt_json` methods. JSON uses `serde_json::to_string_pretty`. Called via `print(&result, output_format)`.

### Config (`crates/cli/src/config.rs`)

**Pattern:** `#[derive(Debug, Clone, Default, Deserialize)]` struct with all `Option<T>` fields. Added as `Option<DeadCodeCliConfig>` field on `CodeGraphConfig`. Loaded from `.code-graph/config.toml` via `toml::from_str`.

**Merge semantics:** Defaults -> config file -> CLI flags (highest priority). Uses `if let Some()` pattern.

**Section name:** `[dead-code]` in config.toml (TOML supports hyphens in section names).

## Dependencies

### Existing (reusable as-is)

| Dependency | Location | Usage |
|---|---|---|
| `SymbolNode` | `model.rs:154-165` | Fields: `qualified_name`, `kind`, `location`, `visibility`, `is_exported`, `is_test`, `decorators` |
| `Edge` | `model.rs:191-197` | Fields: `kind`, `source`, `target` |
| `EdgeKind` | `model.rs:86-103` | 16 variants. `.confidence()` method maps to Structural/Low/Medium/High |
| `SymbolKind` | `model.rs:42-57` | 14 variants (Function through Test) |
| `Visibility` | `model.rs:68-73` | Public, Private, Crate |
| `Location` | `model.rs:137-144` | `file: PathBuf`, `line_start`, `line_end` |
| `detect_entry_points()` | `analysis/flow.rs` | Takes `(&[SymbolNode], &[Edge], &FlowConfig) -> Vec<EntryPoint>` |
| `EntryPoint` | `model.rs:379-383` | `qualified_name`, `kind: EntryPointKind`, `confidence` |
| `EntryPointKind` | `model.rs:386-392` | Main, Test, HttpHandler, CliCommand, PublicRoot |
| `FlowConfig` | `model.rs:426-447` | `max_public_roots: 50` (default), `extra_entry_points`, `excluded_entry_points` |
| `GraphStore` trait | `ports.rs:6-50` | `all_symbols()`, `all_edges()` |
| `InMemoryGraphStore` | `test_support.rs` | For unit tests |

### New dependency needed

| Crate | Purpose | Where |
|---|---|---|
| `glob` (or `globset`) | Match `exclude_patterns`, `entry_point_patterns`, `migration_patterns` against qualified names and file paths | `crates/domain/Cargo.toml` |

**Decision:** Use `globset` (from the BurntSushi/ripgrep ecosystem) over `glob` — it compiles multiple patterns into a single automaton for O(n) matching instead of O(n*p). The `glob` crate requires per-pattern matching. Since we iterate all symbols and check against potentially multiple patterns, `globset` is the better fit.

**Alternative:** Implement simple wildcard matching inline (only `*` and `**` needed). This avoids a new dep but limits pattern expressiveness. Given the spec explicitly says "glob patterns," a proper glob crate is warranted.

## Spec-to-Codebase Alignment

### Confirmed correct

- **USAGE_EDGES set:** All 12 EdgeKind variants listed in spec exist in `model.rs`. Structural edges (Contains, ChildOf, HasDecorator, TestedBy) correctly excluded — confirmed via `EdgeKind::confidence()` returning `Confidence::Structural`.
- **`detect_entry_points()` returns Test entries:** Confirmed — flow.rs classifies `is_test`, `kind == Test`, and `test_` prefix all as `EntryPointKind::Test`.
- **`is_exported` field on SymbolNode:** Exists at `model.rs:160`.
- **`is_test` field on SymbolNode:** Exists at `model.rs:162`.
- **`FlowConfig::default()` sets `max_public_roots: 50`:** Confirmed.
- **Hexagonal architecture:** domain has no CLI/infra deps. Analysis is pure functions, use cases orchestrate via ports.

### Spec naming vs codebase naming

| Spec says | Codebase uses | Note |
|---|---|---|
| `DeadSymbol` | N/A (new type) | Define in `model.rs` with Serialize/Deserialize |
| `DeadCodeSummary` | N/A (new type) | Define in `model.rs` |
| `DeadCodeAnalysis` | N/A (new type) | Define in `model.rs` — follows `RiskAnalysis`, `CommunityAnalysis` pattern |
| `DeadCodeConfig` | N/A (new type) | Define in `model.rs` for domain config + `config.rs` for CLI config |
| `ExclusionReason` | N/A (new type) | Define in `model.rs` |
| `file_path: String` | `location.file: PathBuf` | Use `location.file.to_string_lossy().to_string()` |
| `line: usize` | `location.line_start: usize` | Direct mapping |

### Config type split

Per existing pattern, two config structs are needed:
1. **Domain:** `DeadCodeConfig` in `model.rs` — fully resolved, no Options
2. **CLI:** `DeadCodeCliConfig` in `config.rs` — all `Option<T>` fields for TOML deserialization

Merge happens in the CLI command handler (default -> config file -> CLI flags).

## Architecture Review

| Aspect | Status | Finding |
|---|---|---|
| Layer dependency | pass | Analysis is pure (no store access). Use case depends on GraphStore port. CLI depends on domain. |
| Module boundaries | pass | Single responsibility: analysis/dead_code.rs computes, use_cases/dead_code.rs orchestrates, commands/dead_code.rs presents |
| Port coverage | pass | GraphStore provides all_symbols() + all_edges() — sufficient for bulk query pattern |
| Cross-cutting concerns | pass | Reuses detect_entry_points() via direct call (same crate). No cross-module coupling introduced. |

## Risk Assessment

### Low risk
- **Algorithm complexity:** O(E + S) — trivial. HashSet build + linear scan.
- **Type integration:** All needed types exist. New types follow established Serialize/Deserialize pattern.
- **CLI wiring:** Mechanical — follows exact pattern of 16 existing commands.

### Medium risk
- **Glob dependency:** New crate (`globset`) must be added to domain. This is the only new external dependency. Needs version pinning.
- **FlowConfig interaction:** `detect_entry_points()` takes `&FlowConfig` which has its own `extra_entry_points` and `excluded_entry_points`. The dead code config also has `entry_point_patterns`. Need to clearly separate: FlowConfig defaults for entry-point detection, dead code's `entry_point_patterns` for additional exclusions.

### Mitigations
- **Glob:** `globset` is well-maintained (BurntSushi), no transitive dep conflicts expected.
- **FlowConfig:** Call `detect_entry_points()` with `FlowConfig::default()` (as spec says), then apply dead code's `entry_point_patterns` as a second pass on the resulting entry-point set.

## Open Questions (Resolved)

1. **Where to define domain types?** In `model.rs`, following `RiskAnalysis`, `CommunityAnalysis` placement.
2. **Config section name?** `[dead-code]` — TOML supports hyphens, matches CLI command name `dead-code`.
3. **Glob crate choice?** `globset` for multi-pattern efficiency.
4. **Test function handling in entry points?** Filter OUT `EntryPointKind::Test` from the entry-point exclusion set so layer 3 (`--include-tests`) controls test handling independently. This is explicitly specified in the spec.
