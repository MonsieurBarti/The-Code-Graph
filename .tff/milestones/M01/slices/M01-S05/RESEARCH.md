# Research — M01-S05: CLI Foundation & Index Command

## R1: ParseProvider Port Trait Design

### Problem

`IndexUseCase` lives in `domain` (no parser dependency). It needs to orchestrate parse + resolve without knowing about `ParserRegistry`, `ResolverRegistry`, `RawImport`, `Export`, or `ResolveContext`.

### Current API Surface

**LanguageParser::parse()** (parser crate):
```rust
pub trait LanguageParser: Send + Sync {
    fn language(&self) -> Language;
    fn file_extensions(&self) -> &[&str];
    fn parse(&self, source: &[u8], path: &Path) -> Result<ParseResult>;
}
```

**ParseResult** (parser crate):
```rust
pub struct ParseResult {
    pub symbols: Vec<SymbolNode>,
    pub edges: Vec<Edge>,           // structural: Contains, ChildOf, Implements, Extends, Embeds
    pub imports: Vec<RawImport>,    // unresolved
    pub exports: Vec<Export>,       // declarations
}
```

**ImportResolver::resolve()** (parser crate):
```rust
fn resolve(
    &self,
    file_path: &Path,
    parse_result: &ParseResult,
    context: &ResolveContext,
) -> Result<Vec<Edge>>;
```

**ResolveContext** (parser crate):
```rust
pub struct ResolveContext {
    pub project_root: PathBuf,
    pub parsed_files: HashMap<PathBuf, ParseResult>,
    pub file_tree: Vec<PathBuf>,
}
```

**GraphStore::store_file_data()** (domain crate):
```rust
fn store_file_data(&self, file: &FileNode, symbols: &[SymbolNode], edges: &[Edge]) -> Result<()>;
```

### Proposed Port Trait

```rust
/// domain/src/ports.rs — new outbound port

/// Data ready for storage: one file's worth of graph data.
pub struct FileData {
    pub file: FileNode,
    pub symbols: Vec<SymbolNode>,
    pub edges: Vec<Edge>,   // structural + resolved imports
}

/// Outbound port: parse and resolve a batch of source files.
/// Abstracts away parser-crate internals (RawImport, Export, ResolveContext).
pub trait ParseProvider: Send + Sync {
    /// Parse and resolve a set of source files into storage-ready data.
    /// Caller provides file paths + source content; provider returns FileData.
    fn parse_and_resolve(
        &self,
        files: &[(PathBuf, Vec<u8>)],
        project_root: &Path,
    ) -> Result<Vec<FileData>>;
}
```

### Why This Design

- **Batch-oriented**: The provider receives all files at once — it can internally parallelize parsing (rayon) and resolve imports (needs cross-file context). The caller doesn't need to know about phases.
- **Domain types only**: `FileData` contains `FileNode`, `SymbolNode`, `Edge` — all domain types. No parser types leak.
- **Single method**: `parse_and_resolve()` is the only method. No need for separate `parse_file()` + `resolve_imports()` — the caller has no use for intermediate results. Keeping phases internal to the adapter is cleaner.
- **Testable**: `IndexUseCase` can be tested with a mock `ParseProvider` that returns canned `FileData` — no tree-sitter needed in domain tests.

### IndexUseCase Updated Signature

```rust
pub struct IndexUseCase<S, P, F, G> {
    store: S,       // GraphStore
    parser: P,      // ParseProvider (NEW)
    fs: F,          // FileSystem
    git: G,         // GitProvider
}
```

### Adapter Implementation Location

`cli` crate: `adapters/parse_adapter.rs` — wraps `ParserRegistry` + `ResolverRegistry` + `rayon`:

