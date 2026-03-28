# Verification — M01-S07: Incremental Updates & Watch Daemon

**Date:** 2026-03-27
**Branch:** `slice/M01-S07`
**Build evidence:** `cargo test --workspace` -> 416 passed (11 suites), `cargo clippy --workspace -- -Dwarnings` -> clean

---

## Acceptance Criteria Verdicts

| AC | Criterion | Verdict | Evidence |
|----|-----------|---------|----------|
| AC1 | `GitProvider::modified_files()` returns uncommitted changes | **PASS** | Trait: `ports.rs:46`; Mock: `test_support.rs:207-209`; Impl: `git.rs:169-172`; Tests: `git.rs:304-346` (8 cases) |
| AC2 | `incremental_index()` — git status + hash check + 1-hop dependents + atomic store | **PASS** | Impl: `index.rs:60-149` (3-phase pipeline); Tests: `index.rs:265-379` (skip unchanged, reparse changed, 1-hop dependents, no modifications) |
| AC3 | `incremental_files()` processes explicit file list through same pipeline | **PASS** | Impl: `index.rs:65-67` delegates to `run_incremental`; Tests: `index.rs:395-441` (explicit list, skip unchanged in list) |
| AC4 | Unchanged files skipped (no re-parse, no store update) | **PASS** | Logic: `index.rs:84-87` hash comparison + continue; Tests: `index.rs:265-282` (stats.files_indexed == 0), `index.rs:421-441` |
| AC5 | Deleted files removed via `remove_file_data()` | **PASS** | Error handling: `index.rs:76-82`; Cleanup: `graph_store.rs:575-601` (atomic edge+file delete); Tests: `index.rs:444-461`, `graph_store.rs:803-821` |
| AC6 | `notify-debouncer-full` watcher with configurable debounce (default 100ms) | **PASS** | Impl: `watcher.rs:31-32` (`new_debouncer(Duration::from_millis(...))`); Dep: `watch/Cargo.toml` |
| AC7 | Path filtering ignores `.git/`, `target/`, `node_modules/`, `.code-graph/`; supported extensions only | **PASS** | Impl: `watcher.rs:71-84` (`should_ignore`, `has_supported_extension`); Tests: `watcher.rs:86-140` (8 cases) |
| AC8 | PID file at `.code-graph/daemon.pid` — write, stale detection, cleanup | **PASS** | Impl: `pid.rs:5-47` (`write_pid`, `read_pid`, `is_process_running`, `check_daemon`); Tests: `pid.rs:49-120` (10 cases) |
| AC9 | Signal handling — SIGTERM/SIGINT trigger graceful shutdown | **PASS** | Impl: `daemon.rs:133-136` (`signal_hook::flag::register`); Shutdown: `daemon.rs:149-178` (PID removal, log flush) |
| AC10 | Log rotation via `tracing-appender` with daily rotation | **PASS** | Impl: `daemon.rs:88-89` (`rolling::daily()`); Dep: `watch/Cargo.toml:14` |
| AC11 | `ensure_fresh()` — daemon alive -> skip, else lazy staleness | **PASS** | Impl: `freshness.rs:12-78` (PID check -> short circuit, else incremental); Tests: `freshness.rs:80-135` (3 cases) |
| AC12 | `index --incremental` routes to incremental pipeline | **PASS** | Flag: `mod.rs:74-75`; Routing: `index.rs:38-44`; Tests: `index.rs:93-126` |
| AC13 | `index --files <paths>` routes to incremental for explicit files | **PASS** | Flag: `mod.rs:77-79` (`value_delimiter = ','`); Routing: `index.rs:38-39`; Tests: `index.rs:160-185` |
| AC14 | `watch` runs foreground watcher (logs to stderr) | **PASS** | Handler: `watch.rs:48-53`; Impl: `daemon.rs:98-116` (`tracing_subscriber::fmt().with_writer(std::io::stderr)`) |
| AC15 | `watch --daemon` self-respawns, prints PID, parent exits | **PASS** | Impl: `daemon.rs:45-69` (`Command::new(exe).args(["watch", "--daemon-internal"]).process_group(0).spawn()`) |
| AC16 | `watch --status` reports daemon status (running/stopped) | **PASS** | Handler: `watch.rs:56-66`; Impl: `daemon.rs:20-25` (`DaemonStatus` enum) |
| AC17 | `watch --stop` sends SIGTERM + removes PID file | **PASS** | Impl: `daemon.rs:27-43` (`libc::kill(pid, libc::SIGTERM)` + `pid::remove_pid`) |
| AC18 | All query commands auto-call `ensure_fresh()` via `open_graph()` | **PASS** | Impl: `helpers.rs:10-32` (ensure_fresh after store open, non-fatal on error); 8 query commands verified: find, refs, callers, callees, search, impact, diff, stats |
| AC19 | `ShellGitProvider::modified_files()` parses porcelain (M, A, D, R, ??) + extension filter | **PASS** | Impl: `git.rs:122-172` (`parse_git_status` + `has_supported_extension`); Tests: `git.rs:303-360` (8 cases: M, ??, D, MM, R rename, multi-line, empty, extension filter) |
| AC20 | `cargo test --workspace` passes | **PASS** | 416 tests passed across 11 suites (0.23s) |
| AC21 | `cargo clippy --workspace -- -Dwarnings` passes | **PASS** | No issues found |

---

## Summary

**Verdict: PASS** — All 21 acceptance criteria met with evidence.

**Test coverage highlights:**
- Domain: incremental pipeline tested for skip-unchanged, reparse-changed, 1-hop-dependents, deleted-file-removal, explicit-file-list
- Watch: watcher path filtering (8 tests), PID management (10 tests), freshness (3 tests), daemon lifecycle (4 tests)
- CLI: index flags routing (3 tests), watch subcommands, 8 query commands verified for ensure_fresh integration
- Adapter: git status parsing (8 tests covering all porcelain statuses + extension filtering)

**Architecture notes:**
- New `watch` crate (6th workspace member) follows hexagonal architecture — depends only on `domain` for ports (traits) and error types. All public functions use generic bounds (`<S: GraphStore, P: ParseProvider, F: FileSystem, G: GitProvider>`). Concrete adapters are wired at the CLI composition root (`commands/watch.rs`, `helpers.rs`). Phantom `parser`/`storage` deps removed during verification.
- `ensure_fresh()` is non-fatal by design — stale query data preferred over error
- Self-respawn daemon avoids `fork()` in multi-threaded process (tree-sitter, rayon)
- PID-only lifecycle management (no Unix socket) per v0.1 scope
