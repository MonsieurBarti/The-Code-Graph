# Plan — M01-S05: CLI Foundation & Index Command

> For agentic workers: execute task-by-task with TDD.

**Goal:** Create `cli` + `binary` crates, wire all adapters to domain ports, implement the full indexing pipeline, and deliver a working `code-graph index` command that parses source files, resolves imports, and stores the graph to SQLite.

**Architecture:** Two new workspace crates (`cli`, `binary`). New `ParseProvider` outbound port in domain. Three adapters in cli: `RealFileSystem` (ignore crate), `ShellGitProvider` (shell-out), `RayonParseProvider` (wraps parser + resolver + rayon). Binary is the entry point.

**Tech Stack:** clap 4 (derive), tracing 0.1, tracing-subscriber 0.3, ignore 0.4, rayon 1.10, sha2 0.10, toml 0.8, serde_json 1

**Scope note:** Cross-file call resolution (Section 3.9) is deferred — parsers don't extract call sites. Graph after S05 has symbols, structural edges, and import edges. A new slice will add call extraction + resolution.

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `Cargo.toml` (workspace root) | Modify | Add cli + binary to members |
| `crates/domain/src/ports.rs` | Modify | Add `ParseProvider` trait + `FileData` struct |
| `crates/domain/src/use_cases/index.rs` | Modify | Add `ParseProvider` generic, implement `full_index()` |
| `crates/domain/src/test_support.rs` | Modify | Add `MockParseProvider` |
| `crates/cli/Cargo.toml` | Create | Dependencies: domain, parser, storage, clap, tracing, etc. |
| `crates/cli/src/lib.rs` | Create | Module declarations, `run()` entry point |
| `crates/cli/src/project.rs` | Create | Project root detection, blocklist, `.code-graph/` setup |
| `crates/cli/src/config.rs` | Create | `CodeGraphConfig` struct + TOML loader |
| `crates/cli/src/output.rs` | Create | `OutputFormat`, `Displayable` trait, `print()` |
| `crates/cli/src/logging.rs` | Create | tracing-subscriber init, verbosity handling |
| `crates/cli/src/commands/mod.rs` | Create | `Cli` struct, `Commands` enum, global flags |
| `crates/cli/src/commands/index.rs` | Create | `index` command handler |
| `crates/cli/src/commands/stubs.rs` | Create | Stub handlers for find/refs/impact/diff/callers/callees/search/stats/watch/setup/eval |
| `crates/cli/src/adapters/mod.rs` | Create | Module declarations |
| `crates/cli/src/adapters/fs.rs` | Create | `RealFileSystem` (ignore crate + sha2) |
| `crates/cli/src/adapters/git.rs` | Create | `ShellGitProvider` (Command::new("git")) |
| `crates/cli/src/adapters/parse.rs` | Create | `RayonParseProvider` (ParserRegistry + ResolverRegistry + rayon) |
| `crates/binary/Cargo.toml` | Create | Dependency: cli |
| `crates/binary/src/main.rs` | Create | Entry point, tracing init, error-to-exit-code |

---

## Acceptance Criteria

### Workspace
- AC1: `cargo build --workspace` succeeds with cli + binary crates
- AC2: Workspace has 5 members: domain, parser, storage, cli, binary

### Domain Port
- AC3: `ParseProvider` trait exists in `domain::ports` with `parse_and_resolve()` method
- AC4: `FileData` struct exists with `file: FileNode`, `symbols: Vec<SymbolNode>`, `edges: Vec<Edge>`
- AC5: `ParseProvider` is `Send + Sync` (compile-time check)

### IndexUseCase
- AC6: `IndexUseCase` takes 4 generics: `S: GraphStore`, `P: ParseProvider`, `F: FileSystem`, `G: GitProvider`
- AC7: `full_index(root)` lists files, calls `parse_and_resolve()`, stores results, returns `IndexStats`
- AC8: Parse failures for individual files are non-fatal — skipped with warning
- AC9: `IndexStats` reflects actual counts (files, symbols, edges, duration)

### Adapters
- AC10: `RealFileSystem` implements `FileSystem` — `list_files()` uses ignore crate with `.code-graphignore` support
- AC11: `RealFileSystem::file_hash()` returns SHA-256 hex string
- AC12: `ShellGitProvider` implements `GitProvider` — `current_head()` returns git HEAD hash
- AC13: `RayonParseProvider` implements `ParseProvider` — parallel parse (rayon) + import resolution
- AC14: `RayonParseProvider` produces `FileData` with structural edges + resolved import edges

