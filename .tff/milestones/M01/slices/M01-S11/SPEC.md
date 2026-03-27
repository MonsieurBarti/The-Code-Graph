# Spec — M01-S11: Polish, Benchmarks & Performance

## Problem

M01 slices S01–S09 deliver all core functionality but leave gaps: resolver auto-detection is absent (hardcoded settings), no benchmarks exist (R9 requires them), bulk memory loading in incremental/impact paths is wasteful, and one Unix-only shell command breaks cross-platform support.

## Goal

Close all known v0.1 gaps — resolver config reading, filtered storage queries, criterion benchmarks, and cross-platform fixes — so M01 ships complete against its requirements.

## Approach: Fix-Forward with Typed Configs

No shared trait — each resolver gets its own typed config struct loaded in `ResolverRegistry::new()`. Each fix is independent and isolated.

---

## Design

### 1. Resolver Configuration — Per-Language Typed Configs

**Location:** `crates/parser/src/resolver/`

**No shared `ResolverConfig` trait.** Each language has its own config struct with different shapes. `ResolverRegistry::new(project_root)` loads each config at construction and passes it to the respective resolver.

#### TypeScript — oxc_resolver built-in tsconfig support

`oxc_resolver` already supports `TsconfigDiscovery::Auto`, which reads `tsconfig.json` (including `paths`, `baseUrl`, and `extends` chains) natively. Current code ignores this.

**Change:** In `TypeScriptResolver::build_resolver()`, set:
```rust
ResolveOptions {
    tsconfig: Some(TsconfigOptions {
        config_file: TsconfigDiscovery::Auto,
        references: TsconfigReferences::Auto,
    }),
    // ... existing extensions, conditions, main_fields
    ..Default::default()
}
```

This is a ~5-line change. No manual tsconfig parsing needed. The `_project_root` field on `TypeScriptResolver` is already available for `oxc_resolver` to discover the tsconfig.

**Defaults when tsconfig.json is absent:** `oxc_resolver` falls back to its built-in defaults (same as current hardcoded values). No special handling needed.

#### Python — src/ directory heuristic

Instead of parsing build-tool-specific config (setuptools, poetry, hatch all differ), detect the common `src/` layout convention.

**New struct:**
```rust
pub struct PythonConfig {
    pub package_roots: Vec<PathBuf>,
}

impl PythonConfig {
    pub fn load(project_root: &Path) -> Self {
        let src = project_root.join("src");
        let package_roots = if src.is_dir() { vec![src] } else { vec![] };
        PythonConfig { package_roots }
    }
}
```

**Integration:** `PythonResolver` stores `PythonConfig` and prepends `package_roots` to resolution paths when resolving relative imports.

**Defaults:** Empty `package_roots` → resolve from project root only (current behavior).

#### Rust — data-only workspace parsing (no resolution changes)

**New struct:**
```rust
pub struct RustConfig {
    pub workspace_members: Vec<String>,  // e.g., ["crates/foo", "crates/bar"]
    pub edition: Option<String>,
}

impl RustConfig {
    pub fn load(project_root: &Path) -> Self {
        // Parse Cargo.toml [workspace].members
        // Returns empty if no [workspace] section
    }
}
```

**Integration:** `RustResolver` stores `RustConfig` but does NOT change resolution behavior in this slice. The parsed data is available for future cross-crate resolution (separate slice). This is data-only preparation.

**Defaults:** Empty `workspace_members`, no edition → current single-crate resolution behavior preserved.

#### Go — wrap existing logic

**New struct:**
```rust
pub struct GoConfig {
    pub module_path: Option<String>,
}

impl GoConfig {
    pub fn load(project_root: &Path) -> Self {
        GoConfig { module_path: parse_go_mod(project_root) }
    }
}
```

**Integration:** Wraps existing `parse_go_mod()` logic. `GoResolver` stores `GoConfig` instead of calling `parse_go_mod` at resolve time. No behavior change.

### 2. Storage Layer — Filtered Queries

**Location:** `crates/domain/src/ports.rs` (trait) + `crates/storage/src/graph_store.rs` (impl)

**New trait methods on `GraphStore`** (with default impls to avoid breaking existing implementors):

