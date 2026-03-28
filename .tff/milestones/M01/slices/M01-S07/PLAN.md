# Plan — M01-S07: Incremental Updates & Watch Daemon

> For agentic workers: execute task-by-task with TDD.

**Goal:** Implement the incremental indexing pipeline and watch daemon so `code-graph` can efficiently update only changed files instead of rebuilding from scratch. This is the slice that turns a "rebuild the world" indexer into a practical developer tool.

**Architecture:** New `watch` crate (6th workspace crate) with `notify-debouncer-full` file watcher, PID-based daemon lifecycle, and `signal-hook` for graceful shutdown. Domain gains `GitProvider::modified_files()` and the incremental pipeline in `IndexUseCase`. All query commands get automatic freshness via `ensure_fresh()` integrated into `open_graph()`. No async runtime needed.

**Key decisions (from DISCUSS/RESEARCH):**
- PID file only (no Unix socket) for daemon management — simpler for v0.1
- Self-respawn via `Command::spawn` for daemon backgrounding — avoids fork hazards
- `notify-debouncer-full` for debounce — battle-tested, handles rename stitching
- `ensure_fresh()` inserted into `open_graph()` — single change point covers all 8 query commands
- Setup command deferred to S08 (Agent Integration)

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `Cargo.toml` (workspace) | Modify | Add `"crates/watch"` to workspace members |
| `crates/domain/src/ports.rs` | Modify | Add `modified_files()` to `GitProvider` |
| `crates/domain/src/test_support.rs` | Modify | Update `MockGitProvider` with `modified_files()` |
| `crates/domain/src/use_cases/index.rs` | Modify | Implement `incremental_index()`, add `incremental_files()` |
| `crates/cli/src/adapters/git.rs` | Modify | Implement `ShellGitProvider::modified_files()` |
| `crates/cli/src/commands/mod.rs` | Modify | `Watch` variant becomes `Watch(WatchArgs)`, `IndexArgs` gains flags |
| `crates/cli/src/commands/helpers.rs` | Modify | Add `ensure_fresh()` call inside `open_graph()` |
| `crates/cli/src/lib.rs` | Modify | Wire `Commands::Watch(args)` to watch handler |
| `crates/cli/src/config.rs` | Modify | Add `WatchConfig` with `debounce_ms` |
| `crates/cli/Cargo.toml` | Modify | Add `watch` dependency |
| `crates/watch/Cargo.toml` | Create | Watch crate manifest |
| `crates/watch/src/lib.rs` | Create | Crate root, re-exports |
| `crates/watch/src/watcher.rs` | Create | `notify`-based watcher + debounce event loop |
| `crates/watch/src/daemon.rs` | Create | Daemon lifecycle: start, stop, status, PID, signal handling |
| `crates/watch/src/freshness.rs` | Create | `ensure_fresh()` — lazy staleness check |
| `crates/watch/src/pid.rs` | Create | PID file read/write/stale detection |
| `crates/cli/src/commands/watch.rs` | Create | CLI command handler for `watch` |

---

## Acceptance Criteria

> Numbering follows SPEC.md.

### Domain
- AC1: `GitProvider::modified_files(&self) -> Result<Vec<PathBuf>>` returns files with uncommitted changes.
- AC2: `IndexUseCase::incremental_index(root)` detects changed files via git status, hash-checks against stored hashes, re-parses changed files + 1-hop dependents, atomically updates store.
- AC3: `IndexUseCase::incremental_files(root, files)` processes an explicit file list through the same pipeline (hook use case).
- AC4: Files whose hash hasn't changed are skipped (no re-parse, no store update).
- AC5: Deleted files (hash computation fails) are removed from store via `remove_file_data()`.

### Watch Crate
- AC6: `notify-debouncer-full` based watcher batches filesystem events with configurable debounce (default 100ms).
- AC7: Path filtering ignores `.git/`, `target/`, `node_modules/`, `.code-graph/`. Only supported extensions pass through.
- AC8: PID file at `.code-graph/daemon.pid` — write on start, stale detection via `kill(pid, 0)`, cleanup on shutdown.
- AC9: Signal handling via `signal-hook`: SIGTERM/SIGINT trigger graceful shutdown (remove PID file, flush logs).
- AC10: Log rotation via `tracing-appender` with daily rotation, max 7 files, in `.code-graph/` directory.
- AC11: `ensure_fresh(store, root, data_dir)` checks daemon PID → if alive, skip. Otherwise run lazy staleness pipeline.