### Project Infrastructure
- AC15: `find_project_root()` walks up to `.git`, returns error for blocklisted roots
- AC16: `ensure_data_dir()` creates `.code-graph/` directory
- AC17: `load_config()` parses `.code-graph/config.toml` or returns defaults
- AC18: Blocklisted roots (`/`, `/home`, `/Users`, `$HOME`) return `CodeGraphError::BlocklistedRoot`

### CLI
- AC19: `code-graph --help` shows all 12 subcommands
- AC20: `code-graph --version` prints version
- AC21: Global flags: `--verbose` / `-v` (count), `--debug`, `--json`, `--table`
- AC22: `code-graph index` runs full index and prints stats
- AC23: Unimplemented commands print "not implemented" message with exit code 1

### Output Formatting
- AC24: `OutputFormat` enum: Compact, Table, Json
- AC25: `Displayable` trait with `fmt_compact`, `fmt_table`, `fmt_json` methods
- AC26: `IndexStats` implements `Displayable` — compact prints summary line, JSON prints serialized stats

### Logging
- AC27: Default log level is WARN (quiet), `-v` → INFO, `--debug` → DEBUG
- AC28: `CODE_GRAPH_LOG` env var overrides log level
- AC29: Logs go to stderr, output goes to stdout

### Binary
- AC30: `code-graph index` exit code 0 on success
- AC31: `CodeGraphError::NoProject` → exit code 2
- AC32: `CodeGraphError::BlocklistedRoot` → exit code 2
- AC33: Other errors → exit code 1

### Integration
- AC34: `code-graph index` on a fixture project populates `.code-graph/graph.db` with symbols and edges
- AC35: `cargo test --workspace` passes
- AC36: `cargo clippy --workspace -- -Dwarnings` passes

---

## Wave 0 — Domain Port + Workspace Scaffold

### T01: Add `ParseProvider` port trait and `FileData` to domain
**AC coverage:** AC3, AC4, AC5
**Files:** `crates/domain/src/ports.rs`, `crates/domain/src/test_support.rs`

1. Write tests first:
   - `ParseProvider` is Send + Sync (compile-time assertion, same pattern as other ports)
   - `FileData` can be constructed with FileNode, symbols, edges
2. Add to `ports.rs`:
   ```rust
   pub struct FileData {
       pub file: FileNode,
       pub symbols: Vec<SymbolNode>,
       pub edges: Vec<Edge>,
   }

   pub trait ParseProvider: Send + Sync {
       fn parse_and_resolve(
           &self,
           files: &[(PathBuf, Vec<u8>)],
           project_root: &Path,
       ) -> Result<Vec<FileData>>;
   }
   ```
3. Add `MockParseProvider` to `test_support.rs` (returns canned FileData)
4. `cargo test -p domain` passes

### T02: Update `IndexUseCase` with `ParseProvider` generic
**AC coverage:** AC6
**Files:** `crates/domain/src/use_cases/index.rs`
**Depends on:** T01

1. Write tests first:
   - `IndexUseCase::new(store, parser, fs, git)` compiles
   - Construction with mock types succeeds
2. Update struct:
   ```rust
   pub struct IndexUseCase<S, P, F, G> {
       store: S,
       parser: P,
       fs: F,
       git: G,
   }
   ```
3. Update `new()` and trait bounds
4. Keep `full_index()` and `incremental_index()` as `todo!()` for now
5. Fix any compilation errors in domain tests

### T03: Create `cli` and `binary` crate scaffolds
**AC coverage:** AC1, AC2
**Files:** `Cargo.toml` (root), `crates/cli/Cargo.toml`, `crates/cli/src/lib.rs`, `crates/binary/Cargo.toml`, `crates/binary/src/main.rs`

1. Create `crates/cli/Cargo.toml` with all dependencies:
   ```toml
   [dependencies]
   domain = { path = "../domain" }
   parser = { path = "../parser" }
   storage = { path = "../storage" }
   clap = { version = "4", features = ["derive"] }
   tracing = "0.1"
   tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
   serde = { version = "1", features = ["derive"] }
   serde_json = "1"
   toml = "0.8"
   ignore = "0.4"
   rayon = "1.10"
   sha2 = "0.10"
   ```
2. Create `crates/cli/src/lib.rs` with module stubs
3. Create `crates/binary/Cargo.toml` depending on `cli`
4. Create `crates/binary/src/main.rs` with `fn main() {}`
5. Add `"crates/cli"`, `"crates/binary"` to workspace members
6. `cargo build --workspace` succeeds (AC1)

---

## Wave 1 — Project Infrastructure + Adapters (parallel — all independent)

