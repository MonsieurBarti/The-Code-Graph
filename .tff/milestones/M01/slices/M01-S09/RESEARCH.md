# Research — M01-S09: Eval Framework & CI/CD

## 1. Dependency Research

### 1.1 Eval Crate Dependencies

**No new external crate dependencies required.** The eval crate reuses existing workspace dependencies.

| Dependency | Source | Purpose | Notes |
|------------|--------|---------|-------|
| domain | `path = "../domain"` | Graph types, use cases (Index, Query, Impact), error types | Already available |
| parser | `path = "../parser"` | Tree-sitter parsing + import resolution pipeline | Already available |
| storage | `path = "../storage"` | SqliteStore for isolated per-repo eval databases | Already available |
| serde | `1` + `derive` | JSON manifest deserialization | Already in workspace |
| serde_json | `1` | Manifest parsing, report JSON output | Already in workspace |
| tracing | `0.1` | Progress logging during eval runs | Already in workspace |

**Dev-dependencies:** None needed in eval crate itself (tests run through CLI integration).

**Proposed `crates/eval/Cargo.toml`:**
```toml
[package]
name = "eval"
version = "0.1.0"
edition = "2021"

[dependencies]
domain = { path = "../domain" }
parser = { path = "../parser" }
storage = { path = "../storage" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
```

### 1.2 Git Clone Strategy

**Decision: `std::process::Command` (shell out to git), not `git2` crate.**

Rationale:
- `git2` is a heavy C dependency (libgit2) that complicates cross-compilation (especially aarch64-linux via cross-rs)
- The existing `ShellGitProvider` in `cli/src/adapters/git.rs` already establishes the shell-out pattern
- SPEC explicitly mentions `git clone --depth 1 --branch <revision>` suggesting this approach
- Git is universally available on dev machines and CI runners
- No `git2` dependency anywhere in the current workspace

**Clone command:**
```rust
std::process::Command::new("git")
    .args(["clone", "--depth", "1", "--branch", &revision, &url, path_str])
    .output()
```

### 1.3 Cache Directory Resolution

**XDG_CACHE_HOME with `~/.cache` fallback:**
```rust
fn eval_cache_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(xdg).join("code-graph-eval"));
    }
    let home = std::env::var("HOME")
        .map_err(|_| CodeGraphError::Other("HOME not set".into()))?;
    Ok(PathBuf::from(home).join(".cache").join("code-graph-eval"))
}
```

Consistent with existing `HOME` usage in `project.rs:26`. No new dependency (no `dirs` crate) — macOS/Linux only per spec.

**Cache validation:** Check directory exists + `.revision` marker file matches expected revision. Simple, git-independent, explicit.

**`--no-cache`:** Delete cache directory before cloning (`std::fs::remove_dir_all`).

### 1.4 CLI Integration — `eval` Crate as Dependency

CLI already imports domain, parser, storage, watch. Adding eval follows the same pattern:

```toml
# crates/cli/Cargo.toml — add:
eval = { path = "../eval" }
```

CLI dispatch update in `lib.rs:27`:
```rust
// From:
Commands::Eval => commands::stubs::not_implemented("eval"),
// To:
Commands::Eval(args) => commands::eval::run_eval(args, output_format),
```

### 1.5 Dependency Summary

**Zero new external crates.** Only workspace-internal dependency wiring:
- Root `Cargo.toml`: add `"crates/eval"` to workspace members
- `crates/cli/Cargo.toml`: add `eval = { path = "../eval" }`
- New `crates/eval/Cargo.toml`: depends on domain, parser, storage, serde, serde_json, tracing

---

## 2. Integration Point Analysis

### 2.1 What Already Exists (No Changes Needed)

| Component | Location | Why It's Ready |
|-----------|----------|---------------|
| `find_project_root()` | `project.rs:6-21` | Project detection for eval repos |
| `ShellGitProvider` pattern | `adapters/git.rs` | Precedent for shelling out to git |
| `RayonParseProvider` | `adapters/parse.rs` | Parallel parsing adapter for IndexUseCase |
| `RealFileSystem` | `adapters/fs.rs` | File discovery adapter |
| `SqliteStore::open()` | `storage/src/lib.rs` | Creates isolated DBs per eval repo |
| `IndexUseCase::full_index()` | `domain/use_cases/index.rs` | Full index pipeline (parse -> resolve -> store) |
| `QueryUseCase::search()` | `domain/use_cases/query.rs` | FTS5 search for eval queries |
| `ImpactUseCase::blast_radius()` | `domain/use_cases/impact.rs` | Blast radius analysis for eval scenarios |
| `Displayable` trait | `cli/src/output.rs` | Three-format output (compact/table/json) |
| `OutputFormat::from_flags()` | `cli/src/output.rs` | Format selection pattern |
| `Commands::Eval` enum variant | `commands/mod.rs:66` | Stub already in place (unit variant, no args) |
| `not_implemented("eval")` | `lib.rs:27` | Current dispatch target to replace |
| Eval test case | `commands/mod.rs:241` | `vec!["code-graph", "eval"]` already in parse tests |