### CLI
- AC12: `code-graph index --incremental` runs incremental pipeline instead of full index.
- AC13: `code-graph index --files <paths>` runs incremental pipeline for specified files only.
- AC14: `code-graph watch` runs foreground watcher (logs to stderr).
- AC15: `code-graph watch --daemon` spawns background daemon via self-respawn, prints PID, parent exits.
- AC16: `code-graph watch --status` reports daemon status (running with PID, or stopped).
- AC17: `code-graph watch --stop` sends SIGTERM to daemon and removes PID file.
- AC18: All query commands auto-call `ensure_fresh()` via `open_graph()` — zero per-command changes.

### Adapter
- AC19: `ShellGitProvider::modified_files()` parses `git status --porcelain` output correctly (M, A, D, R, ?? statuses). Filters to supported extensions.

### Quality
- AC20: `cargo test --workspace` passes.
- AC21: `cargo clippy --workspace -- -Dwarnings` passes.

---

## Wave 0 — Domain Port + Watch Crate Scaffold

### T01: Add `modified_files()` to `GitProvider` trait + mock
**AC coverage:** AC1
**Files:** `crates/domain/src/ports.rs`, `crates/domain/src/test_support.rs`

1. Add to `GitProvider` trait in `ports.rs`:
   ```rust
   fn modified_files(&self) -> Result<Vec<PathBuf>>;
   ```
2. Implement in `MockGitProvider` (`test_support.rs`):
   - Change `MockGitProvider` from unit struct to struct with `modified: Vec<PathBuf>` field
   - Add constructor: `MockGitProvider::new()` (empty) and `MockGitProvider::with_modified(files)`
   - Return `self.modified.clone()` from `modified_files()`
3. Update all `MockGitProvider` usages in `index.rs` tests (replace `MockGitProvider` with `MockGitProvider::new()`)
4. `cargo test -p domain` passes

### T02: Create watch crate scaffold + workspace integration
**AC coverage:** (structural prerequisite)
**Files:** `Cargo.toml` (workspace), `crates/watch/Cargo.toml`, `crates/watch/src/lib.rs`, `crates/watch/src/watcher.rs`, `crates/watch/src/daemon.rs`, `crates/watch/src/freshness.rs`, `crates/watch/src/pid.rs`, `crates/cli/Cargo.toml`

1. Create `crates/watch/Cargo.toml`:
   ```toml
   [package]
   name = "watch"
   version = "0.1.0"
   edition = "2021"

   [dependencies]
   domain = { path = "../domain" }
   parser = { path = "../parser" }
   storage = { path = "../storage" }
   notify = "8"
   notify-debouncer-full = "0.7"
   signal-hook = "0.3"
   tracing = "0.1"
   tracing-appender = "0.2"
   tracing-subscriber = { version = "0.3", features = ["env-filter"] }
   libc = "0.2"
   ignore = "0.4"
   ```
2. Create `crates/watch/src/lib.rs` with module declarations:
   ```rust
   pub mod watcher;
   pub mod daemon;
   pub mod freshness;
   pub mod pid;
   ```
3. Create stub files for each module (empty `pub` items or minimal types)
4. Add `"crates/watch"` to workspace `Cargo.toml` members (before `"crates/cli"`)
5. Add `watch = { path = "../watch" }` to `crates/cli/Cargo.toml` dependencies
6. `cargo build --workspace` compiles

---

## Wave 1 — Core Implementations

### T03: Implement incremental pipeline in `IndexUseCase`
**AC coverage:** AC2, AC3, AC4, AC5
**Files:** `crates/domain/src/use_cases/index.rs`
**Depends on:** T01

1. Write tests first:
   - `incremental_index_skips_unchanged_files`: store has file with hash "abc", fs returns same hash → file not re-parsed, stats show 0 files indexed
   - `incremental_index_reparses_changed_files`: store has file with hash "old", fs returns "new" → file re-parsed, store updated
   - `incremental_index_reparses_one_hop_dependents`: file A changed, file B has edge targeting A → both A and B re-parsed
   - `incremental_index_no_modified_files_returns_zeros`: git reports no changes → stats all zeros
   - `incremental_files_processes_explicit_list`: explicit file list bypasses git status, hash-checks + re-parses
   - `incremental_files_skips_unchanged_in_list`: explicit list but hash matches → skip