### T04: Project root detection + blocklist + data dir
**AC coverage:** AC15, AC16, AC18
**Files:** `crates/cli/src/project.rs`
**Depends on:** T03

1. Write tests first:
   - Directory with `.git` → returns that directory
   - Nested directory walks up to `.git`
   - `/` → `BlocklistedRoot` error
   - `/Users` → `BlocklistedRoot` error
   - No `.git` found → `NoProject` error
   - `ensure_data_dir()` creates `.code-graph/` and returns path
2. Implement `find_project_root(start: &Path) -> Result<PathBuf>`
3. Implement `is_blocklisted(path: &Path) -> bool`
4. Implement `ensure_data_dir(root: &Path) -> Result<PathBuf>`

### T05: Config loading
**AC coverage:** AC17
**Files:** `crates/cli/src/config.rs`
**Depends on:** T03

1. Write tests first:
   - Missing config file → returns defaults (all `None`)
   - Valid config → parses `[index].exclude`, `[search].max_results`
   - Invalid TOML → returns error
2. Define `CodeGraphConfig`, `IndexConfig`, `SearchConfig` structs with serde
3. Implement `load_config(root: &Path) -> Result<CodeGraphConfig>`

### T06: `RealFileSystem` adapter
**AC coverage:** AC10, AC11
**Files:** `crates/cli/src/adapters/mod.rs`, `crates/cli/src/adapters/fs.rs`
**Depends on:** T03

1. Write tests first:
   - `list_files()` on tempdir with mixed extensions returns only matching files
   - `list_files()` respects `.gitignore` patterns
   - `list_files()` respects `.code-graphignore` patterns
   - `read_file()` returns file contents
   - `file_hash()` returns deterministic SHA-256 hex
2. Implement `RealFileSystem` with `ignore::WalkBuilder` for `list_files()`
3. Implement `read_file()` with `std::fs::read_to_string()`
4. Implement `file_hash()` with `sha2::Sha256`

### T07: `ShellGitProvider` adapter
**AC coverage:** AC12
**Files:** `crates/cli/src/adapters/git.rs`
**Depends on:** T03

1. Write tests first:
   - `current_head()` returns a 40-char hex string in a git repo
   - `changed_files()` and `diff_hunks()` return `todo!()` (implemented in S07)
2. Implement `ShellGitProvider` with `Command::new("git")`
3. Only `current_head()` fully implemented for S05

### T08: `RayonParseProvider` adapter
**AC coverage:** AC13, AC14
**Files:** `crates/cli/src/adapters/parse.rs`
**Depends on:** T03, T01

1. Write tests first:
   - Empty file list → empty result
   - Single TS file → returns FileData with symbols + Contains edges
   - Multiple files → returns FileData for each, with resolved import edges
   - Unsupported extension → file skipped (not error)
   - Parse error → file skipped, warning logged, others succeed (AC8)
2. Implement `RayonParseProvider`:
   - Holds `ParserRegistry` + `ResolverRegistry`
   - Phase 1: `files.par_iter()` → parallel parse via registry
   - Phase 2: build `ResolveContext` from all parse results
   - Phase 3: `par_iter()` → resolve imports per file via resolver registry
   - Phase 4: merge structural edges + resolved edges into `FileData`
3. Construct `FileNode` from path + language + hash

---

## Wave 2 — CLI Infrastructure (parallel — all independent)

### T09: Output formatting
**AC coverage:** AC24, AC25, AC26
**Files:** `crates/cli/src/output.rs`
**Depends on:** T03

1. Write tests first:
   - `OutputFormat::from_flags(true, false)` → Json
   - `OutputFormat::from_flags(false, true)` → Table
   - `OutputFormat::from_flags(false, false)` → Compact
   - `IndexStats` compact → "Indexed N files, N symbols, N edges in Xs"
   - `IndexStats` json → valid JSON with all fields
   - `IndexStats` table → formatted table with labels
2. Implement `OutputFormat` enum with `from_flags()`
3. Implement `Displayable` trait with `fmt_compact`, `fmt_table`, `fmt_json`
4. Implement `Displayable for IndexStats`
5. Implement `print()` helper: dispatches to format method, writes to stdout

### T10: Logging setup
**AC coverage:** AC27, AC28, AC29
**Files:** `crates/cli/src/logging.rs`
**Depends on:** T03

1. Write test:
   - `init_logging(0, false)` doesn't panic (basic smoke test)
   - Level mapping: 0 → warn, 1 → info, 2+ → debug, debug flag → debug
2. Implement `init_logging(verbose: u8, debug: bool)`:
   - Build level string from args
   - `EnvFilter::try_from_env("CODE_GRAPH_LOG")` with fallback
   - `tracing_subscriber::fmt()` to stderr, compact, no timestamps

