# The Code Graph — Design Specification

## 1. Vision

A high-performance Rust CLI tool that indexes codebases into a queryable dependency graph, reducing AI coding agent token consumption by providing structural context instead of raw file reads. Rebuild of [code-review-graph](https://github.com/tirth8205/code-review-graph) with Rust performance, cleaner architecture, better search quality, better blast radius precision, and superior import resolution.

**Target platforms:** Claude Code (via hooks), PI (via bash), any agent with shell access.

---

## 2. Architecture

### 2.1 Workspace-Based Hexagonal (Ports & Adapters)

Six Cargo workspace crates with compile-time boundary enforcement:

```
the-code-graph/
  Cargo.toml                        # workspace root
  crates/
    domain/                         # pure business logic, zero external deps (only serde derive)
    parser/                         # tree-sitter parsing + import resolution
    storage/                        # SQLite + FTS5 (implements outbound ports)
    watch/                          # file watcher, daemon, incremental updates
    cli/                            # clap commands, output formatting, hooks, adapter wiring
    binary/                         # entry point (main.rs)
```

**Dependency graph:**

```
binary → cli → domain
              → parser  → domain
              → storage → domain
              → watch   → domain
                        → parser
                        → storage
```

- `domain` depends on nothing (only `serde` with `derive` feature — no format-specific serializers)
- `parser` cannot import `rusqlite` — not in its Cargo.toml
- `storage` cannot import `tree-sitter` — not in its Cargo.toml
- `cli` is the orchestration layer: directly depends on `parser`, `storage`, and `watch` to wire adapters to domain ports
- Hexagonal boundaries enforced by the compiler, not discipline

### 2.2 Key Dependencies

| Crate | Key Dependencies |
|-------|-----------------|
| `domain` | `serde` (derive only) |
| `parser` | `tree-sitter`, `tree-sitter-typescript`, `tree-sitter-javascript`, `tree-sitter-rust`, `tree-sitter-python`, `tree-sitter-go`, `oxc_resolver`, `rayon` |
| `storage` | `rusqlite` (bundled), `r2d2` |
| `watch` | `notify`, `notify-debouncer-mini` |
| `cli` | `clap` v4, `serde_json`, `tracing`, `tracing-subscriber` |
| `binary` | `cli` |

### 2.3 Error Model

A unified error hierarchy propagated across crate boundaries:

```rust
// domain/src/error.rs — the root error type
#[derive(Debug, thiserror::Error)]
enum CodeGraphError {
    #[error("parse error in {file}: {message}")]
    Parse { file: PathBuf, message: String },

    #[error("resolution error: {0}")]
    Resolution(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("git error: {0}")]
    Git(String),

    #[error("file system error: {path}: {source}")]
    FileSystem { path: PathBuf, source: std::io::Error },

    #[error("no project found (no .git directory)")]
    NoProject,

    #[error("refused to index blocklisted root: {0}")]
    BlocklistedRoot(PathBuf),

    #[error("index not built — run `code-graph index` first")]
    IndexNotBuilt,

    #[error("{0}")]
    Other(String),
}
```

- All port traits return `Result<T, CodeGraphError>`
- Adapter crates convert their internal errors (e.g., `rusqlite::Error`) into `CodeGraphError` variants at the boundary
- CLI maps errors to exit codes: `0` = success, `1` = runtime error, `2` = no project found, `3` = usage error
- Parse failures for individual files are non-fatal — the file is skipped, a warning is logged, and the previous graph state for that file is retained

### 2.4 Logging and Observability

Uses `tracing` crate for structured logging:

- **CLI default:** `WARN` level (quiet)
- **`--verbose` / `-v`:** `INFO` level (progress output)
- **`--debug`:** `DEBUG` level (detailed diagnostics)
- **`CODE_GRAPH_LOG` env var:** override log level (e.g., `CODE_GRAPH_LOG=trace`)
- **Daemon mode:** logs to `.code-graph/daemon.log` with daily rotation, `INFO` level default
- **Hook scripts:** silent (stderr suppressed) to avoid polluting agent context

### 2.5 Configuration

```toml
# .code-graph/config.toml (optional, per-project)

[index]
exclude = ["vendor/**", "generated/**"]   # additional ignore patterns beyond .gitignore

[watch]
debounce_ms = 100                          # file watcher debounce

[search]
max_results = 20                           # default search limit
```

- Configuration is optional — sensible defaults for everything
- Project-level config at `.code-graph/config.toml`
- No global config (each project is independent)
- `.code-graphignore` file supported (gitignore syntax) as an alternative to `config.toml [index] exclude`
- Language-specific resolver settings (tsconfig location, go.mod path) are auto-detected, not configured

---

## 3. Domain Model

### 3.1 Node Types

Three node kinds representing different levels of code understanding:

```rust
enum NodeKind {
    File,      // source file (parsed with tree-sitter)
    Symbol,    // function, class, interface, etc. extracted from parsed files
    NonParsed, // non-source files (docs, configs, assets) — tracked but not deeply parsed
}

struct FileNode {
    path: PathBuf,
    language: Language,
    hash: String,           // SHA-256 for change detection
}

struct SymbolNode {
    name: String,
    qualified_name: String, // see Section 3.5 for formal grammar
    kind: SymbolKind,       // Function, Class, Interface, Struct, Trait, Enum, TypeAlias,
                            // Method, Property, Const, Macro, Variable, Component, Test
    location: Location,     // file, line_start, line_end, col_start, col_end
    visibility: Visibility, // see Section 3.6
    is_exported: bool,
    is_async: bool,
    is_test: bool,
    decorators: Vec<String>,
    signature: Option<String>,
}

struct NonParsedNode {
    path: PathBuf,
    file_kind: NonParsedKind, // Doc, Config, CI, Asset, Other
    hash: String,
}
```

`NonParsedNode` represents files that are tracked in the graph for completeness (e.g., README.md, .github/workflows/*.yml, assets/) but are not parsed by tree-sitter. They participate in `DependsOn` edges only (file-level dependencies detected by import specifiers referencing non-source files). They carry no symbols and no outgoing edges other than file-level relationships.

### 3.2 Edge Types (16)

```rust
enum EdgeKind {
    Contains,           // File -> Symbol
    ChildOf,            // Symbol -> Symbol (nesting)
    Calls,              // Symbol -> Symbol
    ImportsFrom,        // File -> File (resolved)
    Extends,            // Symbol -> Symbol
    Implements,         // Symbol -> Symbol
    TestedBy,           // Symbol -> Symbol (test)
    DependsOn,          // File -> File (dependency)
    BarrelReExportAll,  // File -> File (export * from)
    ConditionalImport,  // Python TYPE_CHECKING, try/except
    SideEffectImport,   // Go blank import
    DotImport,          // Go dot import (imported names enter local scope)
    HasDecorator,       // Symbol -> decorator
    Embeds,             // Go struct embedding
    TypeReference,      // Symbol -> Symbol (type usage)
    ReExport,           // Rust pub use
}
```

### 3.3 Outbound Port Traits

Defined in domain, implemented by adapter crates:

```rust
trait GraphStore {
    fn store_file(&self, file: &FileNode, symbols: &[SymbolNode], edges: &[Edge]) -> Result<()>;
    fn remove_file(&self, path: &Path) -> Result<()>;
    fn get_file_hash(&self, path: &Path) -> Result<Option<String>>;
    fn get_node(&self, qualified_name: &str) -> Result<Option<Node>>;
    fn get_edges_from(&self, qualified_name: &str) -> Result<Vec<Edge>>;
    fn get_edges_to(&self, qualified_name: &str) -> Result<Vec<Edge>>;
    fn get_all_edges(&self) -> Result<Vec<Edge>>;
    fn get_nodes_in_file(&self, path: &Path) -> Result<Vec<Node>>;
    fn get_nodes_by_kind(&self, kind: SymbolKind) -> Result<Vec<Node>>;
    fn get_all_files(&self) -> Result<Vec<FileNode>>;
}

trait SearchIndex {
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;
    fn index_node(&self, node: &Node) -> Result<()>;
    fn remove_node(&self, qualified_name: &str) -> Result<()>;
}

// Implemented in cli crate (thin wrappers around git CLI and std::fs)
trait GitProvider {
    fn changed_files(&self, base: Option<&str>) -> Result<Vec<PathBuf>>;
    fn diff_hunks(&self, base: Option<&str>) -> Result<Vec<DiffHunk>>;
    fn ls_files(&self) -> Result<Vec<PathBuf>>;
}

// Implemented in cli crate (thin wrapper around std::fs + ignore crate)
trait FileSystem {
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;
    fn file_hash(&self, path: &Path) -> Result<String>;
    fn walk_files(&self, root: &Path, ignores: &[Pattern]) -> Result<Vec<PathBuf>>;
}
```

`GraphStore` and `SearchIndex` are implemented in the `storage` crate.
`GitProvider` and `FileSystem` are implemented in the `cli` crate as thin wrappers.

### 3.4 Inbound Ports (Use Cases)

The domain exposes use-case structs that driving adapters (CLI) call into. Each use case takes outbound ports as constructor parameters:

```rust
struct IndexUseCase<S: GraphStore, F: FileSystem, G: GitProvider> { ... }
impl IndexUseCase {
    fn full_index(&self, root: &Path) -> Result<IndexStats>;
    fn incremental_index(&self, root: &Path, files: Option<&[PathBuf]>) -> Result<IndexStats>;
}

struct QueryUseCase<S: GraphStore> { ... }
impl QueryUseCase {
    fn find(&self, pattern: &str) -> Result<Vec<Node>>;
    fn refs(&self, qualified_name: &str) -> Result<Vec<Reference>>;
    fn callers(&self, qualified_name: &str) -> Result<Vec<Node>>;
    fn callees(&self, qualified_name: &str) -> Result<Vec<Node>>;
    fn search(&self, query: &str, limit: usize, context_file: Option<&Path>) -> Result<Vec<SearchResult>>;
    fn stats(&self) -> Result<GraphStats>;
}

struct ImpactUseCase<S: GraphStore> { ... }
impl ImpactUseCase {
    fn blast_radius(&self, targets: &[ImpactTarget], depth: usize, min_confidence: Confidence) -> Result<ImpactReport>;
    fn diff_impact(&self, hunks: &[DiffHunk], depth: usize) -> Result<DiffImpactReport>;
}
```

`ImpactTarget` can be a file path or a symbol qualified name:
```rust
enum ImpactTarget {
    File(PathBuf),      // all symbols in the file
    Symbol(String),     // specific qualified_name
}
```

### 3.5 Qualified Name Format

Formal grammar for the load-bearing `qualified_name` primary key:

```
qualified_name := file_path "::" symbol_path
symbol_path    := segment ("." segment)*
segment        := identifier

# Examples by language:

# TypeScript/JavaScript
src/services/user.ts::UserService
src/services/user.ts::UserService.create
src/services/user.ts::handleRequest            # top-level function
src/services/user.ts::default                  # default export (anonymous)

# Rust
src/auth/mod.rs::authenticate
src/auth/mod.rs::AuthService
src/auth/mod.rs::AuthService.validate          # impl method

# Python
src/services/user.py::UserService
src/services/user.py::UserService.__init__
src/services/user.py::create_user              # module-level function

# Go
internal/auth/handler.go::AuthHandler
internal/auth/handler.go::AuthHandler.Validate  # method with receiver
internal/auth/handler.go::NewAuthHandler        # package-level function
```

Rules:
- File path is relative to project root
- `::` separates file path from symbol path
- `.` separates nesting levels (class.method, interface.property)
- Anonymous/default exports use `default` as the segment name
- Go receiver methods: `ReceiverType.MethodName`
- Duplicate names in same scope get numeric suffix: `handler.1`, `handler.2`

### 3.6 Visibility Mapping

```rust
enum Visibility {
    Public,   // explicitly public
    Private,  // explicitly private or default-private
    Crate,    // Rust pub(crate)
}
```

| Language | Public | Private | Crate |
|----------|--------|---------|-------|
| TypeScript/JS | `export` keyword | no `export` | N/A |
| Rust | `pub` | default (no modifier) | `pub(crate)` |
| Python | no `_` prefix | `_` or `__` prefix | N/A |
| Go | capitalized name | lowercase name | N/A |

### 3.7 Analysis (Pure Domain Logic)

Analysis modules live in the `domain` crate and operate on the `InMemoryGraph` struct (also in domain):

```rust
// domain/src/traversal.rs
struct InMemoryGraph {
    outgoing: HashMap<String, Vec<(String, EdgeKind)>>,
    incoming: HashMap<String, Vec<(String, EdgeKind)>>,
}

impl InMemoryGraph {
    fn from_edges(edges: Vec<Edge>) -> Self;  // constructed by use cases that load edges via GraphStore
    fn bfs(&self, start: &str, direction: Direction, max_depth: usize) -> Vec<TraversalResult>;
    fn dfs(&self, start: &str, direction: Direction) -> Vec<TraversalResult>;
}
```

- **BlastRadius** — BFS through edges, configurable depth, confidence tiers
- **ChangeDetection** — git diff hunks mapped to overlapping graph nodes
- **Impact** — transitive impact from changed nodes
- **Traversal** — callers, callees, refs, dependents

### 3.8 Confidence Tiers

All 16 edge types classified for blast radius analysis:

| Confidence | Edge Types | Rationale |
|------------|-----------|-----------|
| **High** | `Calls`, `Extends`, `Implements`, `Embeds` | Direct behavioral coupling — changes propagate with near certainty |
| **Medium** | `ImportsFrom`, `BarrelReExportAll`, `ReExport`, `TypeReference`, `DotImport` | Structural coupling — changes likely propagate but may not affect behavior |
| **Low** | `DependsOn`, `ConditionalImport`, `SideEffectImport` | Weak coupling — changes may propagate in edge cases |
| **None** (excluded) | `Contains`, `ChildOf`, `HasDecorator`, `TestedBy` | Structural/metadata edges — not traversed during impact analysis |

### 3.9 Cross-File Call Resolution Strategy

The hardest problem in the system. Our approach:

1. **Scoped resolution (primary):** When resolving `foo()` in file A, first check symbols imported into A (via `ImportsFrom` edges). If exactly one `foo` exists in imported scope → create `Calls` edge. This is high confidence.

2. **Qualified resolution:** If the call site uses a qualified name (`auth.validate()`, `self.validate()`, `pkg.Validate()`), match against the qualifier's type. This handles most method calls.

3. **Single-candidate fallback:** If scoped resolution fails and exactly one `foo` exists in the entire graph → create `Calls` edge with a `low_confidence` metadata flag. This is the same strategy as code-graph-ai but we mark the confidence explicitly.

4. **Ambiguous → no edge:** If multiple candidates exist and we can't disambiguate → no `Calls` edge is created. We prefer missing edges over wrong edges. The symbol still appears in search results.

This is a v0.1 strategy. Type inference (v0.2+) would improve resolution for overloaded methods and dynamic dispatch.

---

## 4. Parser

### 4.1 Structure

```
crates/parser/src/
    lib.rs              # ParserRegistry
    registry.rs         # register_all(), get_parser_for_file()
    typescript.rs       # TypeScript + TSX + JavaScript + JSX
    rust_lang.rs        # Rust
    python.rs           # Python
    go.rs               # Go
    resolver/
      mod.rs            # ImportResolver trait + resolve_all() pipeline
      typescript.rs     # oxc_resolver for file resolution + custom barrel chain traversal
      rust_lang.rs      # crate-root module tree walk, Cargo workspace
      python.rs         # package resolution, __init__.py
      go.rs             # go.mod module resolution
    test_utils.rs
```

### 4.2 Core Trait

```rust
trait LanguageParser: Send + Sync {
    fn language(&self) -> Language;
    fn parse(&self, source: &[u8], path: &Path) -> Result<ParseResult>;
    fn file_extensions(&self) -> &[&str];
}

struct ParseResult {
    symbols: Vec<SymbolNode>,
    edges: Vec<Edge>,
    imports: Vec<RawImport>,
    exports: Vec<Export>,
}

struct RawImport {
    specifier: String,
    names: Vec<ImportName>,
    is_type_only: bool,
    is_side_effect: bool,
    line: usize,
}
```

### 4.3 Key Design Decisions

- **Thread-local parsers** — `thread_local!` with `RefCell<Parser>` for rayon worker threads. Zero lock contention during parallel indexing.
- **Two-phase parse then resolve** — Parsing extracts `RawImport` (unresolved). Resolution is a separate step with cross-file context.
- **TS/JS resolution: oxc_resolver + custom barrel traversal** — `oxc_resolver` handles file-level resolution (given specifier → file path) including tsconfig paths and workspace aliases. Barrel re-export chain traversal (`export * from`) is a separate multi-pass step that parses barrel files to trace symbol origins. This goes beyond what `oxc_resolver` provides.
- **Graceful parse failure** — If tree-sitter fails on a file (syntax errors, unsupported constructs), the file is skipped with a warning log. Previous graph state for that file is retained. The overall index operation continues.
- **Extensibility** — Adding a language: create `new_lang.rs` implementing `LanguageParser`, register in `registry.rs`. No changes to any other crate.

---

## 5. Storage

### 5.1 Database Location

- Database at `.code-graph/graph.db` relative to the project root (the directory containing `.git`)
- `.code-graph/` directory is auto-created on first `index` command
- `.code-graph/` should be added to `.gitignore` (the `setup` command does this automatically)
- Contents of `.code-graph/`: `graph.db`, `config.toml` (optional), `daemon.pid`, `daemon.sock`, `daemon.log`

### 5.2 SQLite Schema

```sql
-- Pragma settings (applied on every connection open)
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;

-- Schema version tracking
CREATE TABLE metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE files (
    path TEXT PRIMARY KEY,
    language TEXT NOT NULL,
    hash TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE non_parsed_files (
    path TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    hash TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE symbols (
    qualified_name TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    file_path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    col_start INTEGER NOT NULL,
    col_end INTEGER NOT NULL,
    visibility TEXT NOT NULL DEFAULT 'private',
    is_exported INTEGER NOT NULL DEFAULT 0,
    is_async INTEGER NOT NULL DEFAULT 0,
    is_test INTEGER NOT NULL DEFAULT 0,
    decorators TEXT,
    signature TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    source_qualified TEXT NOT NULL,
    target_qualified TEXT NOT NULL,
    metadata TEXT,
    UNIQUE(kind, source_qualified, target_qualified)
);

-- Full-text search
CREATE VIRTUAL TABLE symbols_fts USING fts5(
    name, qualified_name, file_path, signature,
    content='symbols', content_rowid='rowid'
);

-- FTS5 sync triggers
CREATE TRIGGER symbols_ai AFTER INSERT ON symbols BEGIN
    INSERT INTO symbols_fts(rowid, name, qualified_name, file_path, signature)
    VALUES (new.rowid, new.name, new.qualified_name, new.file_path, new.signature);
END;

CREATE TRIGGER symbols_ad AFTER DELETE ON symbols BEGIN
    INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, file_path, signature)
    VALUES ('delete', old.rowid, old.name, old.qualified_name, old.file_path, old.signature);
END;

CREATE TRIGGER symbols_au AFTER UPDATE ON symbols BEGIN
    INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, file_path, signature)
    VALUES ('delete', old.rowid, old.name, old.qualified_name, old.file_path, old.signature);
    INSERT INTO symbols_fts(rowid, name, qualified_name, file_path, signature)
    VALUES (new.rowid, new.name, new.qualified_name, new.file_path, new.signature);
END;

-- Indexes
CREATE INDEX idx_symbols_file ON symbols(file_path);
CREATE INDEX idx_symbols_kind ON symbols(kind);
CREATE INDEX idx_symbols_name ON symbols(name);
CREATE INDEX idx_edges_source ON edges(source_qualified);
CREATE INDEX idx_edges_target ON edges(target_qualified);
CREATE INDEX idx_edges_kind ON edges(kind);
```

### 5.3 Key Design Decisions

- **Separate `files`, `non_parsed_files`, and `symbols` tables** — cleaner than code-review-graph's single polymorphic `nodes` table
- **`ON DELETE CASCADE`** on symbols → files FK — removing a file automatically removes its symbols
- **WAL mode + 5s busy timeout** — concurrent reads, serialized writes
- **FTS5 triggers defined in schema** — auto-sync on insert/update/delete
- **Schema migrations** — versioned via `metadata` table, idempotent, forward-only, run automatically on store open
- **Search improvements over code-review-graph (MRR 0.35):**
  - BM25 with custom column weights (name > signature > qualified_name > file_path)
  - Query-aware boosting (PascalCase -> Class/Interface, snake_case -> Function)
  - Trigram similarity fallback for fuzzy matches
  - Context-file boosting

---

## 6. Watch + Incremental Updates

### 6.1 Incremental Pipeline

```
File change detected
  -> Is file parseable? (no -> skip)
  -> Compute SHA-256 hash
  -> Hash matches stored? (yes -> skip)
  -> Re-parse changed file (on failure: skip, retain previous state, log warning)
  -> Find dependents (1 hop: files that import/call into changed file)
  -> Re-parse dependents
  -> Remove stale nodes/edges
  -> Store new nodes/edges
  -> Update FTS5 index (automatic via triggers)
```

### 6.2 Three-Layer Freshness

1. **Daemon running** -> graph is always fresh, queries are instant
2. **No daemon, Claude Code hooks** -> graph updates on agent actions
3. **Neither** -> lazy staleness check on query

### 6.3 Lazy Staleness Check

To avoid scanning all tracked files on every query:

1. First, run `git status --porcelain` to get the list of modified/added/deleted files (fast, O(changed files) not O(all files))
2. Compare only those files against stored hashes
3. If any are stale, auto-update just those files + their dependents

This bounds the check to git-tracked changes, not a full filesystem scan. For a typical query after a few file edits, this adds ~50-100ms.

### 6.4 Daemon

- `code-graph watch` — foreground, logs to stderr
- `code-graph watch --daemon` — background, logs to `.code-graph/daemon.log`
- `code-graph watch --status` — is daemon running?
- `code-graph watch --stop` — stop daemon
- PID file at `.code-graph/daemon.pid`
- Unix socket at `.code-graph/daemon.sock` for health checks
- `notify` crate with 100ms debounce (configurable via `config.toml`)
- Respects `.gitignore` and `.code-graphignore`

### 6.5 Project Detection

- **Only `.git`** is a valid project marker
- Walk up from cwd looking for `.git` directory
- **Blocklisted roots:** `/`, `/home`, `/Users`, `/Users/<name>`, `$HOME`
- No `.git` found -> refuse to index with clear message and exit code 2
- Override: `code-graph index --project ./my-app`

### 6.6 Claude Code Hooks

- **SessionStart:** `code-graph index --incremental` (silently exits 0 if no project detected)
- **PostToolUse (Write/Edit):** `code-graph index --incremental --files $CHANGED_FILE`
- **PreCommit:** `code-graph index --incremental`

### 6.7 `setup` Command

```bash
code-graph setup claude    # install Claude Code hooks
code-graph setup --check   # verify hooks are installed and working
code-graph setup --remove  # uninstall all code-graph hooks
```

**Install (`setup claude`):**
1. Reads `~/.claude/settings.json` (creates if not exists)
2. Adds three hook entries (SessionStart, PostToolUse, PreCommit) to the `hooks` array
3. Each hook is tagged with `"source": "code-graph"` in its metadata so it can be identified for removal
4. Adds `.code-graph/` to the project's `.gitignore` (if not already present)
5. Is idempotent — running twice does not duplicate hooks
6. Preserves existing hooks and settings

**Verify (`setup --check`):**
1. Checks `~/.claude/settings.json` for all three hooks (by `source` tag)
2. Verifies the `code-graph` binary is on `$PATH`
3. Reports status per hook: installed/missing/outdated
4. Exit code 0 if all hooks present, 1 if any missing

**Uninstall (`setup --remove`):**
1. Reads `~/.claude/settings.json`
2. Removes all hook entries tagged with `"source": "code-graph"` — only touches our hooks, never other hooks
3. Optionally removes `.code-graph/` from `.gitignore` (with `--clean` flag)
4. Optionally removes `.code-graph/` directory entirely (with `--purge` flag: deletes graph.db, daemon files, config)
5. Prints summary of what was removed
6. Is safe to run even if hooks are already gone (no-op)

---

## 7. CLI

### 7.1 Commands (v0.1)

| Command | Description |
|---------|-------------|
| `index` | Build or incrementally update graph |
| `find <pattern>` | Find symbols by name/pattern |
| `refs <symbol>` | Find references to a symbol |
| `impact <target> [--depth N] [--confidence LEVEL]` | Blast radius (target = file path or symbol name) |
| `diff [--base REF]` | Git diff -> affected nodes + impact |
| `callers <symbol>` | Who calls this symbol |
| `callees <symbol>` | What does this symbol call |
| `search <query>` | Full-text search |
| `stats` | Graph statistics |
| `watch [--daemon] [--status] [--stop]` | Daemon mode |
| `setup <platform> [--check] [--remove]` | Install/verify/remove agent hooks |
| `eval [--suite NAME]` | Run benchmark suite |

### 7.2 Output Formats

Three modes on every command:

```bash
code-graph find UserService              # compact (default, AI-optimized)
code-graph find UserService --table      # human-readable table
code-graph find UserService --json       # structured JSON
```

Compact output example:
```
UserService class src/services/user.ts:15-89 [pub, async]
  -> calls: AuthService.validate, Database.query, Logger.info
  -> tested_by: test_user_creation, test_user_deletion
  <- callers: UserController.create, UserController.update
```

### 7.3 Blast Radius Precision Control

```bash
code-graph impact src/auth.ts              # all symbols in file, default depth 3
code-graph impact UserService              # specific symbol
code-graph impact --depth 2                # shallow (high precision, defaults to working tree changes)
code-graph impact --confidence high        # only high-confidence impacts
code-graph impact src/auth.ts --depth 5 --confidence medium  # combined
```

---

## 8. Testing Strategy

### 8.1 Per-Crate Tests

| Crate | Unit | Integration | Property (proptest) | Benchmarks |
|-------|------|-------------|---------------------|------------|
| `domain` | traversals, blast radius, change detection, use cases | - | cycles, disconnected graphs, deep nesting | - |
| `parser` | per-language fixtures | cross-language consistency | random valid source -> valid locations, no dupe qualified names | parse throughput |
| `storage` | CRUD, FTS5, migrations, triggers | multi-process WAL concurrency | random insert -> query consistency | query latency at scale |
| `watch` | staleness, debounce, project detection | daemon lifecycle, file change -> update | - | incremental latency |
| `cli` | output formatting | subprocess tests (dogfood own codebase) | - | command latency |

### 8.2 Eval Framework

```bash
code-graph eval                    # all benchmarks
code-graph eval --suite search     # search quality (MRR, precision@k)
code-graph eval --suite impact     # blast radius precision/recall/F1
```

**Methodology:**
- Eval dataset: 3+ open-source repos (one per supported language ecosystem), selected for diversity
- Search ground truth: manually curated query sets (50+ queries) with expected top-k results, reviewed by contributors
- Impact ground truth: manually labeled commits with known affected files/symbols
- Eval runs as a CI job on each release to track quality regressions

**Quality targets:**
- Search MRR > 0.50 (vs code-review-graph's 0.35)
- Blast radius precision > 0.55 at high confidence (vs code-review-graph's 0.38)

### 8.3 Coverage

- `cargo-llvm-cov` with 80% minimum
- CI fails below threshold

---

## 9. CI/CD + Distribution

### 9.1 Lefthook

```yaml
pre-commit:
  parallel: true
  commands:
    fmt:
      run: cargo fmt --check
    clippy:
      run: cargo clippy --workspace -- -Dwarnings
    test:
      run: cargo test --workspace

pre-push:
  commands:
    full:
      run: cargo test --workspace && cargo bench --no-run
```

### 9.2 GitHub Actions

- **On PR:** fmt, clippy, test (with coverage), bench (no-run), `cargo-audit`
- **Matrix:** Ubuntu + macOS, stable Rust
- **On tag (`v*`):** build release binaries (linux x86_64, linux aarch64, macOS x86_64, macOS aarch64), publish to crates.io, create GitHub release

### 9.3 crates.io

- `the-code-graph` — the CLI binary (`cargo install the-code-graph`)
- All workspace crates published with synchronized versions
- Release skill (uncommitted, local) automates: version bump, CHANGELOG, git tag, push

---

## 10. Milestones

### v0.1 — Core
- Tree-sitter parsing (TS/JS, Rust, Python, Go)
- Graph construction (SQLite, nodes, edges)
- Import resolution (oxc_resolver + barrel traversal for TS/JS, language-specific for others)
- Incremental updates (SHA-256 hash-based)
- Blast radius / impact analysis with confidence tiers
- Find, refs, callers, callees, diff
- Full-text search (FTS5, BM25, trigram fallback)
- Watch daemon + Claude Code hooks + lazy staleness
- CLI with compact/table/json output
- Eval framework (search MRR, blast radius precision/recall)
- Lefthook + GitHub Actions CI
- Project detection (.git only, blocklisted roots)
- Error handling, logging, configuration

### v0.2 — Analysis
- Execution flow detection + criticality scoring
- Risk scoring (flow participation, coupling, test coverage, security sensitivity)
- Community detection (Leiden algorithm)
- Embeddings + hybrid search (FTS5 + vector, RRF fusion)
- Dead code detection
- Clone detection

### v0.3 — Ecosystem
- Wiki generation from communities
- Web UI visualization
- Multi-repo registry
- Refactoring tools (rename preview, move suggestions)
- MCP adapter (if needed)
- Language extensibility proven (architecture supports new languages, not necessarily shipped)

---

## 11. Data Flow

### 11.1 Indexing

```
git ls-files
  -> filter by supported extensions
  -> rayon parallel: tree-sitter parse per file (failures skipped with warning)
  -> collect ParseResult (symbols, edges, raw imports)
  -> resolve imports (oxc_resolver + barrel traversal for TS/JS, language-specific for others)
  -> wire resolved edges (ImportsFrom, Calls, Extends, Implements, etc.)
  -> resolve cross-file calls (scoped -> qualified -> single-candidate fallback)
  -> store to SQLite (files, symbols, edges)
  -> FTS5 index updated automatically via triggers
```

### 11.2 Query (e.g., blast radius)

```
code-graph impact src/auth.ts --depth 3
  -> detect project root (.git)
  -> open SQLite store at .code-graph/graph.db
  -> lazy staleness check (git status --porcelain, auto-update if needed)
  -> load edges into InMemoryGraph (domain crate)
  -> resolve target: file path -> all symbols in file
  -> BFS from target nodes, max depth 3
  -> classify results by confidence tier
  -> format output (compact/table/json)
```

### 11.3 Change Detection

```
code-graph diff
  -> git diff --unified=0 -> DiffHunk list
  -> for each hunk: find symbols with overlapping line ranges
  -> compute affected nodes
  -> run blast radius on affected nodes
  -> output changed symbols + impact
```
