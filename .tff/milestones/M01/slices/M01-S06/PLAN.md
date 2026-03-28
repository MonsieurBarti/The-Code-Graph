# Plan — M01-S06: Query Commands

> For agentic workers: execute task-by-task with TDD.

**Goal:** Wire the 8 remaining query/analysis commands (`find`, `refs`, `callers`, `callees`, `search`, `stats`, `impact`, `diff`) so `code-graph` delivers its core value: structural context from indexed codebases.

**Architecture:** Domain port additions (`find_by_name` on `GraphStore`, `diff_hunks` signature fix, `min_confidence` on `diff_impact`), one handler file per command in `cli::commands`, shared `open_graph()` helper, `Displayable` impls for all result types. No new crates, no new dependencies.

**Scope note:** `diff_hunks` was deferred to S07 in S05 but is now pulled into S06 since the `diff` command requires it. The git adapter's `diff_hunks` parser and the `GitProvider` signature change (`to: Option<&str>`) are both in scope.

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/domain/src/ports.rs` | Modify | Add `find_by_name` to `GraphStore`; change `diff_hunks` `to` param to `Option<&str>` |
| `crates/domain/src/test_support.rs` | Modify | Update `InMemoryGraphStore` + `MockGitProvider` for new signatures |
| `crates/domain/src/use_cases/query.rs` | Modify | Rewrite `find()` to call `find_by_name`, return `Vec<SymbolNode>` |
| `crates/domain/src/use_cases/impact.rs` | Modify | Add `min_confidence` param to `diff_impact` |
| `crates/storage/src/graph_store.rs` | Modify | Implement `find_by_name` SQL (exact match then prefix fallback) |
| `crates/cli/src/adapters/git.rs` | Modify | Implement `diff_hunks` parser (parse `git diff --unified=0`); update signature |
| `crates/cli/src/commands/mod.rs` | Modify | Fix `ImpactArgs` (rename field, add `--confidence`), fix `DiffArgs` (add `--depth`, `--confidence`); add module declarations |
| `crates/cli/src/commands/helpers.rs` | Create | `open_graph()` shared helper (project root + store opening) |
| `crates/cli/src/commands/find.rs` | Create | `find` handler with edge enrichment |
| `crates/cli/src/commands/refs.rs` | Create | `refs` handler |
| `crates/cli/src/commands/callers.rs` | Create | `callers` handler |
| `crates/cli/src/commands/callees.rs` | Create | `callees` handler |
| `crates/cli/src/commands/search.rs` | Create | `search` handler |
| `crates/cli/src/commands/stats.rs` | Create | `stats` handler |
| `crates/cli/src/commands/impact.rs` | Create | `impact` handler with target disambiguation |
| `crates/cli/src/commands/diff.rs` | Create | `diff` handler |
| `crates/cli/src/output.rs` | Modify | Add `FindResult` struct; implement `Displayable` for 6 result types |
| `crates/cli/src/lib.rs` | Modify | Wire 8 handlers into dispatcher, add module declarations |

---

## Acceptance Criteria

### Domain
- AC1: `GraphStore::find_by_name(pattern)` returns exact name matches, falling back to prefix match if zero exact matches.
- AC2: `QueryUseCase::find` calls `find_by_name` and returns `Result<Vec<SymbolNode>>`.
- AC3: `GitProvider::diff_hunks(&self, from: &str, to: Option<&str>)` — `None` means working tree.
- AC4: `ImpactUseCase::diff_impact` accepts `min_confidence: Confidence` parameter, propagated to `compute_blast_radius`.

### Storage
- AC5: `SqliteStore::find_by_name` uses SQL exact match on `name`, then `name LIKE ?%` prefix fallback.

### Git Adapter
- AC6: `ShellGitProvider::diff_hunks` parses `git diff --unified=0` output into `DiffHunk` structs.
- AC7: `to: None` maps to `git diff --unified=0 <from>` (working tree). `to: Some(ref)` maps to `git diff --unified=0 <from> <ref>`.

### CLI Commands
- AC8: `code-graph find <name>` returns matching symbols with callers, callees, and tested_by annotations. Compact format per design spec Section 7.2.
- AC9: `code-graph refs <qualified_name>` returns all incoming edges, one per line with source and edge kind.
- AC10: `code-graph callers <qualified_name>` returns incoming `Calls` edges only.
- AC11: `code-graph callees <qualified_name>` returns outgoing `Calls` edges only.
- AC12: `code-graph search <query>` returns symbols ordered by relevance score descending.
- AC13: `code-graph stats` returns file, symbol, and edge counts.
- AC14: `code-graph impact <target> [--depth N] [--confidence LEVEL]` disambiguates target (contains `::` = qualified, contains `/` or known extension = file path, else symbol name). Returns blast radius up to `--depth` (default 3), filtered to `--confidence` or above (default: all).
- AC15: `code-graph diff [from] [to] [--depth N] [--confidence LEVEL]` parses git diff, identifies overlapping symbols, computes blast radius. `from` defaults to `HEAD`; `to` defaults to working tree.
- AC16: All 8 commands support `--json` and `--table` output modes in addition to compact (default).

### Shared
- AC17: `open_graph()` helper handles project root detection + `SqliteStore::open`. All 8 handlers use it.

### Integration
- AC18: `cargo test --workspace` passes.
- AC19: `cargo clippy --workspace -- -Dwarnings` passes.

---

## Wave 0 — Domain Port & Use Case Changes

### T01: Add `find_by_name` to `GraphStore` + rewrite `QueryUseCase::find`
**AC coverage:** AC1, AC2
**Files:** `crates/domain/src/ports.rs`, `crates/domain/src/test_support.rs`, `crates/domain/src/use_cases/query.rs`

1. Write tests first in `query.rs`:
   - `find("foo")` with mock containing `SymbolNode { name: "foo", ... }` returns `vec![symbol]`
   - `find("fo")` with mock containing `SymbolNode { name: "foo", ... }` returns `vec![symbol]` (prefix match)
   - `find("bar")` with no matching symbols returns empty vec
   - `find("foo")` with exact + prefix matches returns only exact matches
2. Add to `GraphStore` trait in `ports.rs`:
   ```rust
   fn find_by_name(&self, pattern: &str) -> Result<Vec<SymbolNode>>;
   ```
3. Implement in `InMemoryGraphStore` (`test_support.rs`):
   - Exact: filter `name == pattern`
   - If empty, prefix: filter `name.starts_with(pattern)`
4. Rewrite `QueryUseCase::find`:
   ```rust
   pub fn find(&self, pattern: &str) -> Result<Vec<SymbolNode>> {
       self.store.find_by_name(pattern)
   }
   ```
5. `cargo test -p domain` passes

### T02: Change `GitProvider::diff_hunks` to `to: Option<&str>`
**AC coverage:** AC3
**Files:** `crates/domain/src/ports.rs`, `crates/domain/src/test_support.rs`, `crates/cli/src/adapters/git.rs`
**Depends on:** T01 (both touch `ports.rs`)

1. Update trait in `ports.rs`:
   ```rust
   fn diff_hunks(&self, from: &str, to: Option<&str>) -> Result<Vec<DiffHunk>>;
   ```
2. Update `MockGitProvider` in `test_support.rs`:
   ```rust
   fn diff_hunks(&self, _from: &str, _to: Option<&str>) -> Result<Vec<DiffHunk>> { Ok(vec![]) }
   ```
3. Update `ShellGitProvider` in `crates/cli/src/adapters/git.rs`: change `_to: &str` to `_to: Option<&str>` (keep `todo!()` for now — implementation in T05)
4. Fix any `diff_hunks` call sites (currently none outside tests — `ImpactUseCase::diff_impact` receives hunks, doesn't call `diff_hunks`)
5. `cargo build --workspace` compiles

### T03: Add `min_confidence` to `ImpactUseCase::diff_impact`
**AC coverage:** AC4
**Files:** `crates/domain/src/use_cases/impact.rs`

1. Write tests first:
   - `diff_impact` with `min_confidence: Confidence::High` filters affected nodes to High only
   - `diff_impact` with `min_confidence: Confidence::Structural` includes all (existing behavior)
2. Change signature:
   ```rust
   pub fn diff_impact(&self, hunks: &[DiffHunk], max_depth: usize, min_confidence: Confidence) -> Result<DiffImpactReport>
   ```
3. Replace hardcoded `Confidence::Structural` with `min_confidence`:
   ```rust
   let impact = compute_blast_radius(&graph, &targets, max_depth, min_confidence);
   ```
4. Update existing tests to pass `Confidence::Structural` as third arg
5. `cargo test -p domain` passes

---

## Wave 1 — Storage & Adapter Implementations

### T04: Implement `find_by_name` in `SqliteStore`
**AC coverage:** AC5
**Files:** `crates/storage/src/graph_store.rs`
**Depends on:** T01

1. Write tests first:
   - Insert symbols "foo", "foobar", "bar" then `find_by_name("foo")` returns `["foo"]` (exact only)
   - Insert symbols "foobar", "foobaz" then `find_by_name("foo")` returns `["foobar", "foobaz"]` (prefix fallback)
   - `find_by_name("nonexistent")` returns empty vec
   - Case sensitivity: `find_by_name("Foo")` with "foo" in store returns empty (SQL is case-sensitive by default)
2. Implement in `graph_store.rs`:
   ```rust
   fn find_by_name(&self, pattern: &str) -> Result<Vec<SymbolNode>> {
       let conn = self.conn()?;
       // Phase 1: exact match on name
       let exact = /* SELECT ... WHERE name = ?1 */;
       if !exact.is_empty() { return Ok(exact); }
       // Phase 2: prefix fallback
       let prefix_pattern = format!("{pattern}%");
       /* SELECT ... WHERE name LIKE ?1 */
   }
   ```
3. Reuse existing row-mapping logic from `get_symbol` / `all_symbols`
4. `cargo test -p storage` passes

### T05: Implement `diff_hunks` parser in `ShellGitProvider`
**AC coverage:** AC6, AC7
**Files:** `crates/cli/src/adapters/git.rs`
**Depends on:** T02

1. Write tests first (unit tests for hunk parsing, not requiring git):
   - Parse output with single-hunk add: `@@ -0,0 +1,5 @@` produces `DiffHunk { old_start: 0, old_count: 0, new_start: 1, new_count: 5 }`
   - Parse output with modify: `@@ -10,3 +10,5 @@` produces correct hunk
   - Parse output with delete: `@@ -5,3 +4,0 @@` produces correct hunk
   - Parse multi-file diff with multiple hunks per file
   - Parse rename: `diff --git a/old.rs b/new.rs` with `rename from`/`rename to` uses new file path
   - Empty diff output produces empty vec
   - Single-line hunk shorthand: `@@ -5 +5 @@` (no comma = count of 1)
2. Extract `parse_diff_output(output: &str) -> Result<Vec<DiffHunk>>` helper function (testable without git)
3. Implement `diff_hunks`:
   ```rust
   fn diff_hunks(&self, from: &str, to: Option<&str>) -> Result<Vec<DiffHunk>> {
       let mut args = vec!["diff", "--unified=0", from];
       if let Some(to_ref) = to {
           args.push(to_ref);
       }
       let output = self.run_git(&args)?;
       parse_diff_output(&output)
   }
   ```
4. Parse `diff --git a/X b/Y` headers to track current file path
5. Parse `@@ -old_start[,old_count] +new_start[,new_count] @@` lines into `DiffHunk` structs
6. Handle edge cases: count defaults to 1 when comma is absent; strip `b/` prefix from file paths
7. `cargo test -p cli` passes

### T06: Fix CLI args + extract `open_graph()` helper
**AC coverage:** AC14 (args), AC15 (args), AC17
**Files:** `crates/cli/src/commands/mod.rs`, `crates/cli/src/commands/helpers.rs`

1. Fix `ImpactArgs` in `commands/mod.rs`:
   ```rust
   pub struct ImpactArgs {
       /// Symbol name, qualified name, or file path to analyze
       pub target: String,         // renamed from qualified_name
       /// Maximum traversal depth
       #[arg(long, default_value = "3")]
       pub depth: usize,           // default changed from 5 to 3
       /// Minimum confidence level (high, medium, low, all)
       #[arg(long, default_value = "all")]
       pub confidence: String,
   }
   ```
2. Fix `DiffArgs` in `commands/mod.rs`:
   ```rust
   pub struct DiffArgs {
       /// Git ref to compare from (default: HEAD)
       #[arg(default_value = "HEAD")]
       pub from: String,
       /// Git ref to compare to (default: working tree)
       pub to: Option<String>,
       /// Maximum traversal depth
       #[arg(long, default_value = "3")]
       pub depth: usize,
       /// Minimum confidence level (high, medium, low, all)
       #[arg(long, default_value = "all")]
       pub confidence: String,
   }
   ```
3. Create `commands/helpers.rs`:
   ```rust
   use domain::error::{CodeGraphError, Result};
   use domain::model::Confidence;
   use storage::SqliteStore;
   use std::path::PathBuf;
   use crate::project::{find_project_root, ensure_data_dir};

   pub fn open_graph() -> Result<(SqliteStore, PathBuf)> {
       let cwd = std::env::current_dir().map_err(|e| {
           CodeGraphError::FileSystem { path: ".".into(), source: e }
       })?;
       let root = find_project_root(&cwd)?;
       let data_dir = ensure_data_dir(&root)?;
       let store = SqliteStore::open(&data_dir.join("graph.db"))
           .map_err(|e| CodeGraphError::Storage(format!("{e}")))?;
       Ok((store, root))
   }

   pub fn parse_confidence(s: &str) -> Result<Confidence> {
       match s {
           "high" => Ok(Confidence::High),
           "medium" => Ok(Confidence::Medium),
           "low" => Ok(Confidence::Low),
           "all" => Ok(Confidence::Structural),
           _ => Err(CodeGraphError::Other(format!("invalid confidence level: {s}"))),
       }
   }
   ```
4. Add `pub mod helpers;` to `commands/mod.rs`
5. Update existing test for impact args parsing (field rename)
6. `cargo build --workspace` compiles

### T07: `FindResult` struct + all `Displayable` implementations
**AC coverage:** AC8 (output), AC9 (output), AC10 (output), AC11 (output), AC12 (output), AC13 (output), AC14 (output), AC15 (output), AC16
**Files:** `crates/cli/src/output.rs`

1. Write tests first for each Displayable impl:
   - `Vec<FindResult>` compact: `Name kind file:lines [flags]\n  -> calls: ...\n  <- callers: ...`
   - `Vec<FindResult>` json: valid JSON array
   - `Vec<Reference>` compact: one line per ref `source_qualified_name (EdgeKind)`
   - `Vec<Reference>` json: valid JSON
   - `Vec<SearchResult>` compact: `qualified_name kind file:lines score=N.NN`
   - `Vec<SearchResult>` json: valid JSON
   - `GraphStats` compact: `Files: N | Symbols: N | Edges: N`
   - `GraphStats` json: valid JSON
   - `ImpactReport` compact: header + affected nodes grouped by confidence
   - `ImpactReport` json: valid JSON
   - `DiffImpactReport` compact: changed symbols + impact section
   - `DiffImpactReport` json: valid JSON
2. Define `FindResult` struct:
   ```rust
   #[derive(Debug, Clone, Serialize)]
   pub struct FindResult {
       pub symbol: SymbolNode,
       pub callers: Vec<String>,
       pub callees: Vec<String>,
       pub tested_by: Vec<String>,
   }
   ```
3. Implement `Displayable` for:
   - `Vec<FindResult>` (compact, table, json)
   - `Vec<Reference>` (compact, table, json) — used by refs, callers, callees
   - `Vec<SearchResult>` (compact, table, json)
   - `GraphStats` (compact, table, json)
   - `ImpactReport` (compact, table, json)
   - `DiffImpactReport` (compact, table, json)
4. Compact format rules (from SPEC):
   - `find`: `Name kind file:lines [flags]\n  -> calls: ...\n  -> tested_by: ...\n  <- callers: ...`
   - `refs/callers/callees`: `source_qualified_name (EdgeKind)` per line
   - `search`: `qualified_name kind file:lines score=N.NN` per line
   - `stats`: `Files: N | Symbols: N | Edges: N`
   - `impact`: header + one line per affected node grouped by confidence tier
   - `diff`: changed symbols section + impact section
5. JSON format: `serde_json::to_string_pretty` (requires Serialize derives — already present on domain types)
6. Table format: column-aligned with headers and separator
7. `cargo test -p cli` passes for output tests

---

## Wave 2 — Command Handlers

### T08: `find` handler with enrichment
**AC coverage:** AC8
**Files:** `crates/cli/src/commands/find.rs`
**Depends on:** T04, T06, T07

1. Write test:
   - Integration: create fixture with indexed symbols + edges, run `find` handler, assert output contains enriched symbol data
2. Implement `run_find(args: &FindArgs, output_format: OutputFormat) -> Result<()>`:
   - `open_graph()` to get `(store, root)`
   - `QueryUseCase::new(store.clone(), store.clone()).find(&args.pattern)?`
   - For each returned symbol, enrich with:
     - callers: `store.get_edges_to(qn)` filtered to `Calls`, extract source qualified names
     - callees: `store.get_edges_from(qn)` filtered to `Calls`, extract target qualified names
     - tested_by: `store.get_edges_to(qn)` filtered to `TestedBy`, extract source qualified names
   - Build `Vec<FindResult>`, print with output format
3. Add `pub mod find;` to `commands/mod.rs`

### T09: `refs` + `callers` + `callees` handlers
**AC coverage:** AC9, AC10, AC11
**Files:** `crates/cli/src/commands/refs.rs`, `crates/cli/src/commands/callers.rs`, `crates/cli/src/commands/callees.rs`
**Depends on:** T06, T07

1. Write tests per handler:
   - `refs`: indexed symbol with edges returns all incoming edges
   - `callers`: returns only `Calls` edges
   - `callees`: returns only outgoing `Calls` edges
2. All three follow the same pattern:
   ```rust
   pub fn run_refs(args: &RefsArgs, output_format: OutputFormat) -> Result<()> {
       let (store, _root) = open_graph()?;
       let uc = QueryUseCase::new(store.clone(), store);
       let refs = uc.refs(&args.qualified_name)?;
       print(&refs, output_format);
       Ok(())
   }
   ```
3. `callers` uses `uc.callers()`, `callees` uses `uc.callees()`
4. Add module declarations to `commands/mod.rs`

### T10: `search` + `stats` handlers
**AC coverage:** AC12, AC13
**Files:** `crates/cli/src/commands/search.rs`, `crates/cli/src/commands/stats.rs`
**Depends on:** T06, T07

1. Write tests:
   - `search`: indexed symbols, FTS returns results ordered by score
   - `stats`: indexed project returns correct counts
2. Implement `run_search`:
   ```rust
   pub fn run_search(args: &SearchArgs, output_format: OutputFormat) -> Result<()> {
       let (store, _root) = open_graph()?;
       let uc = QueryUseCase::new(store.clone(), store);
       let results = uc.search(&args.query, args.limit)?;
       print(&results, output_format);
       Ok(())
   }
   ```
3. Implement `run_stats`:
   ```rust
   pub fn run_stats(output_format: OutputFormat) -> Result<()> {
       let (store, _root) = open_graph()?;
       let uc = QueryUseCase::new(store.clone(), store);
       let stats = uc.stats()?;
       print(&stats, output_format);
       Ok(())
   }
   ```
4. Add module declarations to `commands/mod.rs`

### T11: `impact` handler with target disambiguation
**AC coverage:** AC14
**Files:** `crates/cli/src/commands/impact.rs`
**Depends on:** T06, T07

1. Write tests for disambiguation:
   - `"src/main.rs::foo"` (contains `::`) becomes `ImpactTarget::Symbol`
   - `"src/main.rs"` (contains `/`) becomes `ImpactTarget::File`
   - `"main.ts"` (known extension `.ts`) becomes `ImpactTarget::File`
   - `"MyClass"` (no `/`, no `::`, no ext) becomes `ImpactTarget::Symbol`
2. Implement `disambiguate_target(target: &str) -> ImpactTarget`:
   ```rust
   fn disambiguate_target(target: &str) -> ImpactTarget {
       if target.contains("::") {
           ImpactTarget::Symbol(target.to_string())
       } else if target.contains('/') || has_source_extension(target) {
           ImpactTarget::File(PathBuf::from(target))
       } else {
           ImpactTarget::Symbol(target.to_string())
       }
   }

   fn has_source_extension(s: &str) -> bool {
       [".ts", ".tsx", ".js", ".jsx", ".rs", ".py", ".go"]
           .iter()
           .any(|ext| s.ends_with(ext))
   }
   ```
3. Implement `run_impact`:
   ```rust
   pub fn run_impact(args: &ImpactArgs, output_format: OutputFormat) -> Result<()> {
       let (store, _root) = open_graph()?;
       let target = disambiguate_target(&args.target);
       let confidence = parse_confidence(&args.confidence)?;
       let uc = ImpactUseCase::new(store);
       let report = uc.blast_radius(&[target], args.depth, confidence)?;
       print(&report, output_format);
       Ok(())
   }
   ```
4. Add module declaration to `commands/mod.rs`

### T12: `diff` handler
**AC coverage:** AC15
**Files:** `crates/cli/src/commands/diff.rs`
**Depends on:** T05, T06, T07

1. Write test:
   - Integration: create git repo with fixture, make changes, run `diff` handler, assert output includes changed symbols + blast radius
2. Implement `run_diff`:
   ```rust
   pub fn run_diff(args: &DiffArgs, output_format: OutputFormat) -> Result<()> {
       let (store, root) = open_graph()?;
       let git = ShellGitProvider::new(root);
       let hunks = git.diff_hunks(&args.from, args.to.as_deref())?;
       let confidence = parse_confidence(&args.confidence)?;
       let uc = ImpactUseCase::new(store);
       let report = uc.diff_impact(&hunks, args.depth, confidence)?;
       print(&report, output_format);
       Ok(())
   }
   ```
3. Add module declaration to `commands/mod.rs`

---

## Wave 3 — Wire + Integration

### T13: Wire all handlers into `lib.rs` dispatcher + integration tests + clippy
**AC coverage:** AC16, AC17, AC18, AC19
**Files:** `crates/cli/src/lib.rs`, `crates/cli/src/commands/mod.rs`, integration tests
**Depends on:** T08, T09, T10, T11, T12

1. Update `lib.rs` dispatcher:
   ```rust
   Commands::Find(args) => commands::find::run_find(args, output_format),
   Commands::Refs(args) => commands::refs::run_refs(args, output_format),
   Commands::Callers(args) => commands::callers::run_callers(args, output_format),
   Commands::Callees(args) => commands::callees::run_callees(args, output_format),
   Commands::Search(args) => commands::search::run_search(args, output_format),
   Commands::Stats => commands::stats::run_stats(output_format),
   Commands::Impact(args) => commands::impact::run_impact(args, output_format),
   Commands::Diff(args) => commands::diff::run_diff(args, output_format),
   ```
2. Ensure all module declarations present in `commands/mod.rs`
3. Write integration tests:
   - Per command: create temp git repo with TS/Rust fixtures, index, run command, assert output correctness
   - `--json` flag produces valid parseable JSON for every command
   - Error cases: command on unindexed project, find with no matches, refs for nonexistent symbol
4. `cargo test --workspace` passes (AC18)
5. `cargo clippy --workspace -- -Dwarnings` passes (AC19)

---

## Task Dependency Graph

```
T01 (find_by_name + QueryUseCase) ──────┬──► T04 (SqliteStore find_by_name)
   │                                    │
   └──► T02 (diff_hunks signature) ─────┤──► T05 (diff_hunks parser)
                                        │