```rust
/// Returns symbols only for the specified file paths.
/// Default: calls all_symbols() and filters in memory (correctness fallback).
fn symbols_for_files(&self, paths: &[&Path]) -> Result<Vec<Symbol>> {
    let all = self.all_symbols()?;
    Ok(all.into_iter().filter(|s| paths.contains(&&*s.location.file)).collect())
}

/// Processes edges row-by-row via callback.
/// Default: loads all_edges() and iterates (correctness fallback).
/// Object-safe: uses &mut dyn FnMut.
fn edges_streaming(&self, callback: &mut dyn FnMut(Edge) -> Result<()>) -> Result<()> {
    for edge in self.all_edges()? {
        callback(edge)?;
    }
    Ok(())
}
```

**SQLite implementation** (overrides defaults with efficient queries):
- `symbols_for_files` — `SELECT ... FROM symbols WHERE file_path IN (?)` via `rusqlite::params_from_iter`
- `edges_streaming` — `SELECT ... FROM edges` with `stmt.query_map()`, calling the callback per row without collecting into a Vec

**Honest assessment of streaming benefit:** The per-row Edge allocations (String fields) are the real memory cost, not the Vec shell. Streaming avoids peak memory from holding the full `Vec<Edge>` + the `InMemoryGraph` simultaneously, but individual Edge allocations still occur. This is a modest win for large graphs (avoids 2x peak memory), not a dramatic improvement. The real optimization (passing borrowed `&str` from SQLite rows) is deferred to M02.

**InMemoryGraph incremental construction:**
- Add `InMemoryGraph::new()` + `pub fn add_edge(&mut self, edge: Edge)` method
- `ImpactUseCase::blast_radius()` builds graph via streaming

**Consumer updates:**
- `IndexUseCase::incremental_index()` — replace `all_symbols()` with `symbols_for_files(&reparse_set)`
- `ImpactUseCase::blast_radius()` — replace `all_edges()` with `edges_streaming()` + incremental `InMemoryGraph` construction
- `InMemoryGraphStore` (test double) — inherits default trait impls; additionally overrides `symbols_for_files()` to track call counts for verification
- Existing `all_symbols()` / `all_edges()` retained (used by stats, eval, and as default fallback)

### 3. Benchmark Crate

**Location:** `crates/benches/` as a workspace member

**Cargo.toml:**

```toml
[package]
name = "code-graph-benches"
version = "0.1.0"
edition = "2021"
publish = false

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
code-graph-parser = { path = "../parser" }
code-graph-storage = { path = "../storage" }
code-graph-domain = { path = "../domain" }
code-graph-cli = { path = "../cli" }
tempfile = "3"

[[bench]]
name = "parse_throughput"
harness = false

[[bench]]
name = "query_latency"
harness = false

[[bench]]
name = "incremental_latency"
harness = false

[[bench]]
name = "impact_latency"
harness = false
```

Added to root `Cargo.toml` workspace `members` list.

**Benchmark groups:**

| Group | What it measures | Fixture |
|---|---|---|
| `parse_throughput` | Files parsed per second, per language | 50 committed source files, scaled to 1000+ via programmatic duplication at bench time |
| `query_latency` | find, search, refs, callers on pre-indexed graph | SQLite DB built at bench time; graph synthesized to 10K+ symbols for meaningful results |
| `incremental_latency` | Re-index after 1-file modification | Same synthesized graph + simulated file change |
| `impact_latency` | Blast radius at depth 1, 2, 3 | Same synthesized graph |

**Fixtures:**
- Small committed corpus in `crates/benches/fixtures/` (~50 real source files, ~50KB)
- Bench setup functions scale up programmatically (duplicate files, synthesize symbols) to produce meaningful measurements
- Baseline numbers stored in `target/criterion/` (gitignored, CI uploads as artifact)

**CI integration:**
- Lefthook pre-push: `cargo bench --no-run` (compile check only)
- GitHub Actions: dedicated bench job, uploads `target/criterion/` as CI artifact

### 4. Remove Unix Shell Dependency

**Location:** `crates/cli/src/commands/setup_helpers.rs`

