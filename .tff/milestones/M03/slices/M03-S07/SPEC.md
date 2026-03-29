# Spec — M03-S07: Real-World Validation

## Overview

End-to-end validation of v0.1 Core and v0.2 Analysis features against 5 real open-source repositories. Extends the eval framework with new suites, property invariants, performance baselines, and fixes bugs found during validation.

**Scope**: v0.1 (indexing, queries, search, import resolution, incremental, watch) + v0.2 (flows, risk, communities, dead code, clones, embeddings). M03 features are excluded — each M03 slice validates its own feature during its verifying phase. A final M03 integration pass can be added as a follow-up slice if needed at milestone review.

**Approach**: Suite-per-feature with combined property invariants and metric assertions. Each suite defines both ground-truth-based metrics (requiring curated datasets) and structural property invariants (no ground truth needed). Both layers must pass.

## Target Repositories

Existing eval repos (no additions — deeper validation of each):

| Repo | Language | Purpose |
|---|---|---|
| expressjs/express | JavaScript | HTTP framework, handler patterns |
| trpc/trpc | TypeScript | Type-safe RPC, complex TS generics |
| BurntSushi/ripgrep | Rust | CLI tool, crate workspace |
| tiangolo/fastapi | Python | Async web framework, decorators |
| golang/go (stdlib subset) | Go | Interfaces, struct embedding |

## Eval Framework Extension

### Architecture

Refactor `crates/eval` to support pluggable suites:

```
crates/eval/src/
  suites/
    mod.rs          # Suite trait + registry
    search.rs       # Existing search eval (refactored)
    impact.rs       # Existing impact eval (refactored)
    flows.rs        # NEW: flow detection + criticality
    risk.rs         # NEW: risk scoring
    analysis.rs     # NEW: communities, dead code, clones
    core.rs         # NEW: indexing correctness, imports
    invariants.rs   # NEW: structural property checks
    bench.rs        # NEW: performance baselines
  metrics.rs        # Extended with new metric types
  runner.rs         # Refactored to dispatch to suite modules
  report.rs         # Extended with perf baseline output
```

### Suite Trait

The trait below is illustrative — actual types will align with the existing eval crate API:

```rust
trait Suite {
    fn name(&self) -> &str;
    fn run_metrics(&self, graph: &GraphStore, dataset: &Dataset) -> Vec<MetricResult>;
    fn run_invariants(&self, graph: &GraphStore) -> Vec<InvariantResult>;
}
```

### CLI Extension

`tcg eval <suite>` where suite = `search | impact | flows | risk | analysis | core | invariants | bench | all`

Running `tcg eval all` executes every suite sequentially and produces a unified report.

## Suite Definitions

### Search Suite (existing, enhanced)

**Metrics**:
- MRR >= 0.30, P@5 >= 0.30, P@10 >= 0.20 (ranked quality — MRR threshold matches current codebase target)
- **100% recall for existence queries**: searching an exact or partial symbol name that exists in the graph MUST return that symbol somewhere in results

**Ground truth additions**:
- New query category: `existence` — symbol name substring queries with binary pass/fail
- Curate 20-30 existence queries per repo (function names, class names, partial matches)

**Invariants**:
- Every search result references a valid symbol ID in the graph
- FTS5 index covers all indexed symbols (count match)

### Impact Suite (existing, enhanced)

**Metrics**: Precision >= 0.40 (hard gate — suite fails below threshold), Recall >= 0.30 (hard gate), F1 reported (informational)

**Invariants**:
- Impact set is a subset of nodes reachable from changed symbols
- No self-referential impacts (symbol impacting itself)

### Flows Suite (new)

**Metrics**:
- Entry point detection precision >= 0.80 against ground truth
- Flow path validity: every node in a reported flow is reachable from its entry point

**Ground truth**: 10-20 tagged entry points per repo (main functions, HTTP handlers, test functions, CLI commands, public root exports)

**Invariants**:
- Betweenness centrality scores are non-negative
- Each reported flow path (ordered sequence of symbols from entry to terminal) is acyclic — no symbol appears twice in the same path
- Every flow starts at a detected entry point
- `CriticalityScore.betweenness` values in [0.0, 1.0] (the normalized betweenness centrality field on the domain model)

### Risk Suite (new)

**Metrics**:
- Top-N precision >= 0.60 against manually tagged high-risk symbols
- 5-10 manually tagged high/low risk symbols per repo

**Invariants**:
- All composite risk scores in [0.0, 1.0]
- All `RiskFactors` components (`criticality`, `coupling`, `test_gap`, `sensitivity`) in [0.0, 1.0]. Note: `test_gap` has inverted semantics (1.0 = untested, 0.0 = tested)
- Symbols with zero edges have minimal risk (< 0.2)

### Analysis Suite (new)

**Communities**:
- Invariants: every non-isolated symbol belongs to exactly one community; isolated symbols (zero edges) may be unassigned; no empty communities; community count < total symbol count
- Metric: modularity score > 0.0

**Dead Code**:
- Invariants: every reported dead symbol has zero incoming edges
- Metric: precision >= 0.70 against manually tagged dead code (5-10 per repo)

**Clones**:
- Invariants: each clone match (source, target) with score S implies the reverse pair logically exists (implementation stores one direction only — invariant checks that no contradictory asymmetric pairs exist); similarity scores in [0.0, 1.0]
- Metric: precision against known copy-paste patterns (3-5 pairs per repo)

