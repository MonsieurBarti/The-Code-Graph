# M01-S11: Polish, Benchmarks & Performance — Implementation Plan

> For agentic workers: execute task-by-task with TDD.

**Goal:** Close all known v0.1 gaps — resolver config reading, filtered storage queries, criterion benchmarks, and cross-platform fixes.
**Architecture:** Fix-forward with per-language typed configs; no shared trait. Storage layer gains filtered query methods with default impls.
**Tech Stack:** Rust, oxc_resolver 11.19, rusqlite 0.37, criterion 0.5, which 7

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `crates/parser/src/resolver/typescript.rs` | Modify | Enable `TsconfigDiscovery::Auto` on ResolveOptions |
| `crates/parser/src/resolver/python.rs` | Modify | Add `PythonConfig` with `package_roots`, wire into resolver |
| `crates/parser/src/resolver/rust_lang.rs` | Modify | Add `RustConfig` (data-only workspace parsing) |
| `crates/parser/src/resolver/go.rs` | Modify | Add `GoConfig` wrapping `parse_go_mod()` |
| `crates/parser/src/resolver/mod.rs` | Modify | Construct configs in `ResolverRegistry::new()` |
| `crates/domain/src/ports.rs` | Modify | Add `symbols_for_files()` + `edges_streaming()` with defaults |
| `crates/domain/src/traversal.rs` | Modify | Add `InMemoryGraph::new()` + `add_edge()` |
| `crates/domain/src/test_support.rs` | Modify | Call-count tracking for `symbols_for_files` |
| `crates/storage/src/graph_store.rs` | Modify | SQLite overrides with efficient queries |
| `crates/domain/src/use_cases/index.rs` | Modify | Replace `all_symbols()` with `symbols_for_files()` |
| `crates/domain/src/use_cases/impact.rs` | Modify | Replace `all_edges()` with `edges_streaming()` |
| `Cargo.toml` | Modify | Add `crates/benches` to workspace members |
| `crates/benches/Cargo.toml` | Create | Benchmark crate with criterion dev-dep |
| `crates/benches/benches/parse_throughput.rs` | Create | Parse benchmark |
| `crates/benches/benches/query_latency.rs` | Create | Query benchmark |
| `crates/benches/benches/incremental_latency.rs` | Create | Incremental re-index benchmark |
| `crates/benches/benches/impact_latency.rs` | Create | Impact analysis benchmark |
| `crates/benches/fixtures/` | Create | ~50 source files across TS/Py/Rs/Go |
| `crates/benches/src/lib.rs` | Create | Shared benchmark helpers (fixture loading, graph synthesis) |
| `crates/cli/Cargo.toml` | Modify | Add `which = "7"` |
| `crates/cli/src/commands/setup_helpers.rs` | Modify | Replace `Command::new("which")` with `which::which()` |

---

### Task 1: TypeScript tsconfig auto-discovery
**Files:** Modify `crates/parser/src/resolver/typescript.rs` (test + impl)
**Traces to:** AC1

