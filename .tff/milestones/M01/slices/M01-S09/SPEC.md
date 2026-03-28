# Spec — M01-S09: Eval Framework & CI/CD

## Overview

Implement the eval framework (R8) and CI/CD pipeline (R9) as the final M01 slice. Adds a new `eval` crate to the workspace, curates evaluation datasets across 5 open-source repos, configures Lefthook for local git hooks, and sets up GitHub Actions for PR checks and release automation.

## Approach

**Clone-at-eval + modular workflows:** JSON manifests define repos and queries. Repos are cloned at eval time with local caching. Two separate GitHub Actions workflows (`ci.yml` for PR checks, `release.yml` for tag-triggered builds). Eval runs as a release gate — quality regressions block the release.

## Design

### 1. Eval Crate

New `eval` crate added to the workspace (7 crates total):

```
crates/
  eval/
    Cargo.toml          # depends on: domain, parser, storage
    src/
      lib.rs            # public API: run_suite(), SuiteResult
      metrics.rs        # MRR, precision@k, recall, F1 (pure functions)
      dataset.rs        # manifest parsing, repo clone/cache management
      runner.rs         # orchestration: index repo -> run queries -> collect results
      report.rs         # format results (compact/table/json)
```

**Dependency graph update:**
```
binary -> cli -> eval -> domain
                       -> parser
                       -> storage
```

Domain stays focused on graph operations. Eval owns metrics, dataset management, and the pipeline. CLI delegates to `eval::run_suite()` and handles output formatting.

### 2. Eval Pipeline

1. Parse CLI args (`--suite search|impact|all`)
2. Read JSON manifest -> clone/cache repos at pinned revisions
3. Index each repo (full pipeline: parse -> resolve -> store)
4. **Validate ground truth** — assert all expected qualified names exist in the indexed graph; report mismatches as setup errors (not quality failures)
5. Run queries against indexed graph
6. Compare results to ground truth
7. Compute metrics
8. Report results (compact/table/json)

### 3. Metrics (pure functions in eval crate)

- `mrr(ranked_results, ground_truth) -> f64` — Mean Reciprocal Rank
- `precision_at_k(ranked_results, ground_truth, k) -> f64`
- `blast_precision(predicted, actual) -> f64`
- `blast_recall(predicted, actual) -> f64`
- `f1(precision, recall) -> f64`

### 4. Dataset Structure

```
eval/
  suites/
    search/
      manifest.json         # repo URLs + pinned revisions
      queries/
        javascript.json     # express queries
        typescript.json     # trpc queries
        rust.json           # ripgrep queries
        python.json         # fastapi queries
        go.json             # go std lib queries
    impact/
      manifest.json
      queries/
        javascript.json
        typescript.json
        rust.json
        python.json
        go.json
```

**Manifest format (JSON):**
```json
{
  "suite": {
    "name": "search",
    "description": "Search quality evaluation"
  },
  "repos": [
    {
      "name": "express",
      "url": "https://github.com/expressjs/express",
      "revision": "v4.21.2",
      "languages": ["javascript"]
    }
  ]
}
```

**Search query format:**
```json
{
  "queries": [
    {
      "repo": "express",
      "query": "Router",
      "expected": ["src/router/index.js::Router"]
    }
  ]
}
```

**Impact query format:**
```json
{
  "scenarios": [
    {
      "repo": "express",
      "description": "Changing Router.route affects downstream handlers",
      "target": "src/router/index.js::route",
      "depth": 3,
      "confidence": "high",
      "expected_affected": [
        "src/router/index.js::Router",
        "lib/application.js::lazyrouter"
      ]
    }
  ]
}
```

### 5. Eval Repos (5 repos, 50+ queries)

| Repo | Languages | Rationale |
|------|-----------|-----------|
| expressjs/express | JavaScript | Deep middleware chain, re-exports, barrel files |
| trpc/trpc | TypeScript | Pure TS monorepo, barrel exports, deep re-export chains |
| BurntSushi/ripgrep | Rust | Multi-crate workspace, complex module tree |
| fastapi/fastapi | Python | Heavy __init__.py re-exports, decorator patterns |
| golang/go (std lib subset) | Go | Clean package structure, well-defined imports |

### 6. Cache Strategy

- Cache location: `~/.cache/code-graph-eval/<repo-name>/<revision>/`
- Clone method: `git clone --depth 1 --branch <revision>`
- Cache validation: check revision match (not TTL-based)
- `--no-cache` flag to force re-clone
- CI caching: GitHub Actions cache keyed by manifest checksum

### 7. Quality Targets