2. Implement internal helper `run_incremental(&self, root: &Path, changed_paths: Vec<PathBuf>) -> Result<IndexStats>`:
   ```rust
   fn run_incremental(&self, root: &Path, changed_paths: Vec<PathBuf>) -> Result<IndexStats> {
       let start = Instant::now();
       let mut reparse_set = Vec::new();

       // Phase 1: Hash check — filter to actually-changed files
       for path in &changed_paths {
           let abs_path = root.join(path);
           let current_hash = match self.fs.file_hash(&abs_path) {
               Ok(h) => h,
               Err(_) => { /* file deleted — remove from store */
                   self.store.remove_file_data(path)?;
                   continue;
               }
           };
           let stored = self.store.get_file(path)?;
           if stored.as_ref().is_some_and(|f| f.hash == current_hash) {
               continue; // unchanged
           }
           reparse_set.push(path.clone());
       }

       // Phase 2: Find 1-hop dependents
       let mut dependent_set = Vec::new();
       for path in &reparse_set {
           let path_str = path.to_string_lossy();
           // Find edges targeting symbols in this file (files that import from it)
           let all_symbols = self.store.all_symbols()?;
           let file_symbols: Vec<&SymbolNode> = all_symbols.iter()
               .filter(|s| s.location.file == *path)
               .collect();
           for sym in file_symbols {
               let incoming = self.store.get_edges_to(&sym.qualified_name)?;
               for edge in incoming {
                   if let Some(source_file) = edge.source.split("::").next() {
                       let dep_path = PathBuf::from(source_file);
                       if !reparse_set.contains(&dep_path) && !dependent_set.contains(&dep_path) {
                           dependent_set.push(dep_path);
                       }
                   }
               }
           }
       }
       reparse_set.extend(dependent_set);
       reparse_set.sort();
       reparse_set.dedup();

       if reparse_set.is_empty() {
           return Ok(IndexStats { files_indexed: 0, symbols_extracted: 0, edges_created: 0, duration: start.elapsed() });
       }

       // Phase 3: Read + parse + store
       let mut files_with_content = Vec::new();
       for path in &reparse_set {
           let abs_path = root.join(path);
           match self.fs.read_file(&abs_path) {
               Ok(content) => files_with_content.push((path.clone(), content.into_bytes())),
               Err(e) => tracing::warn!("skipping {}: {e}", path.display()),
           }
       }

       let file_data = self.parser.parse_and_resolve(&files_with_content, root)?;
       let mut stats = IndexStats { files_indexed: 0, symbols_extracted: 0, edges_created: 0, duration: start.elapsed() };
       for fd in &file_data {
           self.store.remove_file_data(&fd.file.path)?;
           self.store.store_file_data(&fd.file, &fd.symbols, &fd.edges)?;
           stats.files_indexed += 1;
           stats.symbols_extracted += fd.symbols.len();
           stats.edges_created += fd.edges.len();
       }
       stats.duration = start.elapsed();
       Ok(stats)
   }
   ```
3. Implement `incremental_index`:
   ```rust
   pub fn incremental_index(&self, root: &Path) -> Result<IndexStats> {
       let modified = self.git.modified_files()?;
       self.run_incremental(root, modified)
   }
   ```
4. Implement `incremental_files`:
   ```rust
   pub fn incremental_files(&self, root: &Path, files: Vec<PathBuf>) -> Result<IndexStats> {
       self.run_incremental(root, files)
   }
   ```
5. `cargo test -p domain` passes

### T04: Implement `ShellGitProvider::modified_files()`
**AC coverage:** AC19
**Files:** `crates/cli/src/adapters/git.rs`
**Depends on:** T01

1. Write tests first (unit tests for parsing, not requiring git):
   - Parse `" M src/main.rs\n"` → `vec![PathBuf::from("src/main.rs")]`
   - Parse `"?? new_file.ts\n"` → `vec![PathBuf::from("new_file.ts")]`
   - Parse `" D deleted.rs\n"` → `vec![PathBuf::from("deleted.rs")]`
   - Parse `"MM both.ts\n"` → `vec![PathBuf::from("both.ts")]`
   - Parse `"R  old.rs -> new.rs\n"` → `vec![PathBuf::from("new.rs")]` (use new name)
   - Parse multi-line output with mixed statuses → correct vec
   - Empty output → empty vec
   - Filter to supported extensions only (skip `.json`, `.md`, etc.)