- [ ] Step 1: Write failing test — test via `ImportResolver::resolve()` trait method (note: `resolve_specifier` is private)
```rust
// In typescript.rs, add to #[cfg(test)] mod tests:
#[test]
fn tsconfig_path_alias_resolves() {
    // Set up temp dir with tsconfig.json containing path mappings
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/foo.ts"), "export const foo = 1;").unwrap();
    let index = dir.path().join("index.ts");
    std::fs::write(&index, "import { foo } from '@/foo';").unwrap();

    // Exercise through the public ImportResolver::resolve() trait method
    let resolver = TypeScriptResolver::new(dir.path());
    let parsed = crate::parse_file(&index, &std::fs::read(&index).unwrap()).unwrap();
    let context = super::ResolveContext {
        project_root: dir.path().to_path_buf(),
        parsed_files: std::collections::HashMap::from([(index.clone(), parsed)]),
        file_tree: vec![index.clone(), dir.path().join("src/foo.ts")],
    };
    let edges = resolver.resolve(&index, &context);
    assert!(!edges.is_empty(), "path-mapped import should produce edges with tsconfig discovery");
}

#[test]
fn resolver_works_without_tsconfig() {
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("index.ts");
    std::fs::write(&index, "import { foo } from './foo';").unwrap();
    std::fs::write(dir.path().join("foo.ts"), "export const foo = 1;").unwrap();

    let resolver = TypeScriptResolver::new(dir.path());
    let parsed = crate::parse_file(&index, &std::fs::read(&index).unwrap()).unwrap();
    let context = super::ResolveContext {
        project_root: dir.path().to_path_buf(),
        parsed_files: std::collections::HashMap::from([(index.clone(), parsed)]),
        file_tree: vec![index.clone(), dir.path().join("foo.ts")],
    };
    let edges = resolver.resolve(&index, &context);
    assert!(!edges.is_empty(), "relative import should still resolve without tsconfig");
}
```
- [ ] Step 2: Run `cargo test -p parser --lib -- typescript::tests::tsconfig_path_alias_resolves`, verify FAIL (path alias unresolved)
- [ ] Step 3: Implement — in `build_resolver()`, add `tsconfig: Some(oxc_resolver::TsconfigDiscovery::Auto),` to `ResolveOptions`
```rust
fn build_resolver() -> oxc_resolver::Resolver {
    oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
        tsconfig: Some(oxc_resolver::TsconfigDiscovery::Auto),
        extensions: vec![/* ... existing ... */],
        condition_names: vec![/* ... existing ... */],
        main_fields: vec![/* ... existing ... */],
        ..Default::default()
    })
}
```
- [ ] Step 4: Run `cargo test -p parser --lib -- typescript::tests::build_resolver`, verify PASS (both tests)
- [ ] Step 5: `git add crates/parser/src/resolver/typescript.rs && git commit -m "feat(S11/T01): enable tsconfig auto-discovery in TypeScript resolver"`

---

### Task 2: Python src/ config + registry wiring
**Files:** Modify `crates/parser/src/resolver/python.rs`, Modify `crates/parser/src/resolver/mod.rs`
**Traces to:** AC2

- [ ] Step 1: Write failing test
```rust
// In python.rs, add:
#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn python_config_detects_src_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let config = PythonConfig::load(dir.path());
        assert_eq!(config.package_roots.len(), 1);
        assert_eq!(config.package_roots[0], dir.path().join("src"));
    }

    #[test]
    fn python_config_empty_without_src() {
        let dir = tempfile::tempdir().unwrap();
        let config = PythonConfig::load(dir.path());
        assert!(config.package_roots.is_empty());
    }
}
```
- [ ] Step 2: Run `cargo test -p parser --lib -- config_tests`, verify FAIL (PythonConfig not found)
- [ ] Step 3: Implement
```rust
// In python.rs, add before PythonResolver:
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

// Change PythonResolver from unit struct to:
pub struct PythonResolver {
    config: PythonConfig,
}

impl PythonResolver {
    pub fn new(config: PythonConfig) -> Self {
        Self { config }
    }
}
```
Update `resolve_python_import()`: add `package_roots: &[PathBuf]` parameter. In the absolute import branch (around line 282-285), try each `package_root` before falling back to `project_root`:
```rust
// In resolve_python_import, absolute import resolution:
// OLD (line ~282):
//   let candidate = project_root.join(&rel);
//   try_resolve(&candidate, file_tree)
// NEW: try package_roots first, then project_root
for root in package_roots {
    let candidate = root.join(&rel);
    if let Some(resolved) = try_resolve(&candidate, file_tree) {
        return Some(resolved);
    }
}
let candidate = project_root.join(&rel);
try_resolve(&candidate, file_tree)
```
Pass `&self.config.package_roots` from `PythonResolver::resolve()` into `resolve_python_import()`.

