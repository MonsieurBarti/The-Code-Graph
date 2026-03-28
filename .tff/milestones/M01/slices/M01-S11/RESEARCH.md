# Research — M01-S11: Polish, Benchmarks & Performance

## 1. Resolver Configuration

### 1.1 TypeScript — oxc_resolver tsconfig

**Current state:** `TypeScriptResolver` (`crates/parser/src/resolver/typescript.rs:10-12`) stores `_project_root: PathBuf` but never uses it. `build_resolver()` (line 21) creates `oxc_resolver::ResolveOptions` with extensions, conditions, and main_fields only — no tsconfig discovery.

**oxc_resolver version:** 11.19.1 (Cargo.lock:928). `TsconfigDiscovery::Auto` is available.

**Spec correction needed:** The spec describes a nested `TsconfigOptions { config_file: TsconfigDiscovery::Auto, references: TsconfigReferences::Auto }`. The actual API in oxc_resolver 11.19.1 is:
```rust
ResolveOptions {
    tsconfig: Some(TsconfigDiscovery::Auto),
    // ...
}
```
The `tsconfig` field is `Option<TsconfigDiscovery>`, not `Option<TsconfigOptions>`. `TsconfigDiscovery::Auto` handles both paths/baseUrl and references automatically. This is a simpler change than the spec suggests.

**Integration:** The `_project_root` field is already stored. `build_resolver()` currently takes no args and is called in `resolve_specifier()` per-file. Adding `tsconfig: Some(TsconfigDiscovery::Auto)` to the existing `ResolveOptions` block is a ~2-line change.

**Risk:** None. `TsconfigDiscovery::Auto` falls back silently when no tsconfig.json exists (default `None` behavior preserved).

### 1.2 Python — src/ layout detection

**Current state:** `PythonResolver` (`crates/parser/src/resolver/python.rs:293`) is zero-sized and stateless. Import resolution uses `project_root` from `ResolveContext` as sole base for absolute imports (line 284). No `src/` detection.

**Integration path:** Add `PythonConfig { package_roots: Vec<PathBuf> }` with `load()` that checks `project_root.join("src").is_dir()`. Pass config to `PythonResolver` at construction in `ResolverRegistry::new()`. Modify `resolve_python_import()` to check `package_roots` before `project_root`.

**Concern:** `PythonResolver` currently receives project_root only via `ResolveContext` at resolve-time, not at construction. Need to change to accept config at construction (similar pattern to `TypeScriptResolver::new()`).

### 1.3 Rust — data-only workspace parsing

**Current state:** `RustResolver` (`crates/parser/src/resolver/rust_lang.rs:10`) is zero-sized. No Cargo.toml parsing. Module tree built from code declarations only.

**Integration path:** Add `RustConfig { workspace_members: Vec<String>, edition: Option<String> }` with `load()` that parses root `Cargo.toml`. `toml` v0.8 is already a workspace dependency. Data-only — no resolution behavior changes.

**Concern:** None. This is purely additive data parsing.

### 1.4 Go — wrap parse_go_mod

**Current state:** `GoResolver` (`crates/parser/src/resolver/go.rs:9`) is zero-sized. `parse_go_mod()` (lines 12-25) is called per-resolution at line 72. Returns `Option<String>` (module path).

**Integration path:** Add `GoConfig { module_path: Option<String> }` with `load()` that calls existing `parse_go_mod()`. Store in `GoResolver` at construction. Replace per-resolve call with cached value.

**Concern:** None. Mechanical refactor wrapping existing logic.

### 1.5 ResolverRegistry

**Current state:** `ResolverRegistry::new()` (`crates/parser/src/resolver/mod.rs:42-52`) passes `project_root` only to `TypeScriptResolver`. Python, Rust, Go resolvers created with no args.

**Change:** Construct config structs and pass to each resolver at construction:
```rust
pub fn new(project_root: &Path) -> Self {
    let python_config = PythonConfig::load(project_root);
    let rust_config = RustConfig::load(project_root);
    let go_config = GoConfig::load(project_root);
    // ... register resolvers with configs
}
```

---

## 2. Storage Layer — Filtered Queries

### 2.1 GraphStore trait

**Current state:** `GraphStore` (`crates/domain/src/ports.rs:6-32`) has `all_symbols()` and `all_edges()` as bulk methods. No filtered variants exist. Trait is object-safe (`Send + Sync`, tested at line 73).

**Adding methods with defaults:** Object-safe default impls are straightforward — filter in memory as fallback. SQLite overrides with efficient queries.

### 2.2 symbols_for_files

**SQLite feasibility:** The `symbols` table has `file_path` column with index `idx_symbols_file` (`crates/storage/src/schema.rs`). Dynamic `WHERE file_path IN (?, ?, ...)` is feasible using `rusqlite::params_from_iter` (confirmed available in rusqlite 0.37).

**Implementation:** Build placeholder string dynamically, use `params_from_iter()` for the path slice.

### 2.3 edges_streaming

**Current state:** `all_edges()` in `SqliteStore` (`crates/storage/src/graph_store.rs:380-406`) uses `prepare_cached("SELECT ... FROM edges")` and collects into `Vec<Edge>`.