```rust
pub struct RayonParseProvider {
    registry: ParserRegistry,
    resolver_registry: ResolverRegistry,
}

impl ParseProvider for RayonParseProvider {
    fn parse_and_resolve(&self, files: &[(PathBuf, Vec<u8>)], project_root: &Path) -> Result<Vec<FileData>> {
        // Phase 1: parallel parse via rayon
        let parse_results: Vec<(PathBuf, ParseResult)> = files.par_iter()
            .filter_map(|(path, source)| {
                self.registry.parser_for_file(path)
                    .and_then(|parser| parser.parse(source, path).ok())
                    .map(|result| (path.clone(), result))
            })
            .collect();

        // Build ResolveContext
        let context = ResolveContext { ... };

        // Phase 2: resolve imports (parallel per file, reads shared context)
        let resolved_edges: Vec<Vec<Edge>> = parse_results.par_iter()
            .map(|(path, result)| {
                self.resolver_registry.resolve_file(path, lang, result, &context)
                    .unwrap_or_default()
            })
            .collect();

        // Phase 3: combine into FileData
        // ... merge structural edges + resolved edges per file
    }
}
```

---

## R2: Cross-File Call Resolution — Critical Finding

### Discovery: Parsers Do NOT Extract Call Sites

Searched all four language parsers (TypeScript, Rust, Python, Go). **Zero call expression extraction exists.** Parsers only extract:
- Symbol declarations (functions, classes, structs, methods, etc.)
- Structural edges (Contains, ChildOf, Implements, Extends, Embeds)
- Raw imports and exports

No parser extracts `call_expression`, `method_call`, or any call site data. The `EdgeKind::Calls` variant exists in the domain model and is used in test fixtures, but no parser produces it.

### What Cross-File Call Resolution (Section 3.9) Requires

The four-step strategy needs:
1. **Call sites**: What function/method is being called, and from where
2. **Import scope**: What symbols are imported into the calling file (from ImportsFrom edges)
3. **Symbol lookup**: The full symbol graph to find candidates

Step 1 is the blocker. Without call site extraction, there's nothing to resolve.

### What Would Be Needed

A new data structure in `ParseResult`:
```rust
pub struct RawCall {
    pub caller_qualified: String,     // "src/main.ts::handleRequest"
    pub callee_name: String,          // bare: "validate"
    pub callee_qualifier: Option<String>, // qualifier: "auth" (from auth.validate())
    pub location: Location,
}
```

Plus tree-sitter extraction in each language parser:
- TS/JS: `call_expression` nodes
- Rust: `call_expression`, `macro_invocation` nodes
- Python: `call` nodes
- Go: `call_expression` nodes

### Scope Decision Required