Update `ResolverRegistry::new()` in mod.rs:
```rust
let python_config = python::PythonConfig::load(project_root);
registry.register(Box::new(python::PythonResolver::new(python_config)));
```
- [ ] Step 4: Run `cargo test -p parser --lib -- config_tests`, verify PASS + run `cargo test -p parser` for no regressions
- [ ] Step 5: `git add crates/parser/src/resolver/python.rs crates/parser/src/resolver/mod.rs && git commit -m "feat(S11/T02): add Python src/ layout config with registry wiring"`

---

### Task 3: Rust workspace config + registry wiring
**Files:** Modify `crates/parser/src/resolver/rust_lang.rs`, Modify `crates/parser/src/resolver/mod.rs`
**Traces to:** AC3
**Depends on:** T02 (shared mod.rs)

- [ ] Step 1: Write failing test
```rust
// In rust_lang.rs, add:
#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn rust_config_parses_workspace_members() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), r#"
[workspace]
members = ["crates/foo", "crates/bar"]

[package]
edition = "2021"
"#).unwrap();
        let config = RustConfig::load(dir.path());
        assert_eq!(config.workspace_members, vec!["crates/foo", "crates/bar"]);
        assert_eq!(config.edition.as_deref(), Some("2021"));
    }

    #[test]
    fn rust_config_empty_without_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), r#"
[package]
name = "solo"
edition = "2021"
"#).unwrap();
        let config = RustConfig::load(dir.path());
        assert!(config.workspace_members.is_empty());
    }

    #[test]
    fn rust_config_empty_without_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        let config = RustConfig::load(dir.path());
        assert!(config.workspace_members.is_empty());
        assert!(config.edition.is_none());
    }
}
```
- [ ] Step 2: Run `cargo test -p parser --lib -- config_tests::rust_config`, verify FAIL
- [ ] Step 3: Implement
```rust
// In rust_lang.rs, add:
pub struct RustConfig {
    pub workspace_members: Vec<String>,
    pub edition: Option<String>,
}

impl RustConfig {
    pub fn load(project_root: &Path) -> Self {
        let cargo_path = project_root.join("Cargo.toml");
        let contents = match std::fs::read_to_string(&cargo_path) {
            Ok(c) => c,
            Err(_) => return Self { workspace_members: vec![], edition: None },
        };
        let table: toml::Table = match contents.parse() {
            Ok(t) => t,
            Err(_) => return Self { workspace_members: vec![], edition: None },
        };
        let workspace_members = table.get("workspace")
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let edition = table.get("package")
            .and_then(|p| p.get("edition"))
            .and_then(|e| e.as_str())
            .map(String::from);
        Self { workspace_members, edition }
    }
}

// Change RustResolver from unit struct to:
pub struct RustResolver {
    _config: RustConfig,
}
impl RustResolver {
    pub fn new(config: RustConfig) -> Self { Self { _config: config } }
}
```
Update mod.rs registry:
```rust
let rust_config = rust_lang::RustConfig::load(project_root);
registry.register(Box::new(rust_lang::RustResolver::new(rust_config)));
```
- [ ] Step 4: Run `cargo test -p parser`, verify all PASS (config tests + existing rust resolver tests)
- [ ] Step 5: `git add crates/parser/src/resolver/rust_lang.rs crates/parser/src/resolver/mod.rs && git commit -m "feat(S11/T03): add Rust workspace config (data-only)"`

---

### Task 4: Go config wrap + registry wiring
**Files:** Modify `crates/parser/src/resolver/go.rs`, Modify `crates/parser/src/resolver/mod.rs`
**Traces to:** AC4
**Depends on:** T03 (shared mod.rs)

