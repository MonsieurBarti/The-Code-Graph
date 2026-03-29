# Real-World Validation Implementation Plan

> For agentic workers: execute task-by-task with TDD.

**Goal:** End-to-end validation of v0.1 Core and v0.2 Analysis features against 5 real open-source repositories. Extends the eval framework with pluggable suite trait, 6 new suites, property invariants, performance baselines, and fixes bugs found during validation.

**Architecture:** Refactor eval crate from hardcoded enum dispatch to trait-based suite registry. Each suite implements `EvalSuite` with `run_metrics()` and `run_invariants()`. Runner iterates registered suites. Existing search/impact suites are refactored to implement the trait without changing behavior.

**Tech Stack:** Rust, SQLite (via SqliteStore), tree-sitter parsers, serde_json for ground truth, std::time::Instant for benchmarks.

## File Structure

### New files
| File | Responsibility |
|---|---|
| `crates/eval/src/suites/mod.rs` | `EvalSuite` trait, `InvariantResult`, suite registry |
| `crates/eval/src/suites/search.rs` | Search suite (refactored from runner.rs) |
| `crates/eval/src/suites/impact.rs` | Impact suite (refactored from runner.rs) |
| `crates/eval/src/suites/core.rs` | Indexing idempotency, import resolution |
| `crates/eval/src/suites/flows.rs` | Entry point detection, flow path validation |
| `crates/eval/src/suites/risk.rs` | Risk scoring validation |
| `crates/eval/src/suites/analysis.rs` | Communities, dead code, clones |
| `crates/eval/src/suites/invariants.rs` | Meta-suite collecting all invariants |
| `crates/eval/src/suites/bench.rs` | Performance baselines with timing |
| `eval/suites/core/manifest.json` | Core suite repo manifest |
| `eval/suites/core/ground-truth/*.json` | Import resolution ground truth (5 files) |
| `eval/suites/flows/manifest.json` | Flows suite repo manifest |
| `eval/suites/flows/ground-truth/*.json` | Entry point ground truth (5 files) |
| `eval/suites/risk/manifest.json` | Risk suite repo manifest |
| `eval/suites/risk/ground-truth/*.json` | Tagged risk symbols (5 files) |
| `eval/suites/analysis/manifest.json` | Analysis suite repo manifest |
| `eval/suites/analysis/ground-truth/*.json` | Dead code + clone pairs (5 files) |

### Modified files
| File | Change |
|---|---|
| `crates/eval/src/lib.rs` | Extend `Suite` enum (8 variants + All), add `pub mod suites`, refactor `run_suite()` |
| `crates/eval/src/runner.rs` | Keep shared helpers (`index_repo`, `validate_ground_truth`, `confidence_from_str`), remove suite-specific run functions |
| `crates/eval/src/report.rs` | Add 6 new result structs, extend `SuiteResult` with Optional fields, update formatters |
| `crates/eval/src/metrics.rs` | Add `existence_recall()` metric for search existence queries |
| `crates/eval/src/dataset.rs` | Add parsers for new ground truth formats (entry points, risk tags, dead code tags, clone pairs, import chains) |
| `crates/cli/src/commands/eval.rs` | Extend `suite_from_str()` for 8 suite names, add `--compare` flag for bench |

---

### Task 1: EvalSuite Trait + Suite Registry
**Files:** Create `crates/eval/src/suites/mod.rs`
**Traces to:** AC7 (invariant framework), all ACs (trait enables all suites)