**Option A: Add call extraction to parsers in S05, then implement resolution.**
- Extends parser crate (violates S05's "CLI Foundation" focus)
- ~200-400 lines per language parser for call extraction
- Significant scope creep

**Option B: Defer call resolution entirely. S05 produces a graph with import edges but no cross-file Calls edges.**
- Clean slice boundary
- `callers`/`callees` commands won't work until a later slice adds call extraction + resolution
- Simpler, faster delivery

**Option C: Add call extraction as a focused sub-task within S05, resolution as post-processing.**
- Middle ground — adds call extraction to parsers, implements resolution in the index pipeline
- Still a large addition (~800-1200 lines across parsers + resolver)

### Recommendation: Option B — Defer

Cross-file call resolution requires parser changes (call extraction) that don't belong in a "CLI Foundation" slice. The graph after S05 will have: symbols, structural edges (Contains, ChildOf, Implements, Extends, Embeds), and import edges (ImportsFrom, ReExport, BarrelReExportAll, ConditionalImport, SideEffectImport, DotImport). This is a useful, queryable graph — `find`, `refs`, `impact`, `search`, and `stats` all work. Only `callers`/`callees` require Calls edges.

Create a new slice (S05b or insert before S06) for: call extraction + cross-file call resolution.

---

## R3: CLI Architecture — clap v4 + tracing

### Clap v4 Derive Pattern

```rust
#[derive(Parser)]
#[command(name = "code-graph", version, about)]
pub struct Cli {
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[arg(long, global = true)]
    pub debug: bool,

    /// Output as JSON
    #[arg(long, global = true)]
    pub json: bool,

    /// Output as table
    #[arg(long, global = true)]
    pub table: bool,

    #[command(subcommand)]
    pub command: Commands,
}
```

**Design note**: Use `--json` and `--table` as boolean flags (not `--output=json`). This matches the spec's examples: `code-graph find UserService --json`. Default is compact.

### Subcommand Stubs

S05 implements `index` fully. All other commands return `CodeGraphError::Other("not implemented — coming in a future release")` with exit code 1. This lets us define the full CLI surface without implementing every handler.

### tracing-subscriber Setup

```rust
pub fn init_logging(verbose: u8, debug: bool) {
    let level = if debug { "debug" }
        else if verbose >= 2 { "debug" }
        else if verbose == 1 { "info" }
        else { "warn" };

    let filter = EnvFilter::try_from_env("CODE_GRAPH_LOG")
        .unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .compact()
        .without_time()   // no timestamps for CLI
        .init();
}
```

**Key**: logs go to stderr, output goes to stdout. No timestamps for CLI mode (daemon gets timestamps later in S07).

### No Async Runtime

This is a synchronous CLI tool. No `tokio`. All I/O is blocking (`std::fs`, `Command::new("git")`, SQLite via r2d2). Rayon handles parallelism for CPU-bound parsing. Async would add complexity with zero benefit for S05.

### Error-to-Exit-Code Mapping

```rust
fn exit_code(err: &CodeGraphError) -> i32 {
    match err {
        CodeGraphError::NoProject => 2,
        CodeGraphError::BlocklistedRoot(_) => 2,
        _ => 1,
    }
}
// Exit code 3 (usage error) is handled by clap automatically
```

---

## R4: File Walking — `ignore` Crate vs `git ls-files`

### Recommendation: Use `ignore` crate

The `ignore` crate (from the ripgrep author) provides a high-performance recursive walker that respects `.gitignore` and supports custom ignore files.

```rust
let mut builder = WalkBuilder::new(project_root);
builder.add_custom_ignore_filename(".code-graphignore");
// Automatically respects .gitignore, .git/info/exclude, global gitignore
```

### Why Not `git ls-files`

| Aspect | `git ls-files` | `ignore` crate |
|--------|---------------|----------------|
| Untracked files | Misses them | Includes (if not ignored) |
| Custom ignore | Must post-filter | Native `.code-graphignore` support |
| Performance | Shell-out overhead | Native Rust, single pass |
| Git dependency | Requires git on PATH | No external dependency |
| Pattern semantics | Full gitignore | Full gitignore (same engine as ripgrep) |

### Integration with FileSystem Port

The `RealFileSystem` adapter uses `ignore::WalkBuilder` for `list_files()`:

```rust
impl FileSystem for RealFileSystem {
    fn list_files(&self, root: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
        let mut builder = WalkBuilder::new(root);
        builder.add_custom_ignore_filename(".code-graphignore");

        let files = builder.build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map_or(false, |ft| ft.is_file()))
            .filter(|e| {
                e.path().extension()
                    .and_then(|ext| ext.to_str())
                    .map_or(false, |ext| extensions.contains(&ext))
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        Ok(files)
    }
}
```

### Config-Based Excludes

Additional patterns from `.code-graph/config.toml` `[index].exclude`:
```rust
if let Some(ref index_config) = config.index {
    if let Some(ref excludes) = index_config.exclude {
        for pattern in excludes {
            builder.add_ignore(pattern);  // or use overrides
        }
    }
}
```

---

## R5: Dependency Matrix for New Crates

### `cli` crate (`crates/cli/Cargo.toml`)

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
sha2 = "0.10"       # for file hashing (FileSystem adapter)
```

### `binary` crate (`crates/binary/Cargo.toml`)

```toml
[dependencies]
cli = { path = "../cli" }
```

Minimal — just calls into `cli::run()`.

### Workspace Cargo.toml Update

```toml
[workspace]
members = ["crates/domain", "crates/storage", "crates/parser", "crates/cli", "crates/binary"]
```

### New Dependency Analysis

| Dep | Purpose | Size Impact | Already in Workspace? |
|-----|---------|-------------|----------------------|
| `clap` (derive) | CLI argument parsing | ~400KB | No |
| `tracing` | Structured logging | ~100KB | No (but common) |
| `tracing-subscriber` | Log formatting + filtering | ~200KB | No |
| `ignore` | File walking with gitignore | ~150KB | No |
| `rayon` | Parallel file parsing | ~200KB | No |
| `sha2` | SHA-256 file hashing | ~100KB | No |
| `toml` | Config file parsing | Already in parser | Yes |
| `serde_json` | JSON output | Already in storage | Yes |

Total new binary size impact: ~1.2MB (acceptable for a CLI tool).

---

## R6: Project Root Detection & Setup

### Implementation (inline, no external dependency)

```rust
const BLOCKLIST: &[&str] = &["/", "/home", "/Users", "/tmp", "/var"];

pub fn find_project_root(start: &Path) -> Result<PathBuf> {
    let mut current = start.canonicalize()
        .map_err(|e| CodeGraphError::FileSystem { path: start.into(), source: e })?;

    loop {
        if is_blocklisted(&current) {
            return Err(CodeGraphError::BlocklistedRoot(current));
        }
        if current.join(".git").is_dir() {
            return Ok(current);
        }
        if !current.pop() {
            return Err(CodeGraphError::NoProject);
        }
    }
}

fn is_blocklisted(path: &Path) -> bool {
    let s = path.to_string_lossy();
    BLOCKLIST.contains(&s.as_ref())
        || std::env::var("HOME").ok().map_or(false, |h| s == h)
}
```

### `.code-graph/` Directory Setup

```rust
pub fn ensure_data_dir(project_root: &Path) -> Result<PathBuf> {
    let dir = project_root.join(".code-graph");
    std::fs::create_dir_all(&dir)
        .map_err(|e| CodeGraphError::FileSystem { path: dir.clone(), source: e })?;
    Ok(dir)
}
```

---

## R7: Output Formatting Infrastructure

### Design

```rust
#[derive(Clone, Copy)]
pub enum OutputFormat {
    Compact,
    Table,
    Json,
}

impl OutputFormat {
    pub fn from_flags(json: bool, table: bool) -> Self {
        if json { Self::Json }
        else if table { Self::Table }
        else { Self::Compact }
    }
}

pub trait Displayable {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()>;
    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()>;
    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()>;
}

pub fn print<T: Displayable>(value: &T, format: OutputFormat) {
    let stdout = std::io::stdout();
    let mut w = stdout.lock();
    match format {
        OutputFormat::Compact => value.fmt_compact(&mut w),
        OutputFormat::Table => value.fmt_table(&mut w),
        OutputFormat::Json => value.fmt_json(&mut w),
    }.expect("write to stdout failed");
}
```

### IndexStats Formatting

```rust
impl Displayable for IndexStats {
    fn fmt_compact(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(w, "Indexed {} files, {} symbols, {} edges in {:.1}s",
            self.files_indexed, self.symbols_extracted, self.edges_created,
            self.duration.as_secs_f64())
    }

    fn fmt_table(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(w, "Metric         | Count")?;
        writeln!(w, "───────────────┼──────────")?;
        writeln!(w, "Files indexed  | {}", self.files_indexed)?;
        writeln!(w, "Symbols        | {}", self.symbols_extracted)?;
        writeln!(w, "Edges          | {}", self.edges_created)?;
        writeln!(w, "Duration       | {:.1}s", self.duration.as_secs_f64())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> io::Result<()> {
        let json = serde_json::to_string_pretty(&self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        writeln!(w, "{}", json)
    }
}
```

---

## R8: GitProvider Adapter — Shell-Out

### Implementation

```rust
use std::process::Command;

pub struct ShellGitProvider;

impl GitProvider for ShellGitProvider {
    fn changed_files(&self, from: &str, to: &str) -> Result<Vec<PathBuf>> {
        let output = Command::new("git")
            .args(["diff", "--name-only", from, to])
            .output()
            .map_err(|e| CodeGraphError::Git(format!("failed to run git: {}", e)))?;

        if !output.status.success() {
            return Err(CodeGraphError::Git(
                String::from_utf8_lossy(&output.stderr).to_string()
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(PathBuf::from)
            .collect())
    }

    fn diff_hunks(&self, from: &str, to: &str) -> Result<Vec<DiffHunk>> {
        let output = Command::new("git")
            .args(["diff", "--unified=0", from, to])
            .output()
            .map_err(|e| CodeGraphError::Git(format!("failed to run git: {}", e)))?;
        // Parse unified diff output into DiffHunks...
        todo!("parse diff output")
    }

    fn current_head(&self) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .map_err(|e| CodeGraphError::Git(format!("failed to run git: {}", e)))?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}
```

### Note on `full_index`

For `full_index`, we don't need git at all — we just walk files via the `ignore` crate. Git is needed for:
- `incremental_index` (change detection via `git status --porcelain`)
- `diff` command (git diff → affected symbols)
- `current_head()` for metadata storage

So `GitProvider` can have minimal implementation in S05 — just `current_head()` for storing the indexed commit hash.

---

## R9: Index Pipeline — Full Data Flow

```
code-graph index [--path PATH]
  │
  ├── detect project root (.git) or use --path
  ├── check blocklist
  ├── ensure .code-graph/ exists
  ├── load config (.code-graph/config.toml)
  ├── open/create SqliteStore at .code-graph/graph.db
  │
  ├── list source files (ignore crate, .code-graphignore, extensions filter)
  ├── read file contents + compute SHA-256 hashes
  │
  ├── ParseProvider.parse_and_resolve(files, project_root)
  │   ├── Phase 1: rayon par_iter → LanguageParser::parse() per file
  │   ├── Phase 2: build ResolveContext from all ParseResults
  │   ├── Phase 3: rayon par_iter → ImportResolver::resolve() per file
  │   └── Phase 4: merge structural + resolved edges → Vec<FileData>
  │
  ├── store to SQLite: for each FileData → store.store_file_data()
  │   (FTS5 auto-updated via triggers)
  │
  └── print IndexStats (compact/table/json)
```

### Three Sequential Phases with Internal Parallelism

1. **Parse** (parallel): Each file independently parsed by tree-sitter
2. **Resolve imports** (parallel, shared read-only context): Each file's imports resolved against all parse results
3. **Store** (sequential): SQLite writes are sequential (WAL mode, but single writer)

---

## R10: SPEC Corrections from Research

1. **Cross-file call resolution deferred**: Parsers don't extract call sites. Call resolution requires parser changes (call extraction) that should be a separate slice. S05 produces import edges but not cross-file Calls edges.

2. **No async runtime**: CLI is synchronous. Rayon for CPU parallelism. No tokio.

3. **`ignore` crate replaces `git ls-files`**: Better handling of untracked files, native custom ignore support, no shell dependency.

4. **`--json` / `--table` as boolean flags**: Not `--output=json`. Matches spec examples.

5. **`ParseProvider` is a single-method port**: `parse_and_resolve()` batch operation. No need to expose phases to domain.

6. **`GitProvider` minimal in S05**: Only `current_head()` needed for full_index. `diff_hunks` and `changed_files` are for incremental/diff commands (S06/S07).

---

## Summary of Decisions

| Question | Decision | Rationale |
|----------|----------|-----------|
| ParseProvider design | Single `parse_and_resolve()` batch method | Hides phases, domain stays clean, testable with mock |
| Cross-file call resolution | **Defer to new slice** | Parsers lack call extraction — not an S05 concern |
| Async runtime | None (sync + rayon) | CLI tool, no I/O concurrency needed |
| File walking | `ignore` crate v0.4 | Respects .gitignore + custom .code-graphignore |
| GitProvider | Shell-out, minimal (just `current_head()` for S05) | Full implementation in S07 (incremental) |
| Output format flags | `--json` / `--table` booleans | Matches spec examples |
| Config parsing | `toml` 0.8 in cli crate | Already in workspace |
| File hashing | `sha2` 0.10 | Standard, fast |
| Rayon | cli crate dependency | Adapter orchestrates parallelism |
| Subcommand stubs | All 12 commands defined, only `index` implemented | Full CLI surface, graceful "not implemented" for others |