- [ ] Step 1: Write failing test
```rust
// In go.rs, add:
#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn go_config_loads_module_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module github.com/example/app\n\ngo 1.21\n").unwrap();
        let config = GoConfig::load(dir.path());
        assert_eq!(config.module_path.as_deref(), Some("github.com/example/app"));
    }

    #[test]
    fn go_config_none_without_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        let config = GoConfig::load(dir.path());
        assert!(config.module_path.is_none());
    }
}
```
- [ ] Step 2: Run `cargo test -p parser --lib -- config_tests::go_config`, verify FAIL
- [ ] Step 3: Implement
```rust
pub struct GoConfig {
    pub module_path: Option<String>,
}

impl GoConfig {
    pub fn load(project_root: &Path) -> Self {
        GoConfig { module_path: parse_go_mod(project_root) }
    }
}

// Change GoResolver from unit struct to:
pub struct GoResolver {
    config: GoConfig,
}
impl GoResolver {
    pub fn new(config: GoConfig) -> Self { Self { config } }
}
```
Update `resolve()` to use `self.config.module_path` instead of calling `parse_go_mod()`. Update mod.rs registry:
```rust
let go_config = go::GoConfig::load(project_root);
registry.register(Box::new(go::GoResolver::new(go_config)));
```
- [ ] Step 4: Run `cargo test -p parser`, verify all PASS
- [ ] Step 5: `git add crates/parser/src/resolver/go.rs crates/parser/src/resolver/mod.rs && git commit -m "feat(S11/T04): wrap Go parse_go_mod into GoConfig"`

---

### Task 5: GraphStore filtered queries + InMemoryGraph incremental
**Files:** Modify `crates/domain/src/ports.rs`, Modify `crates/domain/src/traversal.rs`, Modify `crates/domain/src/test_support.rs`
**Traces to:** AC5, AC6

- [ ] Step 1: Write failing tests
```rust
// In traversal.rs tests:
#[test]
fn in_memory_graph_incremental_construction() {
    let mut graph = InMemoryGraph::new();
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: "a::foo".into(),
        target: "b::bar".into(),
        metadata: None,
    });
    let results = graph.bfs("a::foo", Direction::Outgoing, 1);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].node, "b::bar");
}
```
```rust
// In ports.rs tests:
#[test]
fn graph_store_default_symbols_for_files() {
    // Verifies trait compiles with new methods
    fn assert_has_method<T: GraphStore>(store: &T) {
        let _ = store.symbols_for_files(&[]);
    }
}
```
- [ ] Step 2: Run `cargo test -p domain --lib -- in_memory_graph_incremental`, verify FAIL
- [ ] Step 3: Implement
```rust
// In traversal.rs, add to impl InMemoryGraph:
pub fn new() -> Self {
    InMemoryGraph {
        outgoing: HashMap::new(),
        incoming: HashMap::new(),
    }
}

pub fn add_edge(&mut self, edge: Edge) {
    self.outgoing
        .entry(edge.source.clone())
        .or_default()
        .push((edge.target.clone(), edge.kind));
    self.incoming
        .entry(edge.target)
        .or_default()
        .push((edge.source, edge.kind));
}
```
```rust
// In ports.rs, add to GraphStore trait:
fn symbols_for_files(&self, paths: &[&Path]) -> Result<Vec<SymbolNode>> {
    let all = self.all_symbols()?;
    Ok(all.into_iter().filter(|s| paths.contains(&&*s.location.file)).collect())
}

fn edges_streaming(&self, callback: &mut dyn FnMut(Edge) -> Result<()>) -> Result<()> {
    for edge in self.all_edges()? {
        callback(edge)?;
    }
    Ok(())
}
```
```rust
// In test_support.rs, add call tracking to InMemoryGraphStore:
use std::sync::atomic::{AtomicUsize, Ordering};

// IMPORTANT: AtomicUsize does not derive Clone. Remove #[derive(Clone)]
// and implement Clone manually (cloning counter values into new atomics).
// Cloned stores get independent counters — always assert on the original.
pub struct InMemoryGraphStore {
    files: Vec<FileNode>,
    symbols: Vec<SymbolNode>,
    edges: Vec<Edge>,
    pub symbols_for_files_calls: AtomicUsize,
    pub edges_streaming_calls: AtomicUsize,
}

impl Clone for InMemoryGraphStore {
    fn clone(&self) -> Self {
        Self {
            files: self.files.clone(),
            symbols: self.symbols.clone(),
            edges: self.edges.clone(),
            symbols_for_files_calls: AtomicUsize::new(self.symbols_for_files_calls.load(Ordering::Relaxed)),
            edges_streaming_calls: AtomicUsize::new(self.edges_streaming_calls.load(Ordering::Relaxed)),
        }
    }
}

// Override symbols_for_files: increment counter + filter by paths
fn symbols_for_files(&self, paths: &[&Path]) -> Result<Vec<SymbolNode>> {
    self.symbols_for_files_calls.fetch_add(1, Ordering::Relaxed);
    Ok(self.symbols.iter().filter(|s| paths.contains(&&*s.location.file)).cloned().collect())
}

// Override edges_streaming: increment counter + iterate
fn edges_streaming(&self, callback: &mut dyn FnMut(Edge) -> Result<()>) -> Result<()> {
    self.edges_streaming_calls.fetch_add(1, Ordering::Relaxed);
    for edge in &self.edges { callback(edge.clone())?; }
    Ok(())
}
```
- [ ] Step 4: Run `cargo test -p domain`, verify all PASS
- [ ] Step 5: `git add crates/domain/src/ports.rs crates/domain/src/traversal.rs crates/domain/src/test_support.rs && git commit -m "feat(S11/T05): add filtered GraphStore queries and InMemoryGraph incremental construction"`

