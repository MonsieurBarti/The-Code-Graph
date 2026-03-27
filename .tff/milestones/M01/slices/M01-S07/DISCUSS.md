# Discussing M01-S07: Incremental Updates & Watch Daemon

## 1. Slice Intent

M01-S07 delivers the `watch` crate — the sixth and final workspace crate — and implements the incremental indexing pipeline. After this slice, a user can run `code-graph index --incremental` to update only changed files (hash-based), `code-graph watch` for a live daemon that keeps the graph fresh via filesystem events, and queries transparently trigger lazy staleness checks when neither daemon nor hooks are active.

This is the slice that turns a "rebuild the world" indexer into a practical developer tool.

---

## 2. Challenging Assumptions

### A1: The `watch` crate needs to depend on both `parser` and `storage`

The design spec shows `watch → domain, parser, storage`. The watch crate needs to re-parse changed files (parser) and update the store (storage). But the domain already defines the `ParseProvider` and `GraphStore` port traits — the watch crate could depend only on domain and receive injected adapters.

**Challenge:** If watch depends on parser+storage directly, it bypasses hexagonal boundaries. If it depends only on domain ports, CLI must wire the adapters.

**Verdict:** The spec is explicit: `watch → domain, parser, storage`. This is pragmatic — the watch daemon runs as a standalone process and needs concrete implementations, not just port traits. CLI still wires the initial construction, but watch owns the event loop. Accept the spec's dependency graph.

### A2: Incremental re-parsing of dependents is 1-hop only

The spec says: "Find dependents (1 hop: files that import/call into changed file)". But what if file A imports B imports C, and C changes? With 1-hop, only B gets re-parsed, not A.

**Challenge:** Is 1-hop sufficient? If B's re-parse produces different symbols (because C's exports changed), A's edges into B could become stale.

**Counter-argument:** In practice, a changed file's exported API rarely changes on every edit. 1-hop covers the common case (implementation changes within a file). If the exported interface changes, the next lazy staleness check or full re-index catches the cascade. Over-recursing dependents would make incremental updates slow for files imported by many others. 1-hop is the right v0.1 trade-off.

### A3: `git status --porcelain` is the right staleness detector

The spec uses `git status --porcelain` to detect modified files cheaply. This only reports unstaged/staged changes vs HEAD — it misses:
- Files changed since the last index but already committed (HEAD moved)
- Untracked files that were previously indexed

**Challenge:** Should we also compare `git log --name-only HEAD~1..HEAD` to catch committed changes?

**Counter-argument:** The hash-based pipeline is the real correctness guarantee. `git status --porcelain` is a *fast pre-filter* to avoid hashing every tracked file. Even if it misses some changes, the hash check on the next full query or index run catches them. This is an optimization, not a correctness mechanism. Accept the spec's approach.

### A4: The daemon needs a Unix socket for health checks

The spec mentions `.code-graph/daemon.sock` for health checks. Is a socket necessary, or is a PID file + process check sufficient?

**Challenge:** Unix sockets add complexity (async I/O, protocol design). A PID file with `kill -0 $PID` process liveness check achieves "is daemon running?" without socket overhead.

**Verdict:** For v0.1, PID file + process liveness check is sufficient. The socket is nice-to-have for richer health info (e.g., "last update was 5s ago, 1200 files watched"). Defer socket to a later iteration — PID-only is simpler and the spec allows `--status` to work with just a process check.

### A5: Log rotation needs a custom implementation

The spec mentions "log rotation" for daemon logs. Is this a custom rotator, or can we use an existing tracing layer?

**Verdict:** Use `tracing-appender` which provides `RollingFileAppender` with daily/hourly rotation out of the box. No custom implementation needed.

### A6: The `--incremental` flag belongs on the `index` command

The spec shows `code-graph index --incremental` for hook-triggered updates. Currently `IndexUseCase` has a `full_index()` method and a stubbed `incremental_index()`. The CLI args need to add `--incremental` and optionally `--files` (for PostToolUse hook: update specific files).

**Verdict:** Clean fit. `index` gains `--incremental` and `--files <paths>`. Default (no flag) remains full index.

### A7: Lazy staleness should be transparent on every query

The spec's three-layer freshness model says queries auto-check staleness when no daemon is running. This means every `find`, `refs`, `impact`, etc. command should optionally trigger an incremental update before querying.

**Challenge:** This adds latency to every query command. The spec estimates ~50-100ms for typical edits, but it could be more for large changesets.

**Verdict:** The spec is clear. Add a `ensure_fresh()` check at the start of every query command. If the daemon PID file exists and process is alive, skip the check. Otherwise, run the lazy staleness pipeline. The 50-100ms cost is acceptable for correctness.

---

## 3. Surfacing Unknowns

