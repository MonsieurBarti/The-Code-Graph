# Research — M01-S07: Incremental Updates & Watch Daemon

## 1. Dependency Research

### 1.1 `notify` (File Watching)

**Version:** `notify = "8.2"` (latest stable)

**API summary:**
- `RecommendedWatcher` — platform-specific best backend (FSEvents on macOS, inotify on Linux)
- `Watcher` trait: `watch(path, RecursiveMode)`, `unwatch(path)`
- `EventHandler` trait: implemented automatically for `mpsc::Sender<Result<Event>>`
- Channel-based pattern is canonical: watcher sends events to `mpsc::Sender`, app thread receives via `mpsc::Receiver`

**Debounce:** Removed from core in v5. Two companion crates exist:
- `notify-debouncer-mini` — lightweight
- `notify-debouncer-full` (v0.7) — full-featured with rename stitching via file IDs

**Decision: Use `notify-debouncer-full`** rather than rolling manual debounce. It provides:
- Configurable timeout (`Duration::from_millis(100)`)
- File ID caching for rename tracking (critical on macOS where FSEvents can't track rename from/to)
- Emits `Vec<DebouncedEvent>` batches after timeout — natural fit for batch re-indexing
- Channel-based API, no async runtime needed

**Path filtering:** Not built-in. Filter in event handler:
```rust
fn should_ignore(path: &Path) -> bool {
    path.components().any(|c| {
        [".git", "target", "node_modules", ".code-graph"]
            .contains(&c.as_os_str().to_str().unwrap_or(""))
    })
}
```

**macOS gotchas:**
- FSEvents reports at directory level, not per-file — debouncer smooths this
- No precise rename tracking without file ID caching (debouncer-full solves this)
- `event.need_rescan()` indicates event loss — trigger full re-index when true
- Historical events may arrive on watcher startup — filter by timestamp or ignore initial burst

**Thread safety:** Watcher spawns internal thread. Events delivered to handler (must be `Send + 'static`). `mpsc::Sender` satisfies this. Watcher itself is NOT Send+Sync — own it on one thread.

### 1.2 Daemon Backgrounding

**Three approaches evaluated:**

| Approach | Pros | Cons |
|----------|------|------|
| `daemonize` crate | Full double-fork, PID file, setsid, builder API | Dormant maintenance, uses raw `fork()` (unsafe in multi-threaded) |
| Manual `nix::unistd::fork` | Full control, `nix` well-maintained | Boilerplate-heavy, easy to get wrong |
| **Self-respawn via `Command`** | No fork hazards, testable, stable Rust | Not "true" double-fork, needs internal flag |

**Decision: Self-respawn via `std::process::Command`.** Rationale:
- Avoids `fork()` in a potentially multi-threaded process (tree-sitter, rayon could have threads)
- `Command::spawn` does fork+exec under the hood — child is a clean process
- `process_group(0)` (stable since Rust 1.64) detaches from terminal signal group
- `pre_exec(|| { libc::setsid(); Ok(()) })` for full session detachment
- Parent prints PID and exits cleanly
- Child recognizes itself via internal `--daemon-internal` flag

**Implementation sketch:**
```rust
// code-graph watch --daemon
let child = Command::new(std::env::current_exe()?)
    .args(["watch", "--daemon-internal"])
    .stdin(Stdio::null())
    .stdout(File::create(log_path)?)
    .stderr(File::create(log_path)?)
    .process_group(0)
    .spawn()?;
println!("Daemon started (PID {})", child.id());
// Parent exits
```

### 1.3 PID File Management

**Best practices:**

1. **Startup sequence:**
   - Try to read existing PID file
   - If exists, check process liveness via `libc::kill(pid, 0)` (signal 0 = existence check)
   - If alive → abort with "already running" error
   - If stale → log warning, overwrite
   - Write current PID, optionally `flock(LOCK_EX | LOCK_NB)` for advisory lock

2. **Stale PID detection (no `nix` needed):**
   ```rust
   fn is_process_running(pid: u32) -> bool {
       unsafe { libc::kill(pid as i32, 0) == 0 }
   }
   ```

3. **Cleanup:** Signal handler removes PID file on SIGTERM/SIGINT. Stale detection handles crash case.

**PID file location:** `.code-graph/daemon.pid`

### 1.4 Signal Handling

**Decision: `signal-hook` with `flag::register`.**

- No async runtime needed
- `AtomicBool` flag polled in daemon loop — zero-allocation in signal context
- Supports SIGTERM + SIGINT
- Pattern:
  ```rust
  let shutdown = Arc::new(AtomicBool::new(false));
  signal_hook::flag::register(SIGTERM, Arc::clone(&shutdown))?;
  signal_hook::flag::register(SIGINT, Arc::clone(&shutdown))?;
  while !shutdown.load(Ordering::Relaxed) {
      // receive events, process batches
  }
  // cleanup: remove PID file, flush logs
  ```

### 1.5 Log Rotation

**Decision: `tracing-appender` with `RollingFileAppender`.**

- `Rotation::DAILY` — rotates once per day
- `max_log_files(7)` — auto-deletes oldest files beyond 7
- `non_blocking()` — background I/O thread, returns `WorkerGuard` (must outlive daemon)
- Dual-mode: file layer (daemon) + stderr layer (foreground)

**Setup:**
```rust
let appender = RollingFileAppender::builder()
    .rotation(Rotation::DAILY)
    .filename_prefix("daemon")
    .filename_suffix("log")
    .max_log_files(7)
    .build(data_dir)?;
let (non_blocking, _guard) = tracing_appender::non_blocking(appender);
```

**Key gotcha:** `WorkerGuard` must be stored in a named variable that lives for the daemon's lifetime. Binding to `_` drops immediately and loses buffered logs.

**File naming:** `daemon.2026-03-27.log`, `daemon.2026-03-26.log`, etc. (UTC timestamps)

### 1.6 Dependency Summary

```toml
# watch crate Cargo.toml
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
libc = "0.2"              # for setsid in pre_exec, kill(pid,0)
```

No `daemonize`, no `nix`, no async runtime.

---

## 2. Integration Point Analysis

### 2.1 What Already Exists (No Changes Needed)

| Component | Location | Why It's Ready |
|-----------|----------|---------------|
| `FileNode.hash` | `model.rs:131-136` | SHA-256 hash field already in domain model |
| `FileSystem::file_hash()` | `ports.rs` + `adapters/fs.rs` | Port + adapter already compute SHA-256 |
| `files.hash` column | `schema.rs` | SQLite schema already stores hash |
| `GraphStore::all_files()` | `ports.rs` | Returns all FileNodes with hashes |
| `GraphStore::get_file(path)` | `ports.rs` | Gets single file's stored hash |
| `GraphStore::store_file_data()` | `ports.rs` | Atomic upsert of file+symbols+edges |
| `GraphStore::remove_file_data()` | `ports.rs` | Cascade delete for removed/changed files |
| `GraphStore::get_edges_to(target)` | `ports.rs` | Reverse edge lookup for finding dependents |
| `find_project_root()` | `project.rs` | Project detection with blocklist |
| `ensure_data_dir()` | `project.rs` | Creates `.code-graph/` directory |
| `RealFileSystem` | `adapters/fs.rs` | File listing with gitignore support |
| `ShellGitProvider` | `adapters/git.rs` | Git shell-out with `run_git()` helper |
| `RayonParseProvider` | `parser` crate | Multi-threaded parse+resolve |
| `metadata` table | `schema.rs` | Key-value store for schema version (reusable for index state) |

### 2.2 What Needs to Change

**Domain ports (`crates/domain/src/ports.rs`):**
- Add `GitProvider::modified_files()` → returns files with uncommitted changes
- Signature: `fn modified_files(&self) -> Result<Vec<PathBuf>>`
- No `base` parameter needed — `git status --porcelain` reports working tree state vs HEAD

**IndexUseCase (`crates/domain/src/use_cases/index.rs`):**
- Implement `incremental_index(root)` — the core pipeline:
  1. Get modified files from git status
  2. Hash-check each against stored hash
  3. Re-parse changed files
  4. Find 1-hop dependents via `get_edges_to()`
  5. Re-parse dependents
  6. Atomic store updates
- Add `incremental_files(root, files)` — explicit file list variant for hooks

**CLI args (`crates/cli/src/commands/mod.rs`):**
- `IndexArgs`: add `--incremental` flag, `--files <paths>` option
- `Commands::Watch`: change from unit variant to `Watch(WatchArgs)`
- `WatchArgs`: `--daemon`, `--status`, `--stop`, `--daemon-internal` (hidden)

**CLI dispatch (`crates/cli/src/lib.rs`):**
- Route `Commands::Watch(args)` to `commands::watch::run_watch()`

**Query commands (8 files):**
- All query commands call `open_graph()` → insert `ensure_fresh()` after opening
- Better approach: modify `open_graph()` itself to call `ensure_fresh()` internally
- This way, a single change point covers all 8 commands
- Skip if daemon PID file exists and process is alive

**ShellGitProvider (`crates/cli/src/adapters/git.rs`):**
- Implement `modified_files()`: shell out to `git status --porcelain`
- Parse porcelain format: `XY filename` where X=index status, Y=worktree status
- Include: modified (M), added (A), deleted (D), renamed (R) files
- Filter to supported extensions only

**Config (`crates/cli/src/config.rs`):**
- Add `WatchConfig` with `debounce_ms` setting (default 100)

### 2.3 New Files to Create

| File | Purpose |
|------|---------|
| `crates/watch/Cargo.toml` | Watch crate manifest |
| `crates/watch/src/lib.rs` | Crate root, re-exports |
| `crates/watch/src/watcher.rs` | `notify` watcher + debounce event loop |
| `crates/watch/src/daemon.rs` | Daemon lifecycle: start, stop, status, PID management |
| `crates/watch/src/freshness.rs` | `ensure_fresh()` logic — lazy staleness check |
| `crates/cli/src/commands/watch.rs` | CLI command handler for `watch` |

### 2.4 Workspace Cargo.toml

Add `"crates/watch"` to workspace members:
```toml
members = ["crates/domain", "crates/storage", "crates/parser", "crates/watch", "crates/cli", "crates/binary"]
```

CLI crate adds dependency:
```toml
watch = { path = "../watch" }
```

---

## 3. Architecture Decisions

### 3.1 Incremental Pipeline Flow

```
ensure_fresh() or index --incremental
  │
  ├─ Check daemon PID → if alive, skip (daemon keeps graph fresh)
  │
  ├─ git status --porcelain → Vec<PathBuf> of modified files
  │
  ├─ For each modified file:
  │   ├─ fs.file_hash(path) → current_hash
  │   ├─ store.get_file(path) → stored_hash
  │   ├─ current_hash == stored_hash? → skip
  │   └─ Changed! → add to re-parse set
  │
  ├─ For each file in re-parse set:
  │   ├─ Find dependents: store.get_edges_to(file_qualified_name)
  │   │   → files that import/call into this file
  │   └─ Add dependent files to re-parse set (1-hop only)
  │
  ├─ Deduplicate re-parse set
  │
  ├─ parser.parse_and_resolve(re_parse_set, root)
  │
  ├─ For each result:
  │   ├─ store.remove_file_data(path)  ← clean slate
  │   └─ store.store_file_data(file, symbols, edges)
  │
  └─ Return IndexStats { files_indexed: changed_count, ... }
```

### 3.2 Watch Daemon Event Loop

```
code-graph watch [--daemon]
  │
  ├─ find_project_root() → root
  ├─ ensure_data_dir() → .code-graph/
  ├─ Check/write PID file
  ├─ Initialize tracing (file or stderr based on mode)
  ├─ Wire adapters: SqliteStore, RayonParseProvider, RealFileSystem, ShellGitProvider
  │
  ├─ Create notify-debouncer-full with 100ms timeout
  ├─ watcher.watch(root, Recursive)
  ├─ Register SIGTERM/SIGINT → shutdown flag
  │
  └─ Loop while !shutdown:
      ├─ rx.recv_timeout(1s) → batch of DebouncedEvents
      ├─ Filter: ignore .git/, target/, node_modules/, .code-graph/
      ├─ Filter: only supported extensions
      ├─ Deduplicate by path
      ├─ For each changed path:
      │   ├─ Hash check against store
      │   ├─ Find dependents (1-hop)
      │   └─ Batch into re-parse set
      ├─ parse_and_resolve(re_parse_set)
      ├─ Update store atomically
      └─ Log: "Updated N files in Xms"

  On shutdown:
  ├─ Remove PID file
  └─ Flush logs (WorkerGuard drop)
```

### 3.3 `ensure_fresh()` Integration Strategy

**Modify `open_graph()` to auto-freshen** rather than touching all 8 query commands:

```rust
pub fn open_graph() -> Result<(SqliteStore, PathBuf)> {
    let root = find_project_root(&std::env::current_dir()?)?;
    let db_path = root.join(".code-graph").join("graph.db");
    if !db_path.exists() {
        return Err(CodeGraphError::IndexNotBuilt);
    }
    let store = SqliteStore::open(&db_path)?;
    ensure_fresh(&store, &root)?;  // <-- single insertion point
    Ok((store, root))
}
```

This gives all query commands automatic freshness with zero per-command changes.

### 3.4 `modified_files()` — Git Status Parsing

`git status --porcelain` output format: `XY <path>` where:
- X = index status, Y = worktree status
- `M` = modified, `A` = added, `D` = deleted, `R` = renamed, `?` = untracked

For incremental indexing, we care about:
- `?? <path>` — new untracked file (if it has a supported extension)
- ` M <path>` — modified in worktree
- `M  <path>` — modified in index (staged)
- `MM <path>` — modified in both
- ` D <path>` — deleted in worktree
- `A  <path>` — newly added to index

We do NOT need the `base` parameter from the discuss phase — `git status --porcelain` always reports vs HEAD. Simplify signature to `fn modified_files(&self) -> Result<Vec<PathBuf>>`.

---

## 4. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Debouncer crate version incompatibility with notify 8 | Low | Medium | Pin both; they're designed together |
| macOS FSEvents event loss | Low | Low | `need_rescan()` → trigger full re-index |
| Daemon self-respawn fails to detach properly | Medium | Low | Integration test; fallback: foreground-only for v0.1 |
| `ensure_fresh()` adds visible latency to queries | Medium | Medium | Time it; if >200ms, log warning and suggest daemon |
| Dependent discovery via `get_edges_to()` is slow for large graphs | Low | Medium | Edges table has index on target; SQLite is fast |
| PID file race condition (two daemons starting simultaneously) | Low | Low | Advisory flock prevents double-start |

---

## 5. Open Questions Resolved

| Question (from DISCUSS.md) | Resolution |
|---------------------------|-----------|
| Q2: Daemon backgrounding strategy | **Self-respawn via `Command`** — avoids fork hazards, testable, stable Rust |
| Q4: Debounce implementation | **`notify-debouncer-full`** — battle-tested, handles rename stitching, no async |
| `modified_files()` signature | **No `base` param** — `git status --porcelain` always reports vs HEAD |
| New trait methods needed on GraphStore? | **None** — existing methods sufficient |
| Watch crate vs CLI module? | **New `crates/watch/` crate** per spec's 6-crate architecture |