---

### Task 6: SqliteStore filtered query implementations
**Files:** Modify `crates/storage/src/graph_store.rs`
**Traces to:** AC5, AC6
**Depends on:** T05

- [ ] Step 1: Write failing tests
```rust
// In graph_store.rs tests (or storage integration tests):
#[test]
fn symbols_for_files_returns_filtered_subset() {
    let store = SqliteStore::open_in_memory().unwrap();
    // Insert symbols for file_a and file_b
    // Call symbols_for_files(&[Path::new("file_a.rs")])
    // Assert only file_a symbols returned
}

#[test]
fn edges_streaming_invokes_callback_per_row() {
    let store = SqliteStore::open_in_memory().unwrap();
    // Insert 3 edges
    // Call edges_streaming with counter callback
    // Assert counter == 3 == all_edges().len()
}
```
- [ ] Step 2: Run `cargo test -p storage --lib -- symbols_for_files`, verify FAIL
- [ ] Step 3: Implement SQLite overrides
```rust
// In GraphStore impl for SqliteStore:
fn symbols_for_files(&self, paths: &[&Path]) -> Result<Vec<SymbolNode>> {
    if paths.is_empty() {
        return Ok(vec![]);
    }
    let conn = self.conn()?;
    let placeholders: String = (0..paths.len()).map(|i| format!("?{}", i + 1)).collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT qualified_name, name, kind, file_path,
                line_start, line_end, col_start, col_end,
                visibility, is_exported, is_async, is_test,
                decorators, signature
         FROM symbols WHERE file_path IN ({})", placeholders
    );
    let mut stmt = conn.prepare(&sql).map_err(map_rusqlite_error)?;
    let params: Vec<&str> = paths.iter().map(|p| p.to_str().unwrap_or_default()).collect();
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        // ... same column mapping as all_symbols ...
    }).map_err(map_rusqlite_error)?;
    // ... collect into Vec<SymbolNode> ...
}

fn edges_streaming(&self, callback: &mut dyn FnMut(Edge) -> Result<()>) -> Result<()> {
    let conn = self.conn()?;
    let mut stmt = conn.prepare_cached(
        "SELECT kind, source_qualified, target_qualified, metadata FROM edges"
    ).map_err(map_rusqlite_error)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    }).map_err(map_rusqlite_error)?;
    for row in rows {
        let (kind, src, tgt, meta) = row.map_err(map_rusqlite_error)?;
        callback(Edge {
            kind: edge_kind_from_str(&kind)?,
            source: src,
            target: tgt,
            metadata: meta,
        })?;
    }
    Ok(())
}
```
- [ ] Step 4: Run `cargo test -p storage`, verify all PASS
- [ ] Step 5: `git add crates/storage/src/graph_store.rs && git commit -m "feat(S11/T06): SQLite filtered symbols_for_files and edges_streaming"`