### T11: Clap CLI definition + subcommand stubs
**AC coverage:** AC19, AC20, AC21, AC23
**Files:** `crates/cli/src/commands/mod.rs`, `crates/cli/src/commands/stubs.rs`
**Depends on:** T03

1. Write tests first:
   - `Cli::parse_from(["code-graph", "index"])` → Commands::Index
   - `Cli::parse_from(["code-graph", "find", "Foo"])` → Commands::Find with pattern "Foo"
   - `Cli::parse_from(["code-graph", "--json", "stats"])` → json=true
   - All 12 subcommands parse without error
2. Define `Cli` struct with global flags (`verbose`, `debug`, `json`, `table`)
3. Define `Commands` enum with all 12 variants
4. Define per-command arg structs: `IndexArgs`, `FindArgs`, `ImpactArgs`, etc.
5. Implement stub handlers that return `CodeGraphError::Other("not implemented...")`

---

## Wave 3 — Index Pipeline

### T12: Implement `IndexUseCase::full_index()`
**AC coverage:** AC7, AC8, AC9
**Files:** `crates/domain/src/use_cases/index.rs`
**Depends on:** T01, T02

1. Write tests first (using mocks):
   - `full_index` with mock ParseProvider returning 2 FileData → stores both, returns correct IndexStats
   - `full_index` with empty file list → IndexStats all zeros
   - `full_index` with mock FileSystem listing 3 files → reads all 3
   - Duration is non-zero
2. Implement `full_index(root: &Path) -> Result<IndexStats>`:
   - Get supported extensions from a constant or parameter
   - `fs.list_files(root, extensions)` → file paths
   - Read each file: `fs.read_file(path)` → content bytes
   - `parser.parse_and_resolve(files, root)` → Vec<FileData>
   - For each FileData: `store.store_file_data(file, symbols, edges)`
   - Count totals, measure duration, return `IndexStats`

### T13: `index` command handler
**AC coverage:** AC22, AC34
**Files:** `crates/cli/src/commands/index.rs`, `crates/cli/src/lib.rs`
**Depends on:** T04, T05, T06, T07, T08, T09, T11, T12

1. Write test:
   - Integration test: create tempdir with a TS file, run `index` handler, verify `.code-graph/graph.db` exists and has symbols
2. Implement `run_index(args, output_format) -> Result<()>`:
   - `find_project_root()` (or use `--path` override)
   - `ensure_data_dir(root)`
   - `load_config(root)`
   - `SqliteStore::open(root.join(".code-graph/graph.db"))`
   - Construct `RealFileSystem`, `ShellGitProvider`, `RayonParseProvider`
   - `IndexUseCase::new(store, parser, fs, git).full_index(root)`
   - `print(stats, output_format)`
3. Wire into `Commands::Index` dispatch in `lib.rs`

---

## Wave 4 — Binary + Integration

### T14: Binary entry point
**AC coverage:** AC30, AC31, AC32, AC33
**Files:** `crates/binary/src/main.rs`
**Depends on:** T10, T11, T13

1. Implement `main()`:
   - `Cli::parse()`
   - `init_logging(cli.verbose, cli.debug)`
   - Determine `OutputFormat` from flags
   - Match on `cli.command`, dispatch to handlers
   - Map `Result` to exit code: Ok → 0, NoProject/BlocklistedRoot → 2, other → 1
2. Test via `cargo run -p binary -- index --help` (manual smoke test)

### T15: Integration tests + clippy + final verification
**AC coverage:** AC34, AC35, AC36
**Files:** `crates/cli/tests/integration.rs` (or inline)
**Depends on:** T14

1. Write integration tests:
   - Create tempdir with fixture files (TS, Rust, Python, Go)
   - Initialize git repo (`git init`)
   - Run index handler programmatically
   - Open resulting `.code-graph/graph.db` via SqliteStore
   - Verify: files, symbols, and edges exist
   - Verify: ImportsFrom edges exist for files with imports
   - Verify: IndexStats counts match store.stats()
2. `cargo test --workspace` passes (AC35)
3. `cargo clippy --workspace -- -Dwarnings` passes (AC36)
4. `cargo build --workspace` succeeds

---

## Task Dependency Graph