**Change:**
- Add `which = "7"` to `crates/cli/Cargo.toml`
- Replace `Command::new("which").arg(binary).output()` with `which::which(binary)`
- This removes a Unix-only shell dependency. It is NOT a full cross-platform fix — other Windows incompatibilities exist (e.g., `$HOME` env var usage) but are out of scope for v0.1.

---

## Acceptance Criteria

1. **AC1 — TypeScript resolver uses tsconfig.json:** Given a project with `tsconfig.json` containing `"paths"` and `"baseUrl"`, the TypeScript resolver uses `TsconfigDiscovery::Auto` on `oxc_resolver::ResolveOptions`. Verified by: (a) unit test confirming a path-mapped import resolves correctly, (b) unit test confirming resolution works without tsconfig.json present.

2. **AC2 — Python resolver detects src/ layout:** Given a project with a `src/` directory, the Python resolver uses it as a package root. When `src/` is absent, resolves from project root only. Verified by unit tests for both cases.

3. **AC3 — Rust config parses workspace members (data-only):** `RustConfig::load()` parses `[workspace].members` from `Cargo.toml`. Returns empty when no workspace section exists. Resolution behavior is unchanged. Verified by unit tests for `RustConfig::load()` + existing Rust resolver tests still passing.

4. **AC4 — Go config wraps existing logic:** `GoConfig::load()` wraps `parse_go_mod()`. Existing Go resolution behavior unchanged. Verified by existing Go resolver tests passing.

5. **AC5 — Filtered storage queries:** `symbols_for_files(&[path_a])` returns only symbols where `file_path == path_a` — verified by unit test against `SqliteStore`. Incremental index calls `symbols_for_files()` — verified by `InMemoryGraphStore` test double tracking call counts and asserting `symbols_for_files` was called instead of `all_symbols`.

6. **AC6 — Streaming edge loading:** `edges_streaming()` on `SqliteStore` invokes the callback per row — verified by unit test counting invocations vs `all_edges().len()`. `InMemoryGraph` supports `new()` + `add_edge()` — verified by unit test. `ImpactUseCase` uses `edges_streaming()` — verified by call-count tracking on test double.

7. **AC7 — Benchmarks compile and run:** `cargo bench --bench parse_throughput -- --test` exits 0 (criterion quick validation). Same for `query_latency`, `incremental_latency`, `impact_latency`. All 4 `[[bench]]` targets declared with `harness = false` in `crates/benches/Cargo.toml`. Crate listed in workspace `members`.

8. **AC8 — Remove Unix shell dependency:** `setup_helpers::find_on_path()` uses `which::which()`. `grep -r 'Command::new("which")' crates/` returns empty.

9. **AC9 — No regressions:** `cargo test --workspace` exits 0. `cargo clippy --workspace -- -D warnings` exits 0. `cargo llvm-cov --workspace --fail-under-lines 80` exits 0.

---

## Out of Scope

- Hard performance targets (deferred to M02)
- Import resolution caching (deferred to M02)
- Borrowed/zero-copy edge streaming (deferred to M02)
- Rust cross-crate resolution wiring (this slice is data-only; resolution changes are a separate slice)
- Full Windows cross-platform support (only removing the `which` shell dependency)
- tsconfig.json `extends` chain handling (handled by oxc_resolver automatically)
- Python build-tool-specific config parsing (using heuristic instead)

## Dependencies

- `oxc_resolver` must support `TsconfigDiscovery::Auto` (confirmed: available in v11+ already in deps)
- `which` crate v7 (new dependency for cli crate)
- `criterion` v0.5 (new dev-dependency for benches crate)

## Risks

| Risk | Mitigation |
|---|---|
| oxc_resolver tsconfig support has edge cases | Already battle-tested in oxc ecosystem; our tests verify basic paths/baseUrl resolution |
| Benchmark fixtures too small for meaningful results | Programmatic scaling at bench time; committed corpus is just seeds |
| Streaming edge loading has modest memory benefit | Honestly documented; real optimization (borrowed strings) deferred to M02 |
| RustConfig is dead data without resolution wiring | Explicitly scoped as data-only preparation; future slice does the wiring |