---

### Task 7: Consumer updates — IndexUseCase + ImpactUseCase
**Files:** Modify `crates/domain/src/use_cases/index.rs`, Modify `crates/domain/src/use_cases/impact.rs`
**Traces to:** AC5, AC6
**Depends on:** T05

- [ ] Step 1: Write failing tests
```rust
// In index.rs tests (or domain integration tests):
#[test]
fn incremental_index_uses_symbols_for_files() {
    // Setup InMemoryGraphStore with files + symbols
    // Run incremental index on changed file
    // Assert symbols_for_files_calls > 0 (via AtomicUsize counter)
}
```
```rust
// In impact.rs tests:
#[test]
fn blast_radius_uses_edges_streaming() {
    // Setup InMemoryGraphStore with edges
    // Run blast_radius
    // Assert edges_streaming_calls > 0
}
```
- [ ] Step 2: Run tests, verify FAIL (counters are 0 because consumers still use all_symbols/all_edges)
- [ ] Step 3: Implement consumer changes
```rust
// In index.rs, replace line 114:
// OLD: let all_symbols = store.all_symbols()?;
// NEW: let path_refs: Vec<&Path> = reparse_set.iter().map(|p| p.as_path()).collect();
//      let file_symbols = store.symbols_for_files(&path_refs)?;
// Then iterate file_symbols directly instead of filtering all_symbols
```
```rust
// In impact.rs, replace blast_radius lines 21-22:
// OLD: let edges = self.store.all_edges()?;
//      let graph = InMemoryGraph::from_edges(edges);
// NEW: let mut graph = InMemoryGraph::new();
//      self.store.edges_streaming(&mut |edge| {
//          graph.add_edge(edge);
//          Ok(())
//      })?;
```
Also update `diff_impact` edges loading (line 35) to use `edges_streaming`. Note: `diff_impact` also calls `all_symbols()` (line 36) for `find_affected_symbols` — this is NOT replaced by `symbols_for_files` since diff_impact needs all symbols to match against diff hunks (no file filter available at that point). Only the edges loading is optimized here.
- [ ] Step 4: Run `cargo test -p domain`, verify all PASS (call tracking asserts + existing tests)
- [ ] Step 5: `git add crates/domain/src/use_cases/index.rs crates/domain/src/use_cases/impact.rs && git commit -m "feat(S11/T07): use filtered storage queries in index and impact use cases"`

---

### Task 8: Benchmark crate scaffold + fixtures
**Files:** Create `crates/benches/Cargo.toml`, Create `crates/benches/fixtures/`, Modify `Cargo.toml`
**Traces to:** AC7
**Note:** Scaffold task — TDD exempt (no behavior to test; validation is `cargo bench --no-run` exit 0)