### 2.2 What Needs to Change

**1. CLI args (`commands/mod.rs:66`):**
- Change `Eval,` (unit variant) to `Eval(EvalArgs)`
- Add `EvalArgs` struct with `--suite`, `--no-cache`, `--json`, `--table` args
- Update `all_subcommands_parse` test (line 241): `vec!["code-graph", "eval"]` must become `vec!["code-graph", "eval", "--suite", "search"]` or similar

**2. CLI dispatch (`lib.rs:27`):**
- Change `Commands::Eval => commands::stubs::not_implemented("eval")` to `Commands::Eval(args) => commands::eval::run_eval(args, output_format)`

**3. Module registration (`commands/mod.rs`):**
- Add `pub mod eval;` to module declarations (line 1-14 area)

**4. Root Cargo.toml:**
- Add `"crates/eval"` to workspace members list

**5. CLI Cargo.toml:**
- Add `eval = { path = "../eval" }` to dependencies

### 2.3 New Files to Create

| File | Purpose |
|------|---------|
| `crates/eval/Cargo.toml` | Crate manifest |
| `crates/eval/src/lib.rs` | Public API: `run_suite()`, `SuiteResult` |
| `crates/eval/src/metrics.rs` | Pure metric functions: MRR, precision@k, recall, F1 |
| `crates/eval/src/dataset.rs` | Manifest parsing, repo clone/cache management |
| `crates/eval/src/runner.rs` | Orchestration: index repo -> run queries -> collect results |
| `crates/eval/src/report.rs` | Format results (compact/table/json via Displayable) |
| `crates/cli/src/commands/eval.rs` | CLI command handler: parse args, delegate to eval crate |
| `eval/suites/search/manifest.json` | Search suite repo manifest |
| `eval/suites/search/queries/*.json` | Search queries per language (5 files) |
| `eval/suites/impact/manifest.json` | Impact suite repo manifest |
| `eval/suites/impact/queries/*.json` | Impact scenarios per language (5 files) |
| `lefthook.yml` | Git hooks configuration |
| `.github/workflows/ci.yml` | PR check workflow |
| `.github/workflows/release.yml` | Release build + publish workflow |

### 2.4 Existing Indexing Pipeline (Eval Reuses This)

The eval runner calls the same pipeline used by `code-graph index`:

```
1. Create adapters: RealFileSystem, ShellGitProvider, RayonParseProvider
2. Open isolated SqliteStore per repo
3. IndexUseCase::new(store, parser, fs, git)
4. use_case.full_index(clone_path) → IndexStats
5. QueryUseCase::new(store.clone(), store) for search queries
6. ImpactUseCase::new(store) for blast radius scenarios
```

**Key difference from normal index:** Eval creates a temporary SQLite database per cloned repo rather than using `.code-graph/graph.db`. This isolates eval state from the user's project.

### 2.5 Search & Impact Query APIs

**Search (for MRR/precision@k measurement):**
```rust
// QueryUseCase::search(query, limit) -> Vec<SearchResult>
// SearchResult { qualified_name, name, kind, file_path, score }
```

**Impact (for blast radius precision/recall/F1):**
```rust
// ImpactUseCase::blast_radius(targets, max_depth, min_confidence) -> ImpactReport
// ImpactReport { affected: Vec<AffectedNode>, ... }
// AffectedNode { qualified_name, depth, confidence, path }
```

### 2.6 Output Formatting Pattern

Eval report will implement the `Displayable` trait (already in `output.rs:35-39`):
- `fmt_compact`: One-liner metrics summary per suite (SPEC section 8 format)
- `fmt_table`: Tabular per-repo breakdown
- `fmt_json`: Full structured results via `serde_json::to_string_pretty`

CLI routes through `output::print(&report, output_format)` — same as all other commands.

---

## 3. Architecture Decisions

### 3.1 Eval Crate Module Structure