2. Extract `parse_git_status(output: &str) -> Vec<PathBuf>` helper function (testable without git)
3. Implement `modified_files()`:
   ```rust
   fn modified_files(&self) -> Result<Vec<PathBuf>> {
       let output = self.run_git(&["status", "--porcelain"])?;
       Ok(parse_git_status(&output))
   }
   ```
4. Parse porcelain format: first 2 chars are status, char 3 is space, rest is path
5. Handle rename format: `R  old -> new` — extract path after ` -> `
6. Filter to `SUPPORTED_EXTENSIONS`
7. `cargo test -p cli` passes

### T05: Implement watcher module
**AC coverage:** AC6, AC7
**Files:** `crates/watch/src/watcher.rs`
**Depends on:** T02

1. Write tests first:
   - `should_ignore` correctly filters `.git/`, `target/`, `node_modules/`, `.code-graph/`
   - `should_ignore` passes through normal source files
   - `has_supported_extension` filters to ts/tsx/js/jsx/rs/py/go only
2. Implement `CodeGraphWatcher` struct:
   ```rust
   pub struct CodeGraphWatcher {
       root: PathBuf,
       debounce_ms: u64,
   }
   ```
3. Implement `new(root: PathBuf, debounce_ms: u64) -> Self`
4. Implement `watch(&self, tx: mpsc::Sender<Vec<PathBuf>>) -> Result<()>`:
   - Create `notify-debouncer-full` with configurable timeout
   - Watch `root` recursively
   - On each debounced event batch:
     - Filter by `should_ignore()` and `has_supported_extension()`
     - Deduplicate by path
     - Convert to relative paths (relative to root)
     - Send batch to `tx`
5. Implement path filtering functions:
   ```rust
   fn should_ignore(path: &Path) -> bool {
       path.components().any(|c| {
           matches!(c.as_os_str().to_str(), Some(".git" | "target" | "node_modules" | ".code-graph"))
       })
   }

   fn has_supported_extension(path: &Path) -> bool {
       path.extension()
           .and_then(|e| e.to_str())
           .is_some_and(|e| ["ts","tsx","js","jsx","rs","py","go"].contains(&e))
   }
   ```
6. `cargo test -p watch` passes

### T06: Implement PID file management
**AC coverage:** AC8
**Files:** `crates/watch/src/pid.rs`
**Depends on:** T02

1. Write tests first:
   - `write_pid` creates file with PID string
   - `read_pid` reads valid PID from file
   - `read_pid` returns `None` for missing file
   - `read_pid` returns `None` for invalid content
   - `remove_pid` deletes file
   - `remove_pid` is no-op if file missing
   - `is_process_running` returns `false` for PID 0 or very large PID
2. Implement:
   ```rust
   pub fn pid_path(data_dir: &Path) -> PathBuf {
       data_dir.join("daemon.pid")
   }

   pub fn write_pid(data_dir: &Path, pid: u32) -> Result<()>
   pub fn read_pid(data_dir: &Path) -> Option<u32>
   pub fn remove_pid(data_dir: &Path)
   pub fn is_process_running(pid: u32) -> bool {
       unsafe { libc::kill(pid as i32, 0) == 0 }
   }

   /// Returns Some(pid) if daemon is alive, None otherwise.
   /// Cleans up stale PID file if process is dead.
   pub fn check_daemon(data_dir: &Path) -> Option<u32> {
       let pid = read_pid(data_dir)?;
       if is_process_running(pid) {
           Some(pid)
       } else {
           remove_pid(data_dir);
           None
       }
   }
   ```
3. `cargo test -p watch` passes

---

## Wave 2 — Composition Modules

### T07: Implement daemon module
**AC coverage:** AC9, AC10, AC15, AC16, AC17
**Files:** `crates/watch/src/daemon.rs`
**Depends on:** T05, T06

1. Write tests first:
   - `init_daemon_logging` creates rolling file appender in data dir
   - `stop_daemon` with valid PID sends SIGTERM (mock-able test: verify PID file removed)
   - `daemon_status` with no PID file returns `DaemonStatus::Stopped`
   - `daemon_status` with stale PID returns `DaemonStatus::Stopped` (cleans up)
2. Implement `DaemonStatus` enum:
   ```rust
   pub enum DaemonStatus {
       Running(u32),
       Stopped,
   }
   ```
3. Implement `daemon_status(data_dir: &Path) -> DaemonStatus`:
   - Delegate to `pid::check_daemon()`