- [ ] Step 1: Create crate scaffold
```toml
# crates/benches/Cargo.toml
[package]
name = "code-graph-benches"
version = "0.1.0"
edition = "2021"
publish = false

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
parser = { path = "../parser" }
storage = { path = "../storage" }
domain = { path = "../domain" }
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
- [ ] Step 2: Add to workspace: add `"crates/benches"` to `Cargo.toml` workspace members
- [ ] Step 3: Create fixture files — ~12 files per language (TS, Python, Rust, Go) with realistic imports, functions, classes. ~1KB each.
- [ ] Step 4: Create minimal benchmark stubs (empty criterion groups) so `cargo bench --no-run` compiles
- [ ] Step 5: Run `cargo bench --no-run`, verify exit 0
- [ ] Step 6: `git add Cargo.toml crates/benches/ && git commit -m "feat(S11/T08): scaffold benchmark crate with fixtures"`

---

### Task 9: Benchmark implementations
**Files:** Modify `crates/benches/benches/parse_throughput.rs`, `query_latency.rs`, `incremental_latency.rs`, `impact_latency.rs`
**Traces to:** AC7
**Depends on:** T08
**Note:** Benchmark task — TDD exempt. Validation is `cargo bench --bench X -- --test` exit 0 per criterion quick-check.

- [ ] Step 1: Implement `parse_throughput` — set up temp dir with duplicated fixture files, benchmark `parse_and_resolve` per language
- [ ] Step 2: Implement `query_latency` — build SQLite DB with synthesized 10K+ symbols, benchmark find/search/refs/callers
- [ ] Step 3: Implement `incremental_latency` — same synthesized graph, benchmark re-index after 1-file change
- [ ] Step 4: Implement `impact_latency` — same synthesized graph, benchmark blast_radius at depth 1, 2, 3
- [ ] Step 5: Run `cargo bench --bench parse_throughput -- --test && cargo bench --bench query_latency -- --test && cargo bench --bench incremental_latency -- --test && cargo bench --bench impact_latency -- --test`, verify all exit 0
- [ ] Step 6: `git add crates/benches/benches/ && git commit -m "feat(S11/T09): implement criterion benchmarks for parse, query, incremental, and impact"`

---

### Task 10: Replace Unix shell dependency
**Files:** Modify `crates/cli/Cargo.toml`, Modify `crates/cli/src/commands/setup_helpers.rs`
**Traces to:** AC8

- [ ] Step 1: Run existing tests to confirm baseline: `cargo test -p cli --lib -- find_on_path`, verify PASS
- [ ] Step 2: Add `which = "7"` to `crates/cli/Cargo.toml` under `[dependencies]`
- [ ] Step 3: Replace `find_on_path` implementation:
```rust
pub(super) fn find_on_path(binary: &str) -> Option<PathBuf> {
    which::which(binary).ok()
}
```
Remove the `use std::process::Command;` import if no longer used elsewhere in the file.
- [ ] Step 4: Run `cargo test -p cli --lib -- find_on_path`, verify PASS
- [ ] Step 5: Verify: `grep -r 'Command::new("which")' crates/` returns empty
- [ ] Step 6: `git add crates/cli/Cargo.toml crates/cli/src/commands/setup_helpers.rs && git commit -m "feat(S11/T10): replace Unix which command with which crate"`

---

### Task 11: Final validation — no regressions
**Files:** None (validation only)
**Traces to:** AC9
**Depends on:** T01–T10

- [ ] Step 1: Run `cargo test --workspace`, verify exit 0
- [ ] Step 2: Run `cargo clippy --workspace -- -D warnings`, verify exit 0
- [ ] Step 3: Run `cargo llvm-cov --workspace --fail-under-lines 80`, verify exit 0
- [ ] Step 4: Run `cargo bench --no-run`, verify exit 0 (compilation check for all benchmarks)

---

## Dependency Graph

```
T01 (TS tsconfig)           ─┐
T02 (Python config)         ─┤
T05 (GraphStore + InMemory) ─┤─── Wave 1 (independent)
T08 (Bench scaffold)        ─┤
T10 (which crate)           ─┘

T03 (Rust config)     ← T02 ─┐
T04 (Go config)       ← T03  │
T06 (SQLite impl)     ← T05 ─┤─── Wave 2
T07 (Consumer updates)← T05 ─┤
T09 (Bench impls)     ← T08 ─┘

T11 (Final validation) ← T01–T10 ── Wave 3
```

## Wave Summary

| Wave | Tasks | Parallel? |
|---|---|---|
| 1 | T01, T02, T05, T08, T10 | Yes (5 independent tracks) |
| 2 | T03, T04, T06, T07, T09 | T03→T04 sequential; T06, T07, T09 parallel |
| 3 | T11 | Sequential (full workspace validation) |
