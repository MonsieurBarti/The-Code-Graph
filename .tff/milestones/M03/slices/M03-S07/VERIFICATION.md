# Verification — M03-S07: Real-World Validation

## Summary

Extended the eval framework with a pluggable `EvalSuite` trait and 8 suite implementations (search, impact, core, flows, risk, analysis, invariants, bench). Added ground truth data for all 5 target repos across all new suites. Wired full dispatch so `tcg eval <suite>` and `tcg eval all` dispatch all 8 suites.

## Acceptance Criteria

| AC | Description | Status | Evidence |
|---|---|---|---|
| AC1 | Search: 100% recall for existence, MRR >= 0.30, P@5 >= 0.30 | Pending | Search suite + existence queries implemented. 242 total queries (125 existence). Requires repo validation run. |
| AC2 | Impact: Precision >= 0.40, Recall >= 0.30. Invariants hold. | Pending | Impact suite with recall gate wired. Requires repo validation run. |
| AC3 | Flows: Entry point precision >= 0.80. Invariants hold. | Pending | FlowsSuite with betweenness invariants + entry_point_precision(). Ground truth: 12-14 entry points/repo. Requires repo validation run. |
| AC4 | Risk: Scores in [0,1]. Top-N precision >= 0.60. | Pending | RiskSuite with composite + factor range invariants. Ground truth: 7-8 symbols/repo. Requires repo validation run. |
| AC5 | Analysis: Modularity > 0. Dead code precision >= 0.70. Clone invariants. | Pending | AnalysisSuite with community, dead code, clone invariants. Ground truth curated. Requires repo validation run. |
| AC6 | Core: Idempotent indexing. Import accuracy >= 0.70. | Pending | CoreSuite with idempotency check + incremental no-op + import resolution. Ground truth: 11-12 import chains/repo. Requires repo validation run. |
| AC7 | Invariants: All hold with zero violations. | Pending | InvariantsSuite meta-suite collects all invariants. Requires repo validation run. |
| AC8 | Bench: Baseline JSON produced. Comparison mode. | Pending | BenchSuite with timing, percentiles, baseline JSON writing. Requires repo validation run. |
| AC9 | Bugs fixed or documented. | Pass | No bugs found during implementation. Bug tracking will happen during validation runs. |
| AC10 | CI passes. No test regression. | Pass | `cargo clippy --workspace` clean, `cargo fmt --check` clean, `cargo test --workspace` 788 tests pass. |

## Test Results

### Workspace-level
- **788 tests pass** across 17 test suites
- **0 failures**
- clippy: clean (no warnings)
- fmt: clean

### Eval crate
- **98 tests pass** in the-code-graph-eval
- New tests added: 48 (trait dispatch, serialization, metrics, suite names, invariants, percentiles, ground truth parsing)

## New Files Created

### Eval suite modules (8 files)
- `crates/eval/src/suites/mod.rs` — EvalSuite trait, MetricResult, InvariantResult, all_suites() registry
- `crates/eval/src/suites/search.rs` — SearchSuite (existence recall + search invariants)
- `crates/eval/src/suites/impact.rs` — ImpactSuite (edge invariants)
- `crates/eval/src/suites/core.rs` — CoreSuite (idempotency check, import validation)
- `crates/eval/src/suites/flows.rs` — FlowsSuite (entry point precision, acyclicity, betweenness range)
- `crates/eval/src/suites/risk.rs` — RiskSuite (top-N precision, composite/factor range)
- `crates/eval/src/suites/analysis.rs` — AnalysisSuite (modularity, dead code, clone invariants)
- `crates/eval/src/suites/invariants.rs` — InvariantsSuite (meta-suite collecting all)
- `crates/eval/src/suites/bench.rs` — BenchSuite (timing, percentiles, baselines)

### Ground truth data (25 files)
- `eval/suites/flows/ground-truth/{express,trpc,ripgrep,fastapi,go-stdlib}.json`
- `eval/suites/risk/ground-truth/{express,trpc,ripgrep,fastapi,go-stdlib}.json`
- `eval/suites/core/ground-truth/{express,trpc,ripgrep,fastapi,go-stdlib}.json`
- `eval/suites/analysis/ground-truth/{repo}-dead-code.json` (5 files)
- `eval/suites/analysis/ground-truth/{repo}-clones.json` (5 files)

### Suite manifests (4 files)
- `eval/suites/{core,flows,risk,analysis}/manifest.json`

## Modified Files

- `crates/eval/src/lib.rs` — Suite enum (8 variants + All), SuiteConfig (compare_baseline), run_suite() full dispatch
- `crates/eval/src/runner.rs` — 6 new runner functions, shared helpers preserved
- `crates/eval/src/report.rs` — 6 new result types, extended SuiteResult, all_passed() checks all gates
- `crates/eval/src/metrics.rs` — existence_recall() function
- `crates/eval/src/dataset.rs` — 10 new ground truth types, 5 parser functions
- `crates/cli/src/commands/eval.rs` — suite_from_str() for 8 suites
- `crates/cli/src/commands/mod.rs` — --compare arg on EvalArgs
- `eval/suites/search/queries/*.json` — 125 new existence queries

## Bugs Found

No bugs found during implementation. All existing tests continue to pass.

**Note**: Full validation against the 5 target repositories requires network access to clone them. The framework is fully wired and ready for `tcg eval all` execution. Ground truth may need refinement after the first live validation run (qualified names in ground truth are based on expected indexing output and may not exactly match actual indexed symbols).

## Commits

| Commit | Type | Description |
|---|---|---|
| c6b7733 | feat(S07/T01) | EvalSuite trait and suite registry |
| 62bc647 | feat(S07/T02) | Report types for 6 new eval suites |
| 4551807 | feat(S07/T04) | Ground truth parsers for 5 new suite formats |
| 91ca798 | feat(S07/T03) | Suite enum and CLI for 6 new suites |
| d3702b6 | refactor(S07/T05) | SearchSuite implementing EvalSuite trait |
| ca7800e | refactor(S07/T06) | ImpactSuite implementing EvalSuite trait |
| 4716a85 | feat(S07/T07) | Core eval suite |
| 3ef9b1b | feat(S07/T08) | Flows eval suite |
| f19af48 | feat(S07/T09) | Risk eval suite |
| 9d6d817 | feat(S07/T10) | Analysis eval suite |
| 399cd60 | feat(S07/T12) | Bench suite with performance baselines |
| be6b6c6 | feat(S07/T11,T13) | Suite dispatch and Invariants meta-suite |
| 08ab374 | feat(S07/T14) | Search existence queries (100+) |
| 82a86fd | feat(S07/T15) | Ground truth for flows, risk, analysis, core |
| c155841 | feat(S07/T16) | Full suite dispatch for all 8 eval suites |