**M01 baseline targets** (current search is bare FTS5 without tuning):
- Search MRR > 0.30
- Blast radius precision > 0.40 at high confidence

**Stretch targets** (achievable after search quality improvements in v0.2):
- Search MRR > 0.50 (vs code-review-graph's 0.35)
- Blast radius precision > 0.55 at high confidence (vs code-review-graph's 0.38)

The eval framework is built to measure and track progress toward stretch targets. M01 release gate uses baseline targets only.

### 8. Eval Output

```
Search Suite — 5 repos, 52 queries
  MRR:          0.62 (target: >0.50) PASS
  Precision@5:  0.71
  Precision@10: 0.58

Impact Suite — 5 repos, 24 scenarios
  Precision:    0.61 (target: >0.55) PASS
  Recall:       0.48
  F1:           0.54
```

Supports `--json` and `--table` output formats.

### 9. Lefthook

```yaml
# lefthook.yml
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

### 10. GitHub Actions — CI

`.github/workflows/ci.yml` — triggered on PR to `main`:

| Job | Runner | Description |
|-----|--------|-------------|
| fmt | ubuntu-latest | `cargo fmt --check` |
| clippy | ubuntu-latest | `cargo clippy --workspace -- -Dwarnings` |
| test | ubuntu-latest + macos-latest | `cargo test --workspace` |
| coverage | ubuntu-latest | `cargo llvm-cov --workspace`, fail under 80% |
| audit | ubuntu-latest | `cargo audit` |
| bench | ubuntu-latest | `cargo bench --no-run` (compilation check) |

### 11. GitHub Actions — Release

`.github/workflows/release.yml` — triggered on tag `v*`:

**Stage 1: Eval Gate**
- Build release binary
- Run search + impact suites
- Assert quality thresholds
- Fail release if thresholds not met

**Stage 2: Build** (needs eval)
- Matrix: 4 targets
  - x86_64-unknown-linux-gnu (ubuntu, native)
  - aarch64-unknown-linux-gnu (ubuntu, cross-compile via `cross-rs`)
  - x86_64-apple-darwin (macos, native)
  - aarch64-apple-darwin (macos, native)
- Native builds: `cargo build --release --target <target>`
- aarch64-linux: `cross build --release --target aarch64-unknown-linux-gnu` (Docker-based, handles C toolchains for tree-sitter + rusqlite automatically)

**Stage 3: Publish** (needs build)
- Create GitHub Release with all 4 binaries
- `cargo publish` to crates.io in topological order (domain -> parser -> storage -> watch -> eval -> cli -> binary)
- Path dependencies auto-resolved by `cargo publish` with workspace inheritance

### 12. Coverage

- Tool: `cargo-llvm-cov` (LLVM-based)
- Threshold: 80% line coverage, enforced in CI
- Scope: workspace-wide including eval crate
- Report: LCOV format uploaded as CI artifact
- Local: `cargo llvm-cov --workspace --html` for HTML report

## Acceptance Criteria

| # | Criterion |
|---|-----------|
| AC1 | `eval` crate added as 7th workspace member with metrics, dataset, runner, report modules |
| AC2 | `code-graph eval --suite search` runs against 5 repos with 50+ queries |
| AC3 | Search suite reports MRR and precision@k metrics |
| AC4 | `code-graph eval --suite impact` runs against 5 repos with 20+ scenarios |
| AC5 | Impact suite reports precision, recall, and F1 metrics |
| AC6 | Search MRR > 0.30 on the curated dataset (M01 baseline) |
| AC7 | Blast radius precision > 0.40 at high confidence (M01 baseline) |
| AC8 | Eval supports compact, table, and JSON output formats |
| AC9 | `code-graph eval --no-cache` re-clones repos even when cache exists |
| AC10 | Eval validates ground truth before computing metrics — mismatched qualified names reported as setup errors |
| AC11 | Eval repo caching works (second run doesn't re-clone) |
| AC12 | `lefthook.yml` runs fmt + clippy + test on pre-commit |
| AC13 | `lefthook.yml` runs full-test + bench-check on pre-push |
| AC14 | GitHub Actions CI workflow passes (fmt, clippy, test, coverage, audit, bench) |
| AC15 | Coverage gate enforces 80% minimum line coverage |
| AC16 | GitHub Actions release workflow builds 4 targets (including aarch64-linux via cross-rs) and creates GitHub Release |
| AC17 | Eval runs as release gate — release blocked if quality thresholds fail |
| AC18 | Release workflow includes `cargo publish` step for crates.io (workspace publish ordering handled) |