4. Implement `stop_daemon(data_dir: &Path) -> Result<()>`:
   - Read PID, send `SIGTERM` via `libc::kill(pid, libc::SIGTERM)`, remove PID file
5. Implement `start_daemon(root: &Path, data_dir: &Path) -> Result<()>`:
   - Check if already running → error
   - Self-respawn via `Command::new(current_exe)` with `["watch", "--daemon-internal"]`
   - `stdin(Stdio::null())`, stdout/stderr to log file
   - `.process_group(0)` for signal isolation
   - Print PID, parent returns
6. Implement `run_daemon(root: &Path, data_dir: &Path, debounce_ms: u64) -> Result<()>`:
   - Called by `--daemon-internal` (the respawned child)
   - Write PID file
   - Init tracing with `RollingFileAppender` (daily, max 7, `daemon.YYYY-MM-DD.log`)
   - Register SIGTERM/SIGINT via `signal_hook::flag::register` → `AtomicBool`
   - Create `CodeGraphWatcher`, wire adapters (SqliteStore, RayonParseProvider, RealFileSystem, ShellGitProvider)
   - Event loop: receive batches from watcher, run `IndexUseCase::incremental_files()` for each batch
   - On shutdown flag: remove PID file, drop WorkerGuard (flushes logs)
7. Implement `run_foreground(root: &Path, data_dir: &Path, debounce_ms: u64) -> Result<()>`:
   - Same as `run_daemon` but tracing to stderr, no PID file, no self-respawn
8. `cargo test -p watch` passes

### T08: Implement freshness module
**AC coverage:** AC11
**Files:** `crates/watch/src/freshness.rs`
**Depends on:** T03, T06

1. Write tests first:
   - `ensure_fresh` with daemon running (mock PID check) → returns Ok immediately, no store calls
   - `ensure_fresh` with no daemon + no changes → returns Ok, no re-parse
   - `ensure_fresh` with no daemon + changed files → triggers incremental pipeline
2. Implement:
   ```rust
   pub fn ensure_fresh<S, P, F, G>(
       store: &S,
       parser: &P,
       fs: &F,
       git: &G,
       root: &Path,
       data_dir: &Path,
   ) -> Result<()>
   where
       S: GraphStore,
       P: ParseProvider,
       F: FileSystem,
       G: GitProvider,
   {
       // If daemon is running, graph is already fresh
       if pid::check_daemon(data_dir).is_some() {
           return Ok(());
       }
       // Run lazy staleness check
       let uc = IndexUseCase::new(store, parser, fs, git);
       // Ignore stats — this is a background freshness check
       let _ = uc.incremental_index(root)?;
       Ok(())
   }
   ```
   Note: The actual signature will use references/borrows that satisfy the trait bounds. The `IndexUseCase::new` takes owned values, so `ensure_fresh` may need to accept the use case directly or work with references. Adapt during implementation.
3. `cargo test -p watch` passes

### T09: CLI index command changes
**AC coverage:** AC12, AC13
**Files:** `crates/cli/src/commands/mod.rs`, `crates/cli/src/commands/index.rs`
**Depends on:** T03

1. Add flags to `IndexArgs` in `commands/mod.rs`:
   ```rust
   pub struct IndexArgs {
       /// Project path (default: current directory)
       pub path: Option<PathBuf>,
       /// Incremental update (only re-index changed files)
       #[arg(long)]
       pub incremental: bool,
       /// Specific files to re-index (implies --incremental)
       #[arg(long, value_delimiter = ',')]
       pub files: Option<Vec<PathBuf>>,
   }
   ```
2. Update `run_index` in `commands/index.rs` to branch on flags:
   ```rust
   if let Some(files) = &args.files {
       uc.incremental_files(&root, files.clone())
   } else if args.incremental {
       uc.incremental_index(&root)
   } else {
       uc.full_index(&root)
   }
   ```
3. `cargo build --workspace` compiles

---

## Wave 3 — CLI Wiring

### T10: CLI watch command
**AC coverage:** AC14, AC15, AC16, AC17
**Files:** `crates/cli/src/commands/mod.rs`, `crates/cli/src/commands/watch.rs`
**Depends on:** T07, T08

1. Update `Commands::Watch` in `commands/mod.rs`:
   ```rust
   /// Watch for file changes and re-index
   Watch(WatchArgs),
   ```
