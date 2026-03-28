# Spec — M01-S07: Incremental Updates & Watch Daemon

## Problem

After S06, every `code-graph index` rebuilds the entire graph from scratch. For a developer editing a handful of files, this means re-parsing and re-storing thousands of unchanged files on every invocation. Query commands return stale results if the user forgets to re-index. There is no way to keep the graph automatically fresh. S07 delivers incremental indexing and a watch daemon so the tool scales to real developer workflows.

## Approach

### Domain Changes

- Add `modified_files(&self) -> Result<Vec<PathBuf>>` to `GitProvider` trait. Returns files with uncommitted changes via `git status --porcelain`. No `base` parameter — porcelain always reports vs HEAD.
- Implement `IndexUseCase::incremental_index(root)` — the core incremental pipeline:
  1. Get modified files from `git.modified_files()`
  2. Hash-check each against stored hash via `fs.file_hash()` + `store.get_file()`
  3. Collect actually-changed files into re-parse set
  4. Find 1-hop dependents via `store.get_edges_to()` — files that import/call into changed files
  5. Re-parse changed + dependent files via `parser.parse_and_resolve()`
  6. Atomic store update: `remove_file_data()` then `store_file_data()` per file
- Add `IndexUseCase::incremental_files(root, files)` — same pipeline but with an explicit file list instead of git status (for hook use case: `code-graph index --files <paths>`).
- Deleted files (hash fails): `remove_file_data()` from store.
- 1-hop dependent discovery is sufficient for v0.1. If a transitive dependency's exported API changes, the next lazy check or full re-index catches it.

### Git Adapter

- Implement `ShellGitProvider::modified_files()`: shell out to `git status --porcelain`, parse output.
- Porcelain format: `XY <path>` where X=index status, Y=worktree status. Handle: `M` (modified), `A` (added), `D` (deleted), `R` (renamed — use new name after ` -> `), `??` (untracked).
- Filter results to supported extensions only (ts, tsx, js, jsx, rs, py, go).

### Watch Crate (new)

New `crates/watch/` crate — the 6th workspace crate per the design spec's architecture. Dependencies: `domain`, `parser`, `storage`, `notify 8`, `notify-debouncer-full 0.7`, `signal-hook 0.3`, `tracing-appender 0.2`, `libc 0.2`, `ignore 0.4`. No async runtime.

**Watcher module (`watcher.rs`):**
- `notify-debouncer-full` with configurable debounce timeout (default 100ms). Provides file ID caching for rename tracking on macOS (FSEvents limitation).
- Watch project root recursively.
- Path filtering: ignore `.git/`, `target/`, `node_modules/`, `.code-graph/`. Only pass through supported extensions.
- On `event.need_rescan()`: log warning, trigger full re-index.
- Batched events sent via `mpsc::Sender<Vec<PathBuf>>`.

**PID management (`pid.rs`):**
- PID file at `.code-graph/daemon.pid`.
- Write PID on daemon start, read on status/stop.
- Stale detection: `libc::kill(pid, 0)` — signal 0 checks process existence without actually signaling.
- Cleanup: remove PID file on shutdown. Stale PID file (process dead) is logged and overwritten on next start.

**Daemon module (`daemon.rs`):**
- Self-respawn for backgrounding: `Command::new(current_exe()).args(["watch", "--daemon-internal"]).process_group(0).spawn()`. Parent prints PID and exits. Child runs event loop. Avoids `fork()` hazards in multi-threaded process (tree-sitter, rayon threads).
- Hidden `--daemon-internal` flag distinguishes respawned child from user invocation.
- Signal handling: `signal-hook::flag::register` sets `AtomicBool` on SIGTERM/SIGINT. Event loop polls this flag.
- Log rotation: `tracing-appender::RollingFileAppender` with `Rotation::DAILY`, max 7 files, `daemon.YYYY-MM-DD.log` naming. `WorkerGuard` must outlive daemon loop.
- Event loop: receive debounced batches → run `IndexUseCase::incremental_files()` → log stats.

**Freshness module (`freshness.rs`):**
- `ensure_fresh()` — lazy staleness check for query commands.
- Logic: check daemon PID → if alive, skip (daemon keeps graph fresh). Otherwise, run `incremental_index()`.
- Integrated into `open_graph()` so all 8 query commands get automatic freshness with zero per-command changes.
- Non-fatal: if freshness check fails, log at debug level and continue with potentially stale data.

### CLI Changes

- `IndexArgs` gains `--incremental` (bool) and `--files <paths>` (comma-separated `Vec<PathBuf>`). `--files` implies incremental.
- `Commands::Watch` changes from unit variant to `Watch(WatchArgs)`.
- `WatchArgs`: `--daemon` (background), `--status` (check), `--stop` (terminate), `--daemon-internal` (hidden).
- New `commands/watch.rs` handler dispatches to daemon module functions.
- `open_graph()` in `helpers.rs` calls `ensure_fresh()` after opening store.

### Configuration