| Unknown | Risk | Mitigation |
|---------|------|------------|
| `notify` crate cross-platform behavior (macOS FSEvents vs Linux inotify) | Low | We only target macOS initially; `notify` abstracts platform differences. Test on macOS. |
| Debounce implementation — does `notify` have built-in debounce? | Low | `notify` v6+ has `RecommendedWatcher` but debounce was removed in v5. We'll need a manual debounce layer (collect events for 100ms, then process batch). |
| Daemon backgrounding on macOS — `fork()` vs `Command::new` with `daemonize` crate | Medium | Research needed. `daemonize` crate handles double-fork, setsid, PID file. Alternatively, just run foreground and let the user background with `&` or `nohup`. |
| `--files` flag interaction with incremental pipeline | Low | When `--files` is provided, skip `git status` detection and directly hash-check + re-parse the specified files + their dependents. |
| GitProvider needs `git status --porcelain` method | Low | Currently `ShellGitProvider` has `diff_hunks()` and `changed_files()`. Need to add `modified_files()` → `git status --porcelain`. |
| Dependent file discovery requires reverse edge lookup | Low | `GraphStore::get_edges_to(target)` already exists. Query edges where target is a symbol in the changed file to find importing files. |
| Watch crate's dependency on parser and storage creates tight coupling | Low | Accepted per A1 — spec is explicit. Watch is a "composition crate" like CLI. |
| Graceful shutdown and PID cleanup on crash | Medium | Use a `Drop` guard or signal handler (SIGTERM/SIGINT) to clean up PID file. Stale PID detection: if PID file exists but process is dead, remove it and proceed. |

---

## 4. Scope Recommendation

### Option A: Full slice as spec'd (recommended)

**S07: Incremental Updates & Watch Daemon**

1. **Domain additions:**
   - `GitProvider` gains `modified_files()` method (git status --porcelain)
   - `IndexUseCase::incremental_index()` implementation — hash check, re-parse changed, re-parse dependents (1-hop), atomic store update
   - `IndexUseCase::incremental_files()` — same but for explicit file list (hook use case)
   - `ensure_fresh()` utility — lazy staleness check for query commands

2. **Watch crate (new):**
   - `notify`-based file watcher with 100ms manual debounce
   - Daemon lifecycle: start, stop, status via PID file
   - Log rotation via `tracing-appender`
   - Respects `.gitignore` + `.code-graphignore`
   - Triggers incremental pipeline on file changes

3. **CLI updates:**
   - `index --incremental` flag, `index --files <paths>` flag
   - `watch` command: foreground mode, `--daemon`, `--status`, `--stop`
   - Every query command calls `ensure_fresh()` before executing
   - `ShellGitProvider` gains `modified_files()` adapter

### Option B: Split incremental and watch

**S07a:** Incremental index pipeline only (no daemon). Delivers `index --incremental`, lazy staleness, `ensure_fresh()`.
**S07b:** Watch daemon. Depends on S07a.

**Trade-off:** The split adds process overhead but reduces per-slice complexity. However, the incremental pipeline and watch daemon share the same debounce/change-detection logic — splitting means building the pipeline twice or creating artificial interfaces between halves.

**Recommendation:** Option A. The watch daemon is fundamentally just "run the incremental pipeline when files change." They're the same concern and should ship together.

---

## 5. Complexity Classification

| Aspect | Rating | Justification |
|--------|--------|---------------|
| **Algorithmic** | Medium | Incremental pipeline is well-defined (hash check → re-parse → dependents). Debounce is simple timer logic. |
| **Integration** | High | New crate (`watch`), new deps (`notify`, `tracing-appender`, possibly `daemonize`), touches domain ports, CLI wiring, and every query command. |
| **Domain knowledge** | Medium | File watching and daemon patterns are well-understood. No exotic algorithms. |
| **Dependencies** | Medium | `notify` (file watching), `tracing-appender` (log rotation), `daemonize` or manual fork. All mature crates. |
| **Testing** | Medium-High | Daemon lifecycle tests, debounce timing tests, incremental correctness (changed file + dependents updated, untouched files preserved). |

**Overall: Medium-High complexity** — primarily from integration breadth (new crate + touching all query commands for ensure_fresh) and daemon lifecycle management.

---

## 6. Decisions

### Q1: Unix socket or PID-only for daemon management?
**Decision: PID-only for v0.1.** PID file at `.code-graph/daemon.pid` with stale-PID detection (`kill -0`). Socket deferred to v0.2 if richer health reporting is needed.

### Q2: Daemon backgrounding strategy?
**Decision: Research during research phase.** Evaluate `daemonize` crate vs. `fork()` + `setsid` vs. "just use nohup/systemd." Leaning toward `daemonize` crate for proper double-fork on Unix.

### Q3: Watch crate dependency graph?
**Decision: `watch → domain, parser, storage` as spec'd.** Watch is a composition crate that runs the full incremental pipeline autonomously.

### Q4: Debounce implementation?
**Decision: Manual debounce with `std::sync::mpsc` channel + timer thread.** Collect `notify` events for 100ms, deduplicate by path, then batch-process. Avoids async runtime dependency.

### Q5: How does `ensure_fresh()` integrate with query commands?
**Decision: Single function call at the start of every query handler.** Checks daemon PID → if alive, skip. If not, run `git status --porcelain` → hash check → incremental update for stale files. Returns early if graph is fresh.

### Q6: Setup command (Section 6.7) — in scope for S07?
**Decision: Defer to S08 (Agent Integration).** Setup installs Claude Code hooks, which is agent-specific functionality. S07 focuses on the incremental + watch infrastructure that hooks will call into. S08 wires the hooks.