```
crates/eval/src/
  lib.rs       — pub fn run_suite(config: &SuiteConfig) -> Result<SuiteResult>
  metrics.rs   — pure functions: mrr(), precision_at_k(), blast_precision(), blast_recall(), f1()
  dataset.rs   — ManifestRepo, clone_or_cache(), validate_cache(), clear_cache()
  runner.rs    — index_repo(), run_search_queries(), run_impact_scenarios()
  report.rs    — SuiteResult, SearchMetrics, ImpactMetrics, Displayable impls
```

**Eval is a library, not a CLI tool.** It does not depend on clap. CLI owns argument parsing and delegates to `eval::run_suite()`.

### 3.2 Eval Pipeline Flow

```
CLI: parse EvalArgs
  ↓
eval::run_suite(config)
  ↓
1. Parse JSON manifest → Vec<ManifestRepo>
  ↓
2. For each repo:
   ├─ Check cache (~/.cache/code-graph-eval/<name>/<revision>/)
   ├─ Clone if needed (git clone --depth 1 --branch <rev>)
   └─ Write .revision marker file
  ↓
3. For each repo:
   ├─ Create temp SqliteStore
   ├─ Create adapters (RealFileSystem, RayonParseProvider)
   ├─ IndexUseCase::full_index(clone_path)
   └─ Validate ground truth (assert expected QNames exist in graph)
  ↓
4. Run queries:
   ├─ Search suite: QueryUseCase::search(query, limit) per query
   └─ Impact suite: ImpactUseCase::blast_radius(target, depth, confidence) per scenario
  ↓
5. Compute metrics:
   ├─ Search: mrr(), precision_at_k(5), precision_at_k(10)
   └─ Impact: blast_precision(), blast_recall(), f1()
  ↓
6. Compare against targets (MRR > 0.30, blast precision > 0.40)
  ↓
7. Return SuiteResult (metrics + pass/fail per target)
```

### 3.3 Ground Truth Validation (SPEC AC10)

Before computing metrics, validate that all expected qualified names in query files actually exist in the indexed graph:

```rust
for query in &queries {
    for expected_qname in &query.expected {
        if store.get_symbol(expected_qname)?.is_none() {
            errors.push(format!("SETUP_ERROR: '{}' not found in indexed graph for repo '{}'",
                expected_qname, repo.name));
        }
    }
}
if !errors.is_empty() {
    return Err(CodeGraphError::Other(errors.join("\n")));
}
```

This distinguishes dataset issues (wrong qualified names) from quality failures (low metrics).

### 3.4 Metrics — Pure Functions (No External Deps)

All metrics use only `std::collections::HashSet` and `f64` arithmetic:

**MRR (Mean Reciprocal Rank):** `sum(1/rank_of_first_correct) / num_queries`
- For each query, find rank of first correct result in ranked list
- Average the reciprocal ranks across all queries

**Precision@K:** `|relevant ∩ top_k| / k`
- For each query, count how many of the top-k results are in ground truth
- Average across queries

**Blast Precision:** `|predicted ∩ actual| / |predicted|`
- How many predicted affected symbols are actually affected

**Blast Recall:** `|predicted ∩ actual| / |actual|`
- How many actually affected symbols were predicted

**F1:** `2 * (precision * recall) / (precision + recall)`
- Harmonic mean, handles zero denominator with 0.0 return

### 3.5 Quality Targets & Exit Codes

| Metric | M01 Baseline | Stretch (v0.2) |
|--------|-------------|----------------|
| Search MRR | > 0.30 | > 0.50 |
| Blast Precision@High | > 0.40 | > 0.55 |

Exit code 0 = all M01 baseline targets met. Exit code 1 = any target failed. Used as release gate in CI.

### 3.6 Temporary Database Per Repo

Each eval'd repo gets an isolated SQLite database:
```rust
let temp_dir = tempfile::tempdir()?;
let db_path = temp_dir.path().join("eval.db");
let store = SqliteStore::open(&db_path)?;
```

This avoids polluting the user's `.code-graph/graph.db` and allows independent eval of each repo. The temp directory auto-cleans on drop.