- [ ] Step 1: Create `crates/eval/src/suites/mod.rs` with the trait, types, registry, and tests. Also create 8 empty stub sub-module files (`search.rs`, `impact.rs`, `core.rs`, `flows.rs`, `risk.rs`, `analysis.rs`, `invariants.rs`, `bench.rs`) containing only `// TODO: implement`. Add `pub mod suites;` to `crates/eval/src/lib.rs`. Contents of `mod.rs`:
```rust
// crates/eval/src/suites/mod.rs
use domain::error::Result;
use storage::SqliteStore;
use std::path::Path;

/// Result of a single invariant check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InvariantResult {
    pub name: String,
    pub suite: String,
    pub passed: bool,
    pub message: Option<String>,
}

/// Result of a single metric measurement.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricResult {
    pub name: String,
    pub value: f64,
    pub target: Option<f64>,
    pub passed: bool,
}

/// Pluggable evaluation suite trait.
pub trait EvalSuite {
    fn name(&self) -> &str;
    fn run_metrics(
        &self,
        store: &SqliteStore,
        clone_path: &Path,
        ground_truth_dir: &Path,
    ) -> Result<Vec<MetricResult>>;
    fn run_invariants(
        &self,
        store: &SqliteStore,
        clone_path: &Path,
    ) -> Result<Vec<InvariantResult>>;
}

/// Registry of all available suites. Populated as suites are implemented.
pub fn all_suites() -> Vec<Box<dyn EvalSuite>> {
    vec![]  // Tasks 5-12 will add entries here
}

pub mod search;
pub mod impact;
pub mod core;
pub mod flows;
pub mod risk;
pub mod analysis;
pub mod invariants;
pub mod bench;

#[cfg(test)]
mod tests {
    use super::*;

    struct DummySuite;
    impl EvalSuite for DummySuite {
        fn name(&self) -> &str { "dummy" }
        fn run_metrics(&self, _: &SqliteStore, _: &Path, _: &Path) -> Result<Vec<MetricResult>> {
            Ok(vec![MetricResult { name: "test_metric".into(), value: 0.5, target: Some(0.3), passed: true }])
        }
        fn run_invariants(&self, _: &SqliteStore, _: &Path) -> Result<Vec<InvariantResult>> {
            Ok(vec![InvariantResult { name: "test_inv".into(), suite: "dummy".into(), passed: true, message: None }])
        }
    }

    #[test]
    fn eval_suite_trait_dispatch() {
        let suite: Box<dyn EvalSuite> = Box::new(DummySuite);
        assert_eq!(suite.name(), "dummy");
    }

    #[test]
    fn metric_result_serializes() {
        let m = MetricResult { name: "mrr".into(), value: 0.5, target: Some(0.3), passed: true };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"mrr\""));
    }

    #[test]
    fn invariant_result_serializes() {
        let i = InvariantResult { name: "scores_in_range".into(), suite: "risk".into(), passed: true, message: None };
        let json = serde_json::to_string(&i).unwrap();
        assert!(json.contains("\"scores_in_range\""));
    }
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval suites::tests`, verify PASS (this task is create-and-verify, not red-green TDD, since trait + tests live in the same module)
- [ ] Step 3: Run `cargo test -p the-code-graph-eval` to verify no existing tests are broken
- [ ] Step 4: Run `cargo test -p the-code-graph-eval suites::tests`, verify PASS
- [ ] Step 5: `git commit -m "feat(S07/T01): add EvalSuite trait and suite registry"`

---

### Task 2: Extend Report Types + SuiteResult
**Files:** Modify `crates/eval/src/report.rs`
**Traces to:** AC1-AC8 (all suites need result reporting)