- Add `WatchConfig { debounce_ms: Option<u64> }` to `CodeGraphConfig`.
- Config key: `[watch] debounce_ms = 100` in `.code-graph/config.toml`.

## Acceptance Criteria

### Domain
- **AC1**: `GitProvider::modified_files()` returns files with uncommitted changes.
- **AC2**: `IndexUseCase::incremental_index(root)` detects changed files via git status, hash-checks against stored hashes, re-parses changed files + 1-hop dependents, atomically updates store. Returns `IndexStats`.
- **AC3**: `IndexUseCase::incremental_files(root, files)` processes an explicit file list through the same pipeline.
- **AC4**: Files whose current hash matches stored hash are skipped — no re-parse, no store update.
- **AC5**: Deleted files (hash computation fails) are removed from store via `remove_file_data()`.

### Watch Crate
- **AC6**: `notify-debouncer-full` watcher batches filesystem events with configurable debounce (default 100ms).
- **AC7**: Path filtering ignores `.git/`, `target/`, `node_modules/`, `.code-graph/`. Only supported extensions pass through.
- **AC8**: PID file at `.code-graph/daemon.pid` — written on start, stale detection via `kill(pid, 0)`, cleanup on shutdown.
- **AC9**: SIGTERM/SIGINT trigger graceful shutdown: remove PID file, flush logs.
- **AC10**: Log rotation via `tracing-appender` with daily rotation, max 7 files.
- **AC11**: `ensure_fresh(store, root, data_dir)` checks daemon PID → if alive, skip. Otherwise run lazy staleness pipeline via `incremental_index`.

### CLI
- **AC12**: `code-graph index --incremental` runs incremental pipeline instead of full index.
- **AC13**: `code-graph index --files <paths>` runs incremental pipeline for specified files only.
- **AC14**: `code-graph watch` runs foreground watcher (logs to stderr).
- **AC15**: `code-graph watch --daemon` spawns background daemon via self-respawn, prints PID, parent exits.
- **AC16**: `code-graph watch --status` reports daemon status (running with PID, or stopped).
- **AC17**: `code-graph watch --stop` sends SIGTERM to daemon and removes PID file.
- **AC18**: All query commands auto-call `ensure_fresh()` via `open_graph()` — zero per-command changes.

### Adapter
- **AC19**: `ShellGitProvider::modified_files()` parses `git status --porcelain` output correctly (M, A, D, R, ?? statuses). Filters to supported extensions.

### Quality
- **AC20**: `cargo test --workspace` passes.
- **AC21**: `cargo clippy --workspace -- -Dwarnings` passes.

## Non-Goals

- Unix socket for daemon health checks — PID-only is sufficient for v0.1. Socket deferred to v0.2.
- `setup` command / Claude Code hooks installation — deferred to S08 (Agent Integration).
- Multi-hop dependent re-parsing — 1-hop only. Transitive staleness caught on next check.
- Async runtime — all I/O is synchronous with `mpsc` channels.
- Windows support — `libc::kill`, `process_group(0)`, signal handling are Unix-only. macOS + Linux only for v0.1.
- `code-graph index --watch` (combined index + watch) — watch is a separate command.
- `.code-graphignore` support in watcher — respects `.gitignore` only for v0.1.

## Design Notes

- **Watch depends on `parser` + `storage` directly** (not just domain ports). This is intentional — watch is a "composition crate" like CLI that runs the full pipeline autonomously. CLI still wires initial construction for the foreground path.
- **`ensure_fresh()` failure is non-fatal.** A query with stale data is better than a query that errors out because the freshness check hit a git issue. Log at debug, continue.
- **`notify-debouncer-full` over manual debounce.** Research confirmed `notify` v5+ removed built-in debounce. The companion `notify-debouncer-full` crate handles file ID caching (critical for macOS FSEvents rename tracking) and emits batched events — natural fit for batch re-indexing.
- **Self-respawn over `fork()`/`daemonize`.** A Rust process may have rayon/tree-sitter threads. `fork()` in a multi-threaded process is undefined behavior. `Command::spawn` does fork+exec under the hood, giving the child a clean process. `process_group(0)` detaches from terminal signal group.
- **`WorkerGuard` must be a named variable** (not `_`). Binding to `_` drops immediately, losing buffered log output.

## Testing Strategy

### Unit Tests
- `modified_files` parser: porcelain output for M, A, D, R, ??, multi-line, renames, empty output, extension filtering.
- Incremental pipeline: unchanged files skipped, changed files re-parsed, dependents discovered (1-hop), deleted files removed.
- PID management: write/read/stale detection/cleanup.
- Path filtering: `.git/`, `target/`, `node_modules/`, `.code-graph/` ignored; supported extensions pass.

### Integration Tests
- Create temp git repo, index, modify file, run incremental, verify only changed file re-parsed.
- Index with no changes → stats are zeros.
- `--files` with explicit path → only that file updated.
- Watch status/stop when no daemon running → graceful behavior.
- `ensure_fresh` skips when daemon PID exists.