Wait — the eval crate doesn't have `tempfile` as a dependency. Two options:
1. Add `tempfile = "3"` to eval crate (it's already a dev-dep of cli)
2. Create temp dirs via `std::env::temp_dir()` + `std::fs::create_dir_all` with a predictable path

**Decision: Add `tempfile = "3"` to eval crate.** It's already in the workspace (cli dev-dep), tiny crate, and the RAII cleanup on `TempDir` drop is valuable for eval databases.

**Updated Cargo.toml:**
```toml
[dependencies]
# ... (same as above)
tempfile = "3"
```

---

## 4. CI/CD Tooling Research

### 4.1 Lefthook Configuration

**No existing CI/CD files in the repo.** No `.github/workflows/`, no `lefthook.yml`, no `.lefthook/`.

**Lefthook YAML format for this project:**
```yaml
pre-commit:
  parallel: true
  commands:
    fmt:
      run: cargo fmt --check
      glob: "*.rs"
    clippy:
      run: cargo clippy --workspace -- -Dwarnings
      glob: "*.rs"
    test:
      run: cargo test --workspace
      glob: "*.rs"

pre-push:
  commands:
    full-test:
      run: cargo test --workspace
    bench-check:
      run: cargo bench --no-run
```

**Note on `commands` vs `jobs`:** The SPEC uses `commands` key (lefthook v1 syntax). Lefthook v2 uses `jobs`. Since `commands` still works in v2, we follow the SPEC exactly for consistency.

**Installation:** `brew install lefthook` or `cargo install lefthook`. After creating `lefthook.yml`, run `lefthook install` to wire git hooks.

### 4.2 cargo-llvm-cov

**Coverage threshold enforcement:**
```bash
cargo llvm-cov --workspace --fail-under-lines 80
```

**LCOV output for CI artifacts:**
```bash
cargo llvm-cov --workspace --lcov --output-path lcov.info
```

**HTML report for local dev:**
```bash
cargo llvm-cov --workspace --html
```

**GitHub Actions setup:**
```yaml
- name: Install cargo-llvm-cov
  uses: taiki-e/install-action@cargo-llvm-cov
- name: Generate coverage
  run: cargo llvm-cov --workspace --lcov --output-path lcov.info --fail-under-lines 80
- name: Upload coverage artifact
  uses: actions/upload-artifact@v4
  with:
    name: lcov-report
    path: lcov.info
```

### 4.3 cross-rs for aarch64-linux

**Command:**
```bash
cross build --release --target aarch64-unknown-linux-gnu
```

**Prerequisites:** Docker installed (pre-installed on GitHub Actions runners). No other setup needed.

**GitHub Actions integration:**
```yaml
- name: Install cross
  run: cargo install cross --git https://github.com/cross-rs/cross
- name: Build aarch64-linux
  run: cross build --release --target aarch64-unknown-linux-gnu
```

cross-rs containerizes the build with the appropriate cross-compilation toolchain (handles C deps like tree-sitter and rusqlite automatically).

### 4.4 GitHub Actions — CI Workflow

**Key actions:**
- `dtolnay/rust-toolchain@stable` — toolchain setup with components
- `actions/cache@v4` — cargo dependency caching keyed by `Cargo.lock`
- `taiki-e/install-action@cargo-llvm-cov` — install coverage tool

**CI workflow structure (`.github/workflows/ci.yml`):**

| Job | Runner | Steps |
|-----|--------|-------|
| fmt | ubuntu-latest | `cargo fmt --check` |
| clippy | ubuntu-latest | `cargo clippy --workspace -- -Dwarnings` |
| test | matrix: [ubuntu-latest, macos-latest] | `cargo test --workspace` |
| coverage | ubuntu-latest | `cargo llvm-cov --workspace --fail-under-lines 80` |
| audit | ubuntu-latest | `cargo audit` |
| bench | ubuntu-latest | `cargo bench --no-run` |

**Caching strategy:**
```yaml
- uses: actions/cache@v4
  with:
    path: |
      ~/.cargo/bin/
      ~/.cargo/registry/index/
      ~/.cargo/registry/cache/
      ~/.cargo/git/db/
      target/
    key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
    restore-keys: ${{ runner.os }}-cargo-
```

### 4.5 GitHub Actions — Release Workflow

**Trigger:** Tag push `v*`

**Stage 1 — Eval Gate:**
- Build release binary
- Run `code-graph eval --suite all`
- Assert exit code 0 (quality thresholds met)

**Stage 2 — Build Matrix (4 targets):**

| Target | Runner | Method |
|--------|--------|--------|
| x86_64-unknown-linux-gnu | ubuntu-latest | `cargo build --release --target` |
| aarch64-unknown-linux-gnu | ubuntu-latest | `cross build --release --target` |
| x86_64-apple-darwin | macos-latest | `cargo build --release --target` |
| aarch64-apple-darwin | macos-latest | `cargo build --release --target` |

**Stage 3 — Publish:**
- Create GitHub Release with 4 binaries via `softprops/action-gh-release@v2`
- `cargo publish` in topological order: domain -> parser -> storage -> watch -> eval -> cli -> binary
- Sleep 30s between publishes for crates.io index propagation

**Crates.io publish order** (topological, respecting dependency edges):
1. domain (no internal deps)
2. parser, storage (depend on domain — can be parallel)
3. watch (depends on domain)
4. eval (depends on domain, parser, storage)
5. cli (depends on all)
6. binary (depends on cli)

---

## 5. Eval Dataset Design

### 5.1 Language Support Verification

All 5 eval repos are fully supported by the parser:

| Repo | Language | Parser | Extensions | Verified |
|------|----------|--------|------------|----------|
| expressjs/express | JavaScript | `JavaScriptParser` | .js, .jsx | Yes |
| trpc/trpc | TypeScript | `TypeScriptParser` | .ts, .tsx | Yes |
| BurntSushi/ripgrep | Rust | `RustParser` | .rs | Yes |
| fastapi/fastapi | Python | `PythonParser` | .py | Yes |
| golang/go (std lib) | Go | `GoParser` | .go | Yes |

### 5.2 Pinned Revisions

Use stable release tags for reproducible eval:

| Repo | Revision | Rationale |
|------|----------|-----------|
| express | v4.21.2 | Latest stable v4 |
| trpc | v11.0.0 | Latest stable v11 |
| ripgrep | 14.1.1 | Latest stable |
| fastapi | 0.115.0 | Latest stable |
| golang/go | go1.23.0 | Latest stable (subset: src/net/http) |

### 5.3 Query Count Targets

| Suite | Per-Repo Target | Total |
|-------|----------------|-------|
| Search | 10+ queries | 50+ (AC2) |
| Impact | 4+ scenarios | 20+ (AC4) |

### 5.4 Go Stdlib Subset Strategy

The full Go stdlib is massive. The SPEC says "Go (std lib subset)". Use a subset approach:
- Clone the full repo but only evaluate queries targeting `src/net/http/` package
- This provides enough symbols for meaningful eval without indexing the entire stdlib
- Alternatively, use `--path` to scope indexing to `src/net/http/`

---

## 6. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Eval repos take too long to clone in CI | Medium | Medium | `--depth 1` + GitHub Actions cache keyed by manifest checksum |
| Indexing large repos (trpc monorepo) times out | Medium | Medium | Per-repo timeout with graceful skip + error report |
| FTS5 search quality too low for M01 baseline (MRR < 0.30) | Medium | High | Baseline targets are conservative; curate queries that play to FTS5 strengths |
| Qualified name format mismatch between eval dataset and parser | Medium | Medium | AC10 ground truth validation catches this early as setup errors |
| cross-rs Docker build fails for aarch64 with tree-sitter C deps | Low | Medium | cross-rs handles C toolchains automatically; fallback to native ARM runner |
| cargo publish ordering — race condition with crates.io index | Low | Medium | 30s sleep between publishes; `--no-verify` only if needed |
| Lefthook conflicts with existing git hooks | Very Low | Low | `lefthook install` is non-destructive; warns about existing hooks |
| cargo-llvm-cov LLVM version mismatch on CI | Low | Low | Use `taiki-e/install-action` which handles version matching |
| Coverage gate (80%) too aggressive for new eval crate code | Medium | Low | Eval crate is mostly delegation + pure functions; coverage should be achievable |

---

## 7. Open Questions Resolved

| Question | Resolution |
|----------|-----------|
| New crate or module in CLI? | **New `eval` crate** — separate library, not a CLI module. CLI delegates to it. |
| Git clone: `git2` or shell? | **Shell** (`std::process::Command`) — matches existing pattern, avoids heavy C dep. |
| Cache directory location? | **`~/.cache/code-graph-eval/`** with XDG_CACHE_HOME fallback. |
| Cache validation method? | **`.revision` marker file** — simple, git-independent. |
| New external deps? | **Only `tempfile = "3"`** in eval crate (already in workspace). Zero new external crates. |
| Eval output formatting? | **Implement `Displayable` trait** — same pattern as all other commands. |
| Go stdlib handling? | **Full clone, subset eval** — query only `src/net/http/` package. |
| Lefthook v1 vs v2 syntax? | **Use `commands` key** (SPEC format) — works in both versions. |
| Coverage tool? | **`cargo-llvm-cov`** with `--fail-under-lines 80` for CI enforcement. |
| Release binary creation? | **`softprops/action-gh-release@v2`** — attach 4 binaries to GitHub Release. |
| Cross-compilation for aarch64-linux? | **`cross-rs`** — Docker-based, handles C toolchains automatically. |
| Crates.io publish order? | **Topological:** domain -> parser, storage -> watch -> eval -> cli -> binary. |