- [ ] Step 1: Write failing test in `crates/eval/src/report.rs`
```rust
#[test]
fn suite_result_with_all_fields() {
    let result = SuiteResult {
        search: None, impact: None,
        core: None, flows: None, risk: None,
        analysis: None, invariants: None, bench: None,
    };
    assert!(result.all_passed()); // all None = pass
}

#[test]
fn core_suite_result_serializes() {
    let r = CoreSuiteResult {
        repos: 5, idempotent: true,
        incremental_stable: true,
        import_accuracy: 0.75,
        import_target: 0.70,
        import_passed: true,
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("\"idempotent\""));
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval report::tests`, verify FAIL (new fields don't exist)
- [ ] Step 3: Add to `report.rs`:
  - `CoreSuiteResult { repos, idempotent, incremental_stable, import_accuracy, import_target, import_passed }`
  - `FlowsSuiteResult { repos, entry_point_precision, entry_point_target, entry_point_passed, invariant_violations }`
  - `RiskSuiteResult { repos, top_n_precision, top_n_target, top_n_passed, invariant_violations }`
  - `AnalysisSuiteResult { repos, community_modularity, dead_code_precision, dead_code_target, dead_code_passed, clone_invariant_violations }`
  - `InvariantsSuiteResult { total, passed, failed, results: Vec<InvariantResult> }`
  - `BenchSuiteResult { repos, baselines: serde_json::Value }`
  - `ImpactSuiteResult`: add `recall_target: f64`, `recall_passed: bool` fields (spec requires Recall >= 0.30 as hard gate for AC2)
  - Extend `SuiteResult` with `core: Option<CoreSuiteResult>`, `flows: Option<FlowsSuiteResult>`, etc.
  - Update `all_passed()` to check all suite results (including `impact.recall_passed`)
  - Update `fmt_compact()`, `fmt_table()`, `fmt_json()` formatters
  - **IMPORTANT**: Update ALL existing construction sites:
    - `SuiteResult` (add 6 new `None` fields): `lib.rs::run_suite()`, `report.rs::tests::sample_search()/sample_impact()` callers, `cli/eval.rs::tests::sample_suite_result()`
    - `ImpactSuiteResult` (add `recall_target: 0.30, recall_passed: recall >= 0.30`): `runner.rs::run_impact_suite()` (line ~285), `cli/eval.rs::tests::sample_suite_result()` (line ~117)
    - Without these updates the codebase will not compile.
- [ ] Step 4: Run `cargo test -p the-code-graph-eval report::tests`, verify PASS
- [ ] Step 5: `git commit -m "feat(S07/T02): extend report types for 6 new eval suites"`

---

### Task 3: Extend Suite Enum + CLI
**Files:** Modify `crates/eval/src/lib.rs`, `crates/cli/src/commands/eval.rs`
**Traces to:** AC1-AC8 (CLI dispatches all suites)

- [ ] Step 1: Write failing test in `crates/cli/src/commands/eval.rs`
```rust
#[test]
fn suite_from_string_core() {
    let suite = suite_from_str("core");
    assert!(suite.is_ok());
}

#[test]
fn suite_from_string_flows() {
    let suite = suite_from_str("flows");
    assert!(suite.is_ok());
}

#[test]
fn suite_from_string_risk() {
    let suite = suite_from_str("risk");
    assert!(suite.is_ok());
}

#[test]
fn suite_from_string_analysis() {
    let suite = suite_from_str("analysis");
    assert!(suite.is_ok());
}

#[test]
fn suite_from_string_invariants() {
    let suite = suite_from_str("invariants");
    assert!(suite.is_ok());
}

#[test]
fn suite_from_string_bench() {
    let suite = suite_from_str("bench");
    assert!(suite.is_ok());
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-cli eval::tests`, verify FAIL
- [ ] Step 3: Extend `Suite` enum in `lib.rs` with `Core`, `Flows`, `Risk`, `Analysis`, `Invariants`, `Bench`. Add `compare_baseline: Option<PathBuf>` to `SuiteConfig` (for bench comparison). Update `run_suite()` to dispatch new variants (stub with `todo!()` until suites are implemented). Extend `suite_from_str()` in `eval.rs`. Add `--compare` optional arg to `EvalArgs` for bench baseline comparison, wire it through to `SuiteConfig.compare_baseline`.
- [ ] Step 4: Run `cargo test -p the-code-graph-cli eval::tests`, verify PASS
- [ ] Step 5: `git commit -m "feat(S07/T03): extend Suite enum and CLI for 6 new suites"`

---

### Task 4: Ground Truth Dataset Parsers
**Files:** Modify `crates/eval/src/dataset.rs`
**Traces to:** AC1, AC3, AC4, AC5, AC6 (ground truth loading)

- [ ] Step 1: Write failing test in `crates/eval/src/dataset.rs`
```rust
#[test]
fn parse_entry_point_ground_truth() {
    let json = r#"{"repo":"express","suite":"flows","ground_truth":[
        {"type":"entry_point","symbol":"app.listen","category":"HttpHandler","file":"lib/application.js"}
    ]}"#;
    let gt: FlowsGroundTruth = serde_json::from_str(json).unwrap();
    assert_eq!(gt.ground_truth.len(), 1);
    assert_eq!(gt.ground_truth[0].symbol, "app.listen");
}

#[test]
fn parse_risk_ground_truth() {
    let json = r#"{"repo":"express","suite":"risk","ground_truth":[
        {"symbol":"lib/router/index.js::handle","risk":"high","reason":"central dispatch"}
    ]}"#;
    let gt: RiskGroundTruth = serde_json::from_str(json).unwrap();
    assert_eq!(gt.ground_truth.len(), 1);
}

#[test]
fn parse_import_ground_truth() {
    let json = r#"{"repo":"express","suite":"core","ground_truth":[
        {"source_file":"lib/express.js","source_symbol":"createApplication","target_file":"lib/application.js","target_symbol":"app"}
    ]}"#;
    let gt: CoreGroundTruth = serde_json::from_str(json).unwrap();
    assert_eq!(gt.ground_truth.len(), 1);
}

#[test]
fn parse_dead_code_ground_truth() {
    let json = r#"{"repo":"express","suite":"analysis","type":"dead_code","ground_truth":[
        {"symbol":"lib/utils.js::deprecate","expected_dead":true}
    ]}"#;
    let gt: DeadCodeGroundTruth = serde_json::from_str(json).unwrap();
    assert_eq!(gt.ground_truth.len(), 1);
}

#[test]
fn parse_clone_ground_truth() {
    let json = r#"{"repo":"express","suite":"analysis","type":"clones","ground_truth":[
        {"source":"lib/router/route.js::dispatch","target":"lib/router/layer.js::handle_request"}
    ]}"#;
    let gt: CloneGroundTruth = serde_json::from_str(json).unwrap();
    assert_eq!(gt.ground_truth.len(), 1);
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval dataset::tests`, verify FAIL
- [ ] Step 3: Add to `dataset.rs`:
  - `FlowsGroundTruth { repo, suite, ground_truth: Vec<EntryPointTruth> }` where `EntryPointTruth { type_, symbol, category, file }`
  - `RiskGroundTruth { repo, suite, ground_truth: Vec<RiskTruth> }` where `RiskTruth { symbol, risk, reason }`
  - `CoreGroundTruth { repo, suite, ground_truth: Vec<ImportTruth> }` where `ImportTruth { source_file, source_symbol, target_file, target_symbol }`
  - `DeadCodeGroundTruth { repo, suite, type_, ground_truth: Vec<DeadCodeTruth> }` where `DeadCodeTruth { symbol, expected_dead }`
  - `CloneGroundTruth { repo, suite, type_, ground_truth: Vec<CloneTruth> }` where `CloneTruth { source, target }`
  - Parser functions: `parse_flows_ground_truth()`, `parse_risk_ground_truth()`, `parse_core_ground_truth()`, `parse_dead_code_ground_truth()`, `parse_clone_ground_truth()`
- [ ] Step 4: Run `cargo test -p the-code-graph-eval dataset::tests`, verify PASS
- [ ] Step 5: `git commit -m "feat(S07/T04): add ground truth parsers for 5 new suite formats"`

---

### Task 5: Refactor Search Suite to Trait
**Files:** Create `crates/eval/src/suites/search.rs`, modify `crates/eval/src/runner.rs`, `crates/eval/src/metrics.rs`
**Traces to:** AC1 (search suite must keep passing)

- [ ] Step 1: Write failing test in `crates/eval/src/suites/search.rs`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_suite_name() {
        let suite = SearchSuite;
        assert_eq!(suite.name(), "search");
    }

    #[test]
    fn existence_recall_perfect() {
        let results = vec![vec!["a::Foo".to_string(), "b::Bar".to_string()]];
        let truth = vec![vec!["a::Foo".to_string()]];
        let recall = crate::metrics::existence_recall(&results, &truth);
        assert!((recall - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn existence_recall_miss() {
        let results = vec![vec!["b::Bar".to_string()]];
        let truth = vec![vec!["a::Foo".to_string()]];
        let recall = crate::metrics::existence_recall(&results, &truth);
        assert!((recall - 0.0).abs() < f64::EPSILON);
    }
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval suites::search::tests`, verify FAIL
- [ ] Step 3: Implement `SearchSuite` struct implementing `EvalSuite`. Move search-specific logic from `runner.rs::run_search_suite()` into `SearchSuite::run_metrics()`. In `run_metrics()`, filter queries by `category == "existence"` and compute `existence_recall()` separately from ranked metrics (MRR, P@5, P@10 apply to non-existence queries). Add search invariants (valid symbol IDs, FTS index coverage via `store.stats().symbols` vs FTS count) to `run_invariants()`. Add `existence_recall()` to `metrics.rs`. Keep `run_search_suite()` in `runner.rs` as a thin wrapper calling the trait for backward compat.
- [ ] Step 4: Run `cargo test -p the-code-graph-eval`, verify all existing tests still PASS
- [ ] Step 5: `git commit -m "refactor(S07/T05): extract SearchSuite implementing EvalSuite trait"`

---

### Task 6: Refactor Impact Suite to Trait
**Files:** Create `crates/eval/src/suites/impact.rs`
**Traces to:** AC2 (impact suite must keep passing)

- [ ] Step 1: Write failing test in `crates/eval/src/suites/impact.rs`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_suite_name() {
        let suite = ImpactSuite;
        assert_eq!(suite.name(), "impact");
    }
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval suites::impact::tests`, verify FAIL
- [ ] Step 3: Implement `ImpactSuite` struct implementing `EvalSuite`. Move impact-specific logic from `runner.rs::run_impact_suite()` into `ImpactSuite::run_metrics()`. Add impact invariants (subset-of-reachable, no self-referential) to `run_invariants()`. Wire `recall_target` (0.30) and `recall_passed` into `ImpactSuiteResult` (added in T02). Update `run_impact_suite()` in `runner.rs` to populate the new recall gate fields.
- [ ] Step 4: Run `cargo test -p the-code-graph-eval`, verify all existing tests still PASS
- [ ] Step 5: `git commit -m "refactor(S07/T06): extract ImpactSuite implementing EvalSuite trait"`

---

### Task 7: Core Suite — Indexing Idempotency + Import Resolution
**Files:** Create `crates/eval/src/suites/core.rs`, create `eval/suites/core/manifest.json`, create `eval/suites/core/ground-truth/*.json`
**Traces to:** AC6 (indexing idempotent, incremental stable, import accuracy >= 0.70)

- [ ] Step 1: Write failing test in `crates/eval/src/suites/core.rs`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_suite_name() {
        let suite = CoreSuite;
        assert_eq!(suite.name(), "core");
    }

    #[test]
    fn idempotency_check_same_counts() {
        assert!(check_idempotency(100, 200, 500, 100, 200, 500));
    }

    #[test]
    fn idempotency_check_different_counts() {
        assert!(!check_idempotency(100, 200, 500, 101, 200, 500));
    }
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval suites::core::tests`, verify FAIL
- [ ] Step 3: Implement `CoreSuite`:
  - `run_metrics()`: Per repo: (a) call `index_repo(clone_path)` -> `(store1, _tmp1)`, get `GraphStats` via `store1.stats()` (fields: `files`, `symbols`, `edges`), (b) call `index_repo(clone_path)` again (creates a fresh temp DB) -> `(store2, _tmp2)`, get `GraphStats` via `store2.stats()`, (c) compare `files`, `symbols`, `edges` counts for idempotency. (d) Incremental no-op: construct a new `IndexUseCase` with `store1`, `EvalParseProvider`, `EvalFileSystem`, `NoOpGitProvider`, call `incremental_index(clone_path)` — because `NoOpGitProvider` returns no changed files, this trivially produces zero changes; the test validates the code path doesn't crash and returns `IndexStats` with zeros (per spec: "incremental on unchanged repo produces zero changes"). (e) Import resolution: load `core/ground-truth/{repo}.json`, check each import chain exists in edges.
  - `run_invariants()`: Every edge references valid source/target symbol IDs. All files visited. Symbol count > 0 per parseable file.
  - Create `eval/suites/core/manifest.json` (same 5 repos, same revisions as search suite).
  - Create placeholder ground truth files (populated during execution phase when repos are actually indexed and inspected).
- [ ] Step 4: Run `cargo test -p the-code-graph-eval suites::core::tests`, verify PASS
- [ ] Step 5: `git commit -m "feat(S07/T07): add Core eval suite with idempotency and import validation"`

---

### Task 8: Flows Suite — Entry Points + Path Validity
**Files:** Create `crates/eval/src/suites/flows.rs`, create `eval/suites/flows/manifest.json`, create `eval/suites/flows/ground-truth/*.json`
**Traces to:** AC3 (entry point precision >= 0.80, flow invariants)

- [ ] Step 1: Write failing test in `crates/eval/src/suites/flows.rs`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flows_suite_name() {
        let suite = FlowsSuite;
        assert_eq!(suite.name(), "flows");
    }

    #[test]
    fn entry_point_precision_perfect() {
        let detected = vec!["main".to_string(), "handler".to_string()];
        let expected = vec!["main".to_string(), "handler".to_string()];
        let p = entry_point_precision(&detected, &expected);
        assert!((p - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn flow_path_is_acyclic() {
        let path = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert!(is_acyclic(&path));
    }

    #[test]
    fn flow_path_with_cycle() {
        let path = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        assert!(!is_acyclic(&path));
    }
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval suites::flows::tests`, verify FAIL
- [ ] Step 3: Implement `FlowsSuite`:
  - `run_metrics()`: Per repo: run `FlowUseCase::new(store.clone()).analyze(&FlowConfig::default())`, load `flows/ground-truth/{repo}.json`, compute entry point precision (detected vs tagged).
  - `run_invariants()`: Betweenness scores non-negative. Flow paths acyclic. Every flow starts at entry point. `CriticalityScore.betweenness` in [0.0, 1.0].
  - Create `eval/suites/flows/manifest.json` and placeholder ground truth files.
- [ ] Step 4: Run `cargo test -p the-code-graph-eval suites::flows::tests`, verify PASS
- [ ] Step 5: `git commit -m "feat(S07/T08): add Flows eval suite with entry point and path validation"`

---

### Task 9: Risk Suite — Score Validation
**Files:** Create `crates/eval/src/suites/risk.rs`, create `eval/suites/risk/manifest.json`, create `eval/suites/risk/ground-truth/*.json`
**Traces to:** AC4 (risk scores in [0,1], top-N precision >= 0.60)

- [ ] Step 1: Write failing test in `crates/eval/src/suites/risk.rs`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_suite_name() {
        let suite = RiskSuite;
        assert_eq!(suite.name(), "risk");
    }

    #[test]
    fn top_n_precision_perfect() {
        let scored = vec![("a".to_string(), 0.9), ("b".to_string(), 0.8)];
        let high_risk = vec!["a".to_string(), "b".to_string()];
        let p = top_n_precision(&scored, &high_risk, 2);
        assert!((p - 1.0).abs() < f64::EPSILON);
    }
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval suites::risk::tests`, verify FAIL
- [ ] Step 3: Implement `RiskSuite`:
  - `run_metrics()`: Per repo: run `RiskUseCase::new(store.clone()).analyze(&RiskConfig::default())`, load `risk/ground-truth/{repo}.json`, sort by composite desc, compute top-N precision vs tagged high-risk symbols.
  - `run_invariants()`: All `composite` in [0.0, 1.0]. All `RiskFactors` components in [0.0, 1.0]. Zero-edge symbols have risk < 0.2.
  - Create `eval/suites/risk/manifest.json` and placeholder ground truth files.
- [ ] Step 4: Run `cargo test -p the-code-graph-eval suites::risk::tests`, verify PASS
- [ ] Step 5: `git commit -m "feat(S07/T09): add Risk eval suite with score validation"`

---

### Task 10: Analysis Suite — Communities, Dead Code, Clones
**Files:** Create `crates/eval/src/suites/analysis.rs`, create `eval/suites/analysis/manifest.json`, create `eval/suites/analysis/ground-truth/*.json`
**Traces to:** AC5 (community modularity > 0, dead code precision >= 0.70, clone invariants)

- [ ] Step 1: Write failing test in `crates/eval/src/suites/analysis.rs`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_suite_name() {
        let suite = AnalysisSuite;
        assert_eq!(suite.name(), "analysis");
    }

    #[test]
    fn dead_code_precision_all_correct() {
        let detected = vec!["a".to_string(), "b".to_string()];
        let tagged = vec!["a".to_string(), "b".to_string()];
        let p = dead_code_precision(&detected, &tagged);
        assert!((p - 1.0).abs() < f64::EPSILON);
    }
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval suites::analysis::tests`, verify FAIL
- [ ] Step 3: Implement `AnalysisSuite`:
  - `run_metrics()`:
    - Communities: run `CommunityUseCase::new(store.clone()).analyze(&CommunityConfig::default())`, report modularity score.
    - Dead code: run `DeadCodeUseCase::new(store.clone()).analyze(&DeadCodeConfig::default())`, load `analysis/ground-truth/{repo}-dead-code.json`, compute precision.
    - Clones: construct `CloneUseCase::new(store.clone(), EvalFileSystem, clone_path.to_path_buf())` (EvalFileSystem is a stateless unit struct from `crate::adapters`), run `.analyze(&CloneConfig::default())`, load `analysis/ground-truth/{repo}-clones.json`, compute precision.
  - `run_invariants()`:
    - Communities: non-isolated symbols in exactly one community, no empty communities, count < total symbols.
    - Dead code: every reported dead symbol has zero incoming edges.
    - Clones: similarity scores in [0.0, 1.0], no contradictory asymmetric pairs.
  - Create manifests and placeholder ground truth.
- [ ] Step 4: Run `cargo test -p the-code-graph-eval suites::analysis::tests`, verify PASS
- [ ] Step 5: `git commit -m "feat(S07/T10): add Analysis eval suite for communities, dead code, clones"`

---

### Task 11: Invariants Meta-Suite
**Files:** Create `crates/eval/src/suites/invariants.rs`
**Traces to:** AC7 (all invariants hold with zero violations)

- [ ] Step 1: Write failing test in `crates/eval/src/suites/invariants.rs`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invariants_suite_name() {
        let suite = InvariantsSuite;
        assert_eq!(suite.name(), "invariants");
    }

    #[test]
    fn collects_from_all_suites() {
        // all_suites() must register all suites implemented so far
        // This test is updated in T13 (wire dispatch) to assert >= 8 once all suites are registered
        let suites = super::super::all_suites();
        assert!(!suites.is_empty(), "all_suites() must have registered suites");
    }
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval suites::invariants::tests`, verify FAIL
- [ ] Step 3: Implement `InvariantsSuite`:
  - `run_metrics()`: Returns empty (no ground-truth-based metrics).
  - `run_invariants()`: Iterates all registered suites via `all_suites()`, calls `run_invariants()` on each, collects all `InvariantResult`s. Reports per-invariant pass/fail. Zero tolerance: any violation = fail.
- [ ] Step 4: Run `cargo test -p the-code-graph-eval suites::invariants::tests`, verify PASS
- [ ] Step 5: `git commit -m "feat(S07/T11): add Invariants meta-suite collecting all property checks"`

---

### Task 12: Bench Suite — Performance Baselines
**Files:** Create `crates/eval/src/suites/bench.rs`
**Traces to:** AC8 (baseline JSON produced, comparison mode functional)

- [ ] Step 1: Write failing test in `crates/eval/src/suites/bench.rs`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_suite_name() {
        let suite = BenchSuite::new(None);
        assert_eq!(suite.name(), "bench");
    }

    #[test]
    fn baseline_entry_serializes() {
        let entry = BaselineEntry {
            repo: "express".into(),
            full_index_ms: 1500,
            incremental_noop_ms: 50,
            query_latencies: QueryLatencies {
                search_p50_ms: 5.0, search_p95_ms: 12.0,
                impact_p50_ms: 8.0, impact_p95_ms: 20.0,
                flows_p50_ms: 15.0, flows_p95_ms: 30.0,
                callers_p50_ms: 2.0, callers_p95_ms: 5.0,
                callees_p50_ms: 2.0, callees_p95_ms: 5.0,
            },
            graph_size: GraphSize { symbols: 1000, edges: 5000, db_bytes: 1_000_000 },
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"full_index_ms\""));
    }

    #[test]
    fn percentile_p50() {
        let mut values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        assert!((percentile(&mut values, 50.0) - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_p95() {
        let mut values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let p95 = percentile(&mut values, 95.0);
        assert!((p95 - 95.0).abs() < 1.0);
    }
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval suites::bench::tests`, verify FAIL
- [ ] Step 3: Implement `BenchSuite`:
  - `BenchSuite::new(compare_path: Option<PathBuf>)` -- optional baseline for comparison.
  - `run_metrics()`: Per repo: measure full index time, incremental no-op time, query latencies (search, impact, flows, callers, callees -- 10 runs each, compute p50/p95), graph size (symbol count, edge count, DB file size). Write `eval/baselines/baseline-{version}.json`. If `compare_path` is set, load previous baseline and compute deltas.
  - `run_invariants()`: Returns empty (bench has no invariants).
  - Helper: `percentile(values, pct)` for p50/p95 computation.
- [ ] Step 4: Run `cargo test -p the-code-graph-eval suites::bench::tests`, verify PASS
- [ ] Step 5: `git commit -m "feat(S07/T12): add Bench suite with performance baselines and comparison"`

---

### Task 13: Wire Suite Dispatch in run_suite()
**Files:** Modify `crates/eval/src/lib.rs`
**Traces to:** AC1-AC8 (all suites must be dispatchable)

- [ ] Step 1: Write failing test
```rust
#[test]
fn run_suite_dispatches_core() {
    // This test verifies the dispatch path exists -- actual execution
    // requires repos and is covered by integration tests
    let config = SuiteConfig {
        suite: Suite::Core,
        no_cache: false,
        suites_dir: PathBuf::from("/tmp/nonexistent"),
        search_limit: 20,
    };
    // Should fail with "manifest not found" rather than "unknown suite"
    let err = run_suite(&config).unwrap_err();
    let msg = format!("{err}");
    assert!(!msg.contains("Unknown suite"), "dispatch should reach Core suite, got: {msg}");
}
```
- [ ] Step 2: Run `cargo test -p the-code-graph-eval tests::run_suite_dispatches_core`, verify FAIL
- [ ] Step 3: Update `run_suite()` in `lib.rs` to dispatch `Suite::Core`, `Suite::Flows`, `Suite::Risk`, `Suite::Analysis`, `Suite::Invariants`, `Suite::Bench` to their respective suite implementations. The `Suite::All` variant runs all suites sequentially. Update `all_suites()` registry in `suites/mod.rs` to return all 8 suite instances. Add test: `assert!(all_suites().len() >= 8)` to catch missed registrations.
- [ ] Step 4: Run `cargo test -p the-code-graph-eval`, verify PASS (all tests including new dispatch test and registry completeness)
- [ ] Step 5: `git commit -m "feat(S07/T13): wire all suite dispatch paths in run_suite()"`

---

### Task 14: Ground Truth Curation — Search Existence Queries
**Files:** Modify `eval/suites/search/queries/*.json`
**Traces to:** AC1 (100% recall for existence queries)

- [ ] Step 1: Define expected test: existence queries added to each language file with `"category": "existence"`, 20-30 per repo.
- [ ] Step 2: Run `cargo test -p the-code-graph-eval search_query_count_meets_minimum`, verify current count (should pass -- adding more queries).
- [ ] Step 3: For each repo, index it, inspect symbols via `tcg search`, and curate 20-30 existence queries (exact/partial symbol name substrings). Add to existing query files with `"category": "existence"`.
- [ ] Step 4: Run `cargo test -p the-code-graph-eval search_query_count_meets_minimum`, verify PASS with increased count.
- [ ] Step 5: `git commit -m "feat(S07/T14): curate existence queries for search suite (100+ queries)"`

---

### Task 15: Ground Truth Curation — Flows, Risk, Analysis, Core
**Files:** Create `eval/suites/{flows,risk,analysis,core}/ground-truth/*.json`
**Traces to:** AC3, AC4, AC5, AC6

- [ ] Step 1: Define expected: each ground truth file parses successfully via the dataset parsers from T04.
- [ ] Step 2: Run `cargo test -p the-code-graph-eval dataset::tests`, verify parsers pass (from T04).
- [ ] Step 3: For each repo + suite, index the repo, run the relevant analysis, inspect results against actual code, and curate ground truth:
  - Flows: 10-20 entry points per repo (main, handlers, tests, CLI, public root exports)
  - Risk: 5-10 tagged high/low risk symbols per repo
  - Dead code: 5-10 tagged dead symbols per repo
  - Clones: 3-5 known copy-paste pairs per repo
  - Core: 10-15 cross-file import chains per repo
- [ ] Step 4: Run validation: parse all ground truth files, confirm counts meet minimums.
- [ ] Step 5: `git commit -m "feat(S07/T15): curate ground truth for flows, risk, analysis, core suites"`

---

### Task 16: Integration Validation + Bug Fixes
**Files:** Various (bug fixes in domain/parser/storage crates)
**Traces to:** AC9 (bugs fixed or documented), AC10 (CI passes)

- [ ] Step 1: Run `tcg eval all` against all 5 repos. Capture results.
- [ ] Step 2: For each suite failure:
  - If crash/panic: fix immediately.
  - If wrong results and root cause is clear: fix and add regression test.
  - If complex: log as known issue with failing test in VERIFICATION.md.
  - If parser changes affecting >1 language: defer to separate slice.
- [ ] Step 3: Re-run `tcg eval all`, iterate until all hard gates pass.
- [ ] Step 4: Run `cargo clippy --workspace && cargo fmt --check && cargo test --workspace`, verify CI passes.
- [ ] Step 5: `git commit -m "fix(S07/T16): fix bugs found during real-world validation"`

---

### Task 17: VERIFICATION.md + Final Report
**Files:** Create `.tff/milestones/M03/slices/M03-S07/VERIFICATION.md`
**Traces to:** AC9, AC10

- [ ] Step 1: Create VERIFICATION.md listing all bugs found with disposition.
- [ ] Step 2: Run `tcg eval all` one final time, capture full output.
- [ ] Step 3: Record final metrics in VERIFICATION.md:
  - Search: MRR, P@5, P@10, existence recall (all 5 repos)
  - Impact: precision, recall, F1 (all 5 repos)
  - Flows: entry point precision (all 5 repos)
  - Risk: top-N precision (all 5 repos)
  - Analysis: modularity, dead code precision (all 5 repos)
  - Core: idempotency, import accuracy (all 5 repos)
  - Invariants: total/passed/failed
  - Bench: baseline JSON path
- [ ] Step 4: Verify all ACs: AC1-AC10 met.
- [ ] Step 5: `git commit -m "docs(S07/T17): add VERIFICATION.md with validation results"`