**Streaming approach:** Use `stmt.query_map()` and invoke callback per row. Avoids holding full `Vec<Edge>` + `InMemoryGraph` simultaneously. As spec notes, individual Edge allocations still occur per row — this is a modest peak-memory win.

### 2.4 InMemoryGraph incremental construction

**Current state:** `InMemoryGraph` (`crates/domain/src/traversal.rs:8-11`) only has `from_edges(edges: Vec<Edge>)` constructor (line 14). No `new()` or `add_edge()`.

**Change needed:** Add:
```rust
pub fn new() -> Self { ... }
pub fn add_edge(&mut self, edge: Edge) { ... }
```
The `add_edge` logic is identical to the loop body in `from_edges()` (lines 18-27).

### 2.5 Consumer updates

**IndexUseCase::incremental_index:** (`crates/domain/src/use_cases/index.rs:114`) calls `store.all_symbols()` then filters in-memory for reparse_set paths. Replace with `store.symbols_for_files(&reparse_set)`.

**ImpactUseCase::blast_radius:** (`crates/domain/src/use_cases/impact.rs:23-24`) calls `store.all_edges()` then `InMemoryGraph::from_edges()`. Replace with `edges_streaming()` + incremental `InMemoryGraph` construction.

### 2.6 InMemoryGraphStore test double

**Current state:** (`crates/domain/src/test_support.rs:8-120`). Implements GraphStore with simple Vec storage. Will inherit default impls for new methods. Can additionally override `symbols_for_files()` to track call counts for verification.

---

## 3. Benchmark Crate

### 3.1 Workspace setup

**Current members:** domain, storage, parser, watch, cli, binary, eval (`Cargo.toml:2`). `crates/benches/` does not exist.

**Action:** Create `crates/benches/` with `Cargo.toml` declaring criterion dev-dep and 4 `[[bench]]` targets. Add to workspace members.

### 3.2 CI and hooks — already configured

**CI:** `.github/workflows/ci.yml:86-99` has `bench` job running `cargo bench --no-run`.

**Lefthook:** `lefthook.yml:14-19` has `bench-check` pre-push hook running `cargo bench --no-run`.

Both are ready — they'll pick up the new bench crate automatically once it's a workspace member.

### 3.3 Fixtures

No fixtures directory exists. Need to create `crates/benches/fixtures/` with ~50 committed source files across languages. Bench setup functions scale programmatically.

**Existing test infrastructure:** Parser has `test_utils.rs` with inline code helpers. Eval has dataset/manifest types. Neither provides file-based fixtures suitable for benchmarking.

### 3.4 Criterion version

Criterion v0.5 is not currently in any workspace Cargo.toml. Will be added as dev-dependency to `crates/benches/Cargo.toml`.

---

## 4. Unix Shell Dependency

### 4.1 Current implementation

**Location:** `crates/cli/src/commands/setup_helpers.rs:109-122`

```rust
pub(super) fn find_on_path(binary: &str) -> Option<PathBuf> {
    let output = Command::new("which").arg(binary).output().ok()?;
    // ...
}
```

Single call site for `Command::new("which")`. Used 3 times in `setup.rs` (lines 150-151, 248) to check for `code-graph` and `jq` binaries.

### 4.2 Replacement

Add `which = "7"` to `crates/cli/Cargo.toml`. Replace body with:
```rust
pub(super) fn find_on_path(binary: &str) -> Option<PathBuf> {
    which::which(binary).ok()
}
```

### 4.3 Tests

Two existing unit tests (`setup_helpers.rs:254-266`) test with real binaries (`ls`, nonexistent). They'll pass unchanged with the `which` crate since behavior is identical.

---

## 5. Dependency Summary

| Dependency | Crate | Version | Status |
|---|---|---|---|
| `oxc_resolver` | parser | 11.19.1 | Already present — just enable tsconfig |
| `toml` | cli | 0.8 | Already present — reuse for Cargo.toml parsing |
| `which` | cli | 7 | **New** — add to Cargo.toml |
| `criterion` | benches | 0.5 | **New** — add to new crate |
| `tempfile` | benches | 3 | Already in workspace — add to new crate |

## 6. Spec Corrections

1. **TypeScript tsconfig API:** Spec says `TsconfigOptions { config_file: TsconfigDiscovery::Auto, references: TsconfigReferences::Auto }`. Actual API is `tsconfig: Some(TsconfigDiscovery::Auto)` on `ResolveOptions`. Simpler.

2. **rusqlite params_from_iter:** Spec mentions it — confirmed available in rusqlite 0.37. No issue.

3. **InMemoryGraph:** Spec says "Add `InMemoryGraph::new()` + `add_edge()`" — confirmed needed, neither exists today.

## 7. Risk Assessment

| Risk | Level | Notes |
|---|---|---|
| oxc_resolver tsconfig API mismatch | Low | Verified actual API; simpler than spec assumed |
| rusqlite dynamic params | None | `params_from_iter` confirmed available |
| InMemoryGraph changes | Low | Additive methods, existing `from_edges` preserved |
| Benchmark fixture creation | Medium | Need to create ~50 real source files; most effort in this slice |
| Python config integration | Low | Straightforward pattern matching TS resolver approach |