T03 (min_confidence param) ─────────────┤
                                        │
T06 (CLI args + open_graph helper) ◄────┤
                                        │
T07 (FindResult + Displayable impls) ◄──┘
   │
   ├──► T08 (find handler) ← T04, T06, T07
   ├──► T09 (refs/callers/callees) ← T06, T07
   ├──► T10 (search/stats) ← T06, T07
   ├──► T11 (impact handler) ← T06, T07
   └──► T12 (diff handler) ← T05, T06, T07
                │
                └──► T13 (wire + integration) ← T08-T12
```

## Wave Summary

| Wave | Tasks | Parallelism |
|------|-------|-------------|
| **0** | T01, T02, T03 | T01 + T03 parallel; T02 after T01 (shared ports.rs) |
| **1** | T04, T05, T06, T07 | All 4 parallel (after Wave 0) |
| **2** | T08, T09, T10, T11, T12 | All 5 parallel (after Wave 1) |
| **3** | T13 | Sequential (after Wave 2) |

## Complexity Estimate

| Task | Size | Notes |
|------|------|-------|
| T01 | S-M | Trait method + mock + use case rewrite, ~60 lines |
| T02 | S | Signature change across 3 files, ~15 lines |
| T03 | S | Add parameter, update calls, ~10 lines |
| T04 | M | SQL exact + prefix query, row mapping, ~80 lines |
| T05 | M-L | Git diff parser with hunk extraction, ~150 lines |
| T06 | S-M | Arg struct fixes + helper function, ~60 lines |
| T07 | L | 6 Displayable impls x 3 formats each, ~300 lines |
| T08 | M | Find handler with edge enrichment, ~80 lines |
| T09 | S-M | 3 nearly identical handlers, ~60 lines total |
| T10 | S | 2 simple handlers, ~40 lines total |
| T11 | M | Disambiguation heuristic + handler, ~70 lines |
| T12 | S-M | Diff handler wiring git + impact, ~50 lines |
| T13 | M | Dispatcher wiring + integration tests, ~200 lines |

**Total estimated:** ~1,175 lines of new/modified code + tests

## AC Traceability Matrix

| AC | Task | Verified By |
|----|------|-------------|
| AC1 | T01, T04 | Test: exact match, prefix fallback |
| AC2 | T01 | Test: find returns Vec<SymbolNode> |
| AC3 | T02 | Test: Option<&str> signature compiles |
| AC4 | T03 | Test: min_confidence filters affected nodes |
| AC5 | T04 | Test: SQL exact then prefix |
| AC6 | T05 | Test: parse git diff output into DiffHunks |
| AC7 | T05 | Test: None maps to working tree, Some maps to ref |
| AC8 | T07, T08 | Test: find output with enrichment |
| AC9 | T07, T09 | Test: refs output format |
| AC10 | T07, T09 | Test: callers output format |
| AC11 | T07, T09 | Test: callees output format |
| AC12 | T07, T10 | Test: search output format |
| AC13 | T07, T10 | Test: stats output format |
| AC14 | T06, T07, T11 | Test: impact disambiguation + output |
| AC15 | T05, T06, T07, T12 | Test: diff end-to-end |
| AC16 | T07, T13 | Test: --json produces valid JSON per command |
| AC17 | T06 | Test: open_graph used by all handlers |
| AC18 | T13 | `cargo test --workspace` |
| AC19 | T13 | `cargo clippy --workspace -- -Dwarnings` |