### Core Suite (new)

**Indexing correctness**:
- Idempotency: indexing the same repo twice (full index, delete DB, full index again) produces identical symbol/edge counts
- All files in repo are visited (file count match)
- Symbol count > 0 for every indexed file with parseable content
- Every edge references valid source and target symbol IDs

Note: full-vs-incremental comparison is not testable with the current eval harness (shallow clones + NoOpGitProvider). Instead, we validate idempotency (two full indexes match) and incremental no-op stability (incremental on unchanged repo produces zero changes). True incremental testing with file modifications is deferred to a separate slice that adds git mock infrastructure.

**Import resolution**:
- Accuracy >= 0.70 against ground truth of known cross-file references (10-15 per repo)

### Invariants Suite (meta-suite)

Runs all invariants from all suites in a single pass. Reports per-invariant pass/fail with zero-tolerance (any violation = fail).

### Bench Suite (performance baselines)

**Captures per repo**:
- Full indexing time (ms)
- Incremental no-change time (ms)
- Query latencies: search, impact, flows, callers, callees — p50/p95 over 10 runs
- Graph size: symbol count, edge count, DB file size (bytes)

**Output**: `eval/baselines/baseline-<version>.json`

**Comparison**: `tcg eval bench --compare <baseline.json>` shows delta per metric. No automated regression gate — informational only for this slice.

## Ground Truth Curation

**Format**: JSON files following existing eval conventions:

```json
{
  "repo": "expressjs/express",
  "suite": "flows",
  "ground_truth": [
    {
      "type": "entry_point",
      "symbol": "app.listen",
      "category": "HttpHandler",
      "file": "lib/application.js"
    }
  ]
}
```

**Location**: `eval/suites/<suite>/ground-truth/<repo-slug>.json` (new suites use `ground-truth/`; existing `queries/` directories in search/impact suites are preserved for backward compatibility)

**Curation process**: During execution, index each repo, inspect results, and curate ground truth by reading the actual code. The agent does this as part of the slice work.

**Prioritization**: If curation effort exceeds reasonable bounds, prioritize suites in this order: (1) search + core (highest value — validates base correctness), (2) flows + risk, (3) analysis. Bench suite requires no ground truth.

**Estimated items**:

| Suite | Items per repo | Total (5 repos) |
|---|---|---|
| Search (existence) | 20-30 | 100-150 |
| Flows (entry points) | 10-20 | 50-100 |
| Risk (tagged symbols) | 5-10 | 25-50 |
| Dead code (tagged) | 5-10 | 25-50 |
| Clones (pairs) | 3-5 | 15-25 |
| Core (import chains) | 10-15 | 50-75 |

## Bug Fix Strategy

**Fix-as-you-go**: When a suite reveals a bug:
1. Record the failure as a ground truth test case (it becomes a regression test)
2. Fix the bug in the relevant crate
3. Re-run the suite to confirm
4. Commit the fix with the test case

**Severity triage**:
- Crash/panic: fix immediately
- Wrong results: fix if root cause is clear; if complex, log as known issue with failing test
- Performance: record in baseline, fix only if >10x expected

**Scope boundary**: Fixes limited to v0.1/v0.2 features. Architectural changes flagged for separate slice.

**Time-box clause**: If a fix requires parser changes affecting more than 1 language, or requires new infrastructure (e.g., git mock provider), defer to a separate slice rather than fixing inline. Log as known issue with a failing test.

**Tracking**: VERIFICATION.md lists all bugs found with disposition (fixed / deferred / known issue).

## Acceptance Criteria

**AC1**: `tcg eval search` — 100% recall for existence queries (exact/partial symbol name search always finds the symbol). MRR >= 0.30, P@5 >= 0.30 for ranked quality. All 5 repos pass.

**AC2**: `tcg eval impact` — Precision >= 0.40, Recall >= 0.30 across all 5 repos. All invariants hold.

**AC3**: `tcg eval flows` — Entry point detection precision >= 0.80. All flow invariants hold (acyclic paths, valid reachability, scores in range). All 5 repos pass.

**AC4**: `tcg eval risk` — Risk scores in [0,1]. High-risk correlation top-N precision >= 0.60. Zero-edge symbols < 0.2 risk. All 5 repos pass.

**AC5**: `tcg eval analysis` — Community modularity > 0. Dead code precision >= 0.70. Clone invariants hold. All 5 repos pass.

**AC6**: `tcg eval core` — Full indexing is idempotent (two runs produce identical counts). Incremental no-op is stable. Import resolution accuracy >= 0.70. All structural invariants hold. All 5 repos pass.

**AC7**: `tcg eval invariants` — All property invariants hold across all 5 repos with zero violations.

**AC8**: `tcg eval bench` — Baseline JSON produced for all 5 repos with all metrics captured. Comparison mode functional.

**AC9**: All bugs found are either fixed (with regression test) or documented as known issues with justification in VERIFICATION.md.

**AC10**: CI passes (clippy, fmt, test) after all fixes. Workspace-level test coverage does not regress below current baseline.

## Out of Scope

- M03 feature validation (wiki, web UI, multi-repo, refactoring, MCP, language extensibility)
- Automated performance regression gates in CI
- Adding new target repositories
- Watch/daemon long-running validation (covered by existing unit tests)