2. Add `WatchArgs`:
   ```rust
   #[derive(Debug, clap::Args)]
   pub struct WatchArgs {
       /// Run as background daemon
       #[arg(long)]
       pub daemon: bool,
       /// Show daemon status
       #[arg(long)]
       pub status: bool,
       /// Stop running daemon
       #[arg(long)]
       pub stop: bool,
       /// Internal flag: marks this process as the daemon child (hidden)
       #[arg(long, hide = true)]
       pub daemon_internal: bool,
   }
   ```
3. Create `commands/watch.rs`:
   ```rust
   pub fn run_watch(args: &WatchArgs) -> Result<()> {
       let root = find_project_root(&cwd)?;
       let data_dir = ensure_data_dir(&root)?;
       let config = load_config(&root)?;
       let debounce_ms = config.watch.as_ref().and_then(|w| w.debounce_ms).unwrap_or(100);

       if args.status {
           return show_status(&data_dir);
       }
       if args.stop {
           return daemon::stop_daemon(&data_dir);
       }
       if args.daemon {
           return daemon::start_daemon(&root, &data_dir);
       }
       if args.daemon_internal {
           return daemon::run_daemon(&root, &data_dir, debounce_ms);
       }
       // Default: foreground mode
       daemon::run_foreground(&root, &data_dir, debounce_ms)
   }
   ```
4. Add `pub mod watch;` to `commands/mod.rs`
5. `cargo build --workspace` compiles

### T11: Integrate `ensure_fresh` into `open_graph()`
**AC coverage:** AC18
**Files:** `crates/cli/src/commands/helpers.rs`
**Depends on:** T08

1. Modify `open_graph()` to call `ensure_fresh` after opening the store:
   ```rust
   pub fn open_graph() -> Result<(SqliteStore, PathBuf)> {
       let cwd = std::env::current_dir().map_err(|e| {
           CodeGraphError::FileSystem { path: ".".into(), source: e }
       })?;
       let root = find_project_root(&cwd)?;
       let db_path = root.join(".code-graph").join("graph.db");
       if !db_path.exists() {
           return Err(CodeGraphError::IndexNotBuilt);
       }
       let store = SqliteStore::open(&db_path)
           .map_err(|e| CodeGraphError::Storage(format!("{e}")))?;

       // Lazy freshness check — skips if daemon is running
       let data_dir = root.join(".code-graph");
       if let Err(e) = watch::freshness::ensure_fresh(/* adapters */, &root, &data_dir) {
           tracing::debug!("freshness check skipped: {e}");
       }

       Ok((store, root))
   }
   ```
   Note: `ensure_fresh` needs adapter instances. Wire `RealFileSystem`, `ShellGitProvider`, `RayonParseProvider` inline. If freshness check fails, log and continue (non-fatal — query should still work with potentially stale data).
2. All 8 query commands automatically get freshness with zero per-command changes
3. `cargo build --workspace` compiles

### T12: Config additions
**AC coverage:** AC6 (configurable debounce)
**Files:** `crates/cli/src/config.rs`

1. Add `WatchConfig` to `config.rs`:
   ```rust
   #[derive(Debug, Clone, Default, Deserialize)]
   pub struct WatchConfig {
       pub debounce_ms: Option<u64>,
   }
   ```
2. Add `pub watch: Option<WatchConfig>` to `CodeGraphConfig`
3. `cargo build --workspace` compiles

---

## Wave 4 — Integration

### T13: Wire watch handler into dispatcher + integration tests + clippy
**AC coverage:** AC20, AC21
**Files:** `crates/cli/src/lib.rs`, integration tests
**Depends on:** T09, T10, T11, T12

1. Update dispatcher in `lib.rs`:
   ```rust
   Commands::Watch(args) => commands::watch::run_watch(args),
   ```
2. Write integration tests:
   - `index_incremental_updates_changed_files`: create temp git repo, index, modify file, run `incremental_index`, verify only changed file re-parsed
   - `index_incremental_skips_unchanged`: index, run `incremental_index` with no changes, verify stats are zeros
   - `index_files_updates_specific_files`: create indexed project, `incremental_files` with one path, verify only that file updated
   - `watch_status_when_not_running`: `--status` reports stopped
   - `watch_stop_when_not_running`: `--stop` is no-op (no error)
   - `ensure_fresh_skips_when_daemon_running`: mock PID file with current process PID, verify no incremental run