```
T01 (ParseProvider port) ──┬──► T02 (IndexUseCase update) ──► T12 (full_index impl)
                           │                                         │
T03 (crate scaffolds) ─────┤                                         │
   │                       │                                         │
   ├──► T04 (project root) ┤                                         │
   ├──► T05 (config)       ├─────────────────────────────────────────┤
   ├──► T06 (RealFileSystem)                                         │
   ├──► T07 (ShellGitProvider)                                       │
   ├──► T08 (RayonParseProvider) ← T01                               │
   │                                                                 │
   ├──► T09 (output formatting)                                      │
   ├──► T10 (logging)                                                │
   ├──► T11 (clap CLI defs)                                          │
   │                                                                 │
   │                           T13 (index handler) ← T04-T12 ───────┤
   │                                                                 │
   │                           T14 (binary main) ← T10, T11, T13    │
   │                                                                 │
   └───────────────────────────T15 (integration) ← T14 ─────────────┘
```

## Wave Summary

| Wave | Tasks | Parallelism |
|------|-------|-------------|
| **0** | T01, T02, T03 | T01+T03 parallel, T02 after T01 |
| **1** | T04, T05, T06, T07, T08 | All 5 parallel (after T01+T03) |
| **2** | T09, T10, T11 | All 3 parallel (after T03) |
| **3** | T12, T13 | T12 after T01+T02; T13 after T04-T12 |
| **4** | T14, T15 | Sequential: T14 then T15 |

## Complexity Estimate

| Task | Size | Notes |
|------|------|-------|
| T01 | S | Port trait + struct, ~30 lines |
| T02 | S | Add generic, update constructor, ~20 lines |
| T03 | S | Cargo.toml + empty modules, ~50 lines |
| T04 | S-M | Project detection + blocklist, ~80 lines |
| T05 | S | Config struct + TOML loader, ~60 lines |
| T06 | M | ignore crate walker + sha2 hashing, ~120 lines |
| T07 | S | Shell-out to git, ~60 lines |
| T08 | L | Rayon parallel parse + resolve + merge, ~200 lines |
| T09 | M | OutputFormat + Displayable + IndexStats impl, ~120 lines |
| T10 | S | tracing-subscriber init, ~40 lines |
| T11 | M | All 12 subcommands + arg structs + stubs, ~200 lines |
| T12 | M | full_index orchestration with mocks, ~100 lines |
| T13 | M | Wiring adapters → use case → output, ~100 lines |
| T14 | S | main() + exit codes, ~50 lines |
| T15 | M | Integration tests with fixtures, ~150 lines |

**Total estimated:** ~1,380 lines of new code + tests across 2 new crates + domain modifications

## AC Traceability Matrix

| AC | Task | Verified By |
|----|------|-------------|
| AC1 | T03 | `cargo build --workspace` |
| AC2 | T03 | Workspace Cargo.toml has 5 members |
| AC3 | T01 | Test: ParseProvider trait exists |
| AC4 | T01 | Test: FileData construction |
| AC5 | T01 | Compile-time Send+Sync assertion |
| AC6 | T02 | Test: IndexUseCase with 4 generics |
| AC7 | T12 | Test: full_index returns correct IndexStats |
| AC8 | T08, T12 | Test: parse failure → file skipped |
| AC9 | T12 | Test: IndexStats counts match |
| AC10 | T06 | Test: list_files with ignore patterns |
| AC11 | T06 | Test: file_hash SHA-256 |
| AC12 | T07 | Test: current_head returns hash |
| AC13 | T08 | Test: RayonParseProvider returns FileData |
| AC14 | T08 | Test: FileData has structural + import edges |
| AC15 | T04 | Test: walks up to .git |
| AC16 | T04 | Test: creates .code-graph/ dir |
| AC17 | T05 | Test: config loading + defaults |
| AC18 | T04 | Test: blocklisted roots |
| AC19 | T11 | Test: --help shows 12 commands |
| AC20 | T11 | Test: --version prints version |
| AC21 | T11 | Test: global flags parse |
| AC22 | T13 | Test: index handler runs end-to-end |
| AC23 | T11 | Test: stub commands return error |
| AC24 | T09 | Test: OutputFormat enum |
| AC25 | T09 | Test: Displayable trait |
| AC26 | T09 | Test: IndexStats formatting |
| AC27 | T10 | Test: verbosity level mapping |
| AC28 | T10 | Test: CODE_GRAPH_LOG override |
| AC29 | T10 | Test: logs to stderr |
| AC30 | T14 | Manual: exit code 0 |
| AC31 | T14 | Test: NoProject → exit 2 |
| AC32 | T14 | Test: BlocklistedRoot → exit 2 |
| AC33 | T14 | Test: other error → exit 1 |
| AC34 | T15 | Integration: fixture index → populated DB |
| AC35 | T15 | `cargo test --workspace` |
| AC36 | T15 | `cargo clippy --workspace -- -Dwarnings` |