3. `cargo test --workspace` passes (AC19)
4. `cargo clippy --workspace -- -Dwarnings` passes (AC20)
5. Remove `#[allow(dead_code)]` from `git` field in `IndexUseCase` (now used)

---

## Task Dependency Graph

```
T01 (GitProvider::modified_files) ─────┬──► T03 (incremental pipeline)
                                       │
                                       └──► T04 (ShellGitProvider::modified_files)

T02 (watch crate scaffold) ────────────┬──► T05 (watcher module)
                                       │
                                       └──► T06 (PID management)

T03 + T06 ─────────────────────────────┬──► T08 (freshness / ensure_fresh)
                                       │
T05 + T06 + T03 ──────────────────────►│──► T07 (daemon module)
                                       │
T03 ───────────────────────────────────►└──► T09 (CLI index --incremental)

T07 + T08 ─────────────────────────────────► T10 (CLI watch command)
T08 ───────────────────────────────────────► T11 (ensure_fresh in open_graph)
                                             T12 (config additions — independent)

T09 + T10 + T11 + T12 ────────────────────► T13 (wire + integration tests)
```

## Wave Summary

| Wave | Tasks | Parallelism |
|------|-------|-------------|
| **0** | T01, T02 | Fully parallel (different crates) |
| **1** | T03, T04, T05, T06 | Fully parallel (T03+T04 in domain/cli, T05+T06 in watch) |
| **2** | T07, T08, T09 | All 3 parallel (after Wave 1) |
| **3** | T10, T11, T12 | All 3 parallel (after Wave 2) |
| **4** | T13 | Sequential (after Wave 3) |

## Complexity Estimate

| Task | Size | Notes |
|------|------|-------|
| T01 | S | Trait method + mock refactor, ~30 lines |
| T02 | S-M | Cargo.toml + lib.rs + stubs + workspace wiring, ~50 lines |
| T03 | L | Incremental pipeline with hash check + dependent discovery + tests, ~250 lines |
| T04 | M | Git status parser + extension filter + tests, ~100 lines |
| T05 | M | Watcher struct + debounce + path filtering + tests, ~120 lines |
| T06 | S-M | PID read/write/check + stale detection + tests, ~80 lines |
| T07 | L | Daemon lifecycle + signal handling + event loop + log rotation, ~200 lines |
| T08 | S-M | Freshness check + daemon skip + tests, ~60 lines |
| T09 | S | CLI args + routing branch, ~40 lines |
| T10 | M | WatchArgs + handler dispatch + status display, ~100 lines |
| T11 | S-M | Modify open_graph + wire adapters, ~40 lines |
| T12 | S | Config struct + field, ~15 lines |
| T13 | M | Dispatcher wiring + integration tests + clippy fixes, ~150 lines |

**Total estimated:** ~1,235 lines of new/modified code + tests

## AC Traceability Matrix

| AC | Task | Verified By |
|----|------|-------------|
| AC1 | T01 | Test: MockGitProvider.modified_files returns configured paths |
| AC2 | T03 | Test: incremental_index detects + re-parses changed + dependents |
| AC3 | T03 | Test: incremental_files processes explicit list |
| AC4 | T03 | Test: unchanged hash → skip |
| AC5 | T03 | Test: deleted file (hash fails) → remove_file_data called |
| AC6 | T05, T12 | Test: watcher batches events with configurable debounce |
| AC7 | T05 | Test: should_ignore filters blocked directories, supported extensions only |
| AC8 | T06 | Test: PID write/read/stale detection |
| AC9 | T07 | Test: signal handler sets shutdown flag |
| AC10 | T07 | Test: RollingFileAppender with daily rotation configured |
| AC11 | T08 | Test: ensure_fresh skips when daemon alive |
| AC12 | T09 | Test: --incremental flag routes to incremental_index |
| AC13 | T09 | Test: --files flag routes to incremental_files |
| AC14 | T10 | Test: default watch runs foreground |
| AC15 | T10, T07 | Test: --daemon spawns via self-respawn |
| AC16 | T10, T07 | Test: --status reports daemon state |
| AC17 | T10, T07 | Test: --stop sends SIGTERM + removes PID |
| AC18 | T11 | Test: open_graph calls ensure_fresh |
| AC19 | T04 | Test: parse_git_status handles M/A/D/R/?? statuses + extension filter |
| AC20 | T13 | `cargo test --workspace` |
| AC21 | T13 | `cargo clippy --workspace -- -Dwarnings` |
