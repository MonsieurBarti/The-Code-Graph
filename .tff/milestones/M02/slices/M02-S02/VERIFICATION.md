# M02-S02: Risk Scoring -- Acceptance Criteria Verification

**Reviewer**: tff-spec-reviewer (fresh -- did not author this code)
**Date**: 2026-03-28
**Worktree**: `/Users/monsieurbarti/Projects/The-Code-Graph/.tff/worktrees/M02-S02`
**Spec**: `/Users/monsieurbarti/Projects/The-Code-Graph/.tff/milestones/M02/slices/M02-S02/SPEC.md`

---

## Summary

| AC | Verdict | Short |
|----|---------|-------|
| AC1 | PASS | `code-graph risk` lists files by descending composite score |
| AC2 | PASS | `code-graph risk --symbols` lists symbols by descending risk |
| AC3 | PASS | Single-target output shows matched patterns and active weights via RiskScoreDetail |
| AC4 | PASS | Composite = weighted linear sum, clamped [0.0, 1.0] |
| AC5 | PASS | Criticality delegates to `brandes_betweenness` |
| AC6 | PASS | Coupling uses non-structural edges only, both endpoints, max_degree=0 guard |
| AC7 | PASS | Test gap = 1.0 if no TestedBy, 0.0 otherwise |
| AC8 | PASS | Sensitivity matches qualified_name and decorators, word-boundary, case-insensitive |
| AC9 | PASS | Weights configurable via `[risk]` section in config.toml |
| AC10 | PASS | extra_security_patterns adds, excluded_security_patterns removes |
| AC11 | PASS | File score = max of symbol composites; zero-symbol files excluded |
| AC12 | PASS | --min-score filters output >= threshold (inclusive) |
| AC13 | PASS | Three output formats: compact, --table, --json |
| AC14 | PASS | `code-graph stats` shows avg_risk and p90_risk |
| AC15 | PASS | Dogfood: `code-graph risk` exits 0 with 125 files; `--symbols` exits 0 with 1153 symbols |

**Result: 15 PASS, 0 FAIL, 0 DEFER**

---

## Detailed Verdicts

### AC1: `code-graph risk` lists files ranked by composite risk score (descending)

**Verdict: PASS**

- `crates/cli/src/commands/risk.rs:62-71` -- default branch calls `uc.analyze()`, iterates `file_scores`, applies `min_score` filter, truncates to `limit`, prints as `RiskAnalysis`.
- `crates/domain/src/analysis/risk.rs:217-222` -- `aggregate_file_scores()` sorts `file_scores` descending by composite.
- `crates/cli/src/output.rs:655-697` -- `Displayable for RiskAnalysis` renders `file_scores` as "Files by risk (top N of M)" table.
- CLI test at `crates/cli/src/commands/mod.rs:354-357` -- `parse_risk_command` verifies parsing.

### AC2: `code-graph risk --symbols` lists symbols ranked by risk (descending)

**Verdict: PASS**

- `crates/cli/src/commands/risk.rs:52-61` -- `--symbols` branch calls `uc.analyze()`, filters by `min_score`, takes `limit`, prints as `Vec<RiskScore>`.
- `crates/domain/src/analysis/risk.rs:170-175` -- `score_symbols()` sorts descending by composite.
- `crates/cli/src/output.rs:703-748` -- `Displayable for Vec<RiskScore>` renders columns: # Symbol Risk Crit Coup Test Sec.
- CLI test at `crates/cli/src/commands/mod.rs:360-369` -- `parse_risk_symbols` verifies `--symbols --limit 50`.

### AC3: Single target shows all four factor values, composite score, matched patterns, and active weights

**Verdict: PASS**

- `crates/cli/src/commands/risk.rs:49-69` -- target branch calls `uc.score_symbol()`, computes matched patterns via `split_into_segments`, wraps result in `RiskScoreDetail { score, matched_patterns, weights }`.
- `crates/cli/src/output.rs:10-14` -- `RiskScoreDetail` struct carries `score`, `matched_patterns`, and `weights`.
- `crates/cli/src/output.rs:761-801` -- `Displayable for RiskScoreDetail` prints all four factors, composite, matched pattern names (e.g., `matches: auth`), and active weights (e.g., `weights: crit=0.30 coup=0.25 test=0.25 sec=0.20`).
- Verified via dogfood: `code-graph risk "crates/benches/fixtures/auth_service.py::pwd_context"` outputs `sensitivity: 1.00 (matches: auth)` and `weights: crit=0.30 coup=0.25 test=0.25 sec=0.20`.

### AC4: Composite score = weighted linear sum, clamped to [0.0, 1.0]

**Verdict: PASS**

- `crates/domain/src/analysis/risk.rs:136-176` -- `score_symbols()` computes `composite = (w.criticality * crit + w.coupling * coup + w.test_gap * tgap + w.sensitivity * sens).clamp(0.0, 1.0)`.
- Weights are normalized at line 144: `let w = weights.normalized();`.
- Test `test_score_symbols_weighted_sum` at line 442-456 verifies `0.30*0.8 + 0.25*0.6 + 0.25*1.0 + 0.20*0.5 = 0.74`.

### AC5: Criticality values equal betweenness centrality scores from `brandes_betweenness()`

**Verdict: PASS**

- `crates/domain/src/analysis/risk.rs:11-14` -- `compute_criticality_scores()` calls `brandes_betweenness(&nodes, &edges)` directly.
- Test `test_criticality_delegates_to_brandes` at line 292-306 verifies a 3-node chain (A->B->C) where B has the highest betweenness.

### AC6: Coupling factor uses non-structural edges, both endpoints must be symbols, max_degree=0 guard

**Verdict: PASS**

- `crates/domain/src/analysis/risk.rs:20-57` -- `compute_coupling_scores()`:
  - Line 27: Filters out structural edges via `e.kind.confidence() != Confidence::Structural`.
  - Line 29: Requires both `source` and `target` in `symbol_set`.
  - Line 42-48: `max_degree == 0` returns all 0.0.
  - Line 50-56: Normalizes by `deg / max_degree`.
- Note: The spec lists "Contains, ChildOf, HasDecorator" as excluded. The code excludes ALL structural-confidence edges, which also includes `TestedBy` (model.rs:122). This is consistent with AC6's wording "non-structural edges only" since `TestedBy` is classified as Structural in the model.
- Tests: `test_coupling_excludes_structural_edges` (line 309), `test_coupling_both_endpoints_must_be_symbols` (line 323), `test_coupling_max_degree_zero` (line 334), `test_coupling_normalization` (line 343).

### AC7: Test gap = 1.0 if no incoming TestedBy edges, 0.0 if tested

**Verdict: PASS**

- `crates/domain/src/analysis/risk.rs:60-79` -- `compute_test_gaps()` collects tested symbols by filtering edges for `EdgeKind::TestedBy`, checking `e.target`, returns 1.0 (untested) or 0.0 (tested).
- Tests: `test_test_gap_untested` (line 366), `test_test_gap_tested` (line 374).

### AC8: Sensitivity matches qualified_name and decorators, case-insensitive word-boundary match

**Verdict: PASS**

- `crates/domain/src/analysis/risk.rs:83-133` -- `compute_sensitivity()`:
  - `split_into_segments()` splits on `_`, `.`, `::`, `/`, and camelCase boundaries, lowercased.
  - Matches `qualified_name` segments AND decorator segments.
  - `segment.starts_with(pattern)` -- word-boundary prefix match, not substring.
  - Case-insensitive via lowercasing both segments and patterns.
- Tests: `test_sensitivity_word_boundary` (line 382), `test_sensitivity_camel_case` (line 406), `test_sensitivity_decorators` (line 414), `test_sensitivity_no_match` (line 424), `test_split_segments` (line 432).

### AC9: Weights configurable via `.code-graph/config.toml` `[risk]` section

**Verdict: PASS**

- `crates/cli/src/config.rs:35-43` -- `RiskCliConfig` with weight_criticality, weight_coupling, weight_test_gap, weight_sensitivity fields, all `Option<f64>`.
- `crates/cli/src/config.rs:11` -- `CodeGraphConfig` has `pub risk: Option<RiskCliConfig>`.
- `crates/cli/src/commands/risk.rs:17-44` -- `run_risk()` loads config, overrides default weights with config values, normalizes weights.
- Test `risk_config_parses` at config.rs:129-155 verifies full config roundtrip.

### AC10: extra_security_patterns adds to built-in; excluded_security_patterns removes from combined list

**Verdict: PASS**

- `crates/cli/src/commands/risk.rs:33-40` -- Extra patterns are appended via `extend()`, excluded patterns are removed via `retain(|p| !excluded.contains(p))`.
- This is pattern-level exclusion (removes patterns from the list, not symbols from results), matching the spec's semantics.
- Test `risk_config_parses` verifies the config fields parse correctly.

### AC11: File-level score = max of contained symbol composites; zero-symbol files excluded

**Verdict: PASS**

- `crates/domain/src/analysis/risk.rs:181-223` -- `aggregate_file_scores()`:
  - Groups by file path from `SymbolNode.location.file`.
  - Tracks max composite per file (line 199-201).
  - Files with zero symbols never appear in the map (only symbols with scores are iterated).
  - Sorts descending.
- Test `test_aggregate_file_scores` at line 459-488 verifies two symbols in one file, file score = max.

### AC12: --min-score flag filters output >= threshold (inclusive)

**Verdict: PASS**

- `crates/cli/src/commands/mod.rs:122-123` -- `RiskArgs` has `--min-score` with default 0.0.
- `crates/cli/src/commands/risk.rs:58` -- symbols mode: `.filter(|s| s.composite >= args.min_score)` -- inclusive (>=).
- `crates/cli/src/commands/risk.rs:68` -- files mode: `.retain(|f| f.composite >= args.min_score)` -- inclusive (>=).
- CLI test `parse_risk_min_score` at commands/mod.rs:381-389.

### AC13: Three output formats: compact (default), --table, --json

**Verdict: PASS**

- `crates/cli/src/output.rs:655-697` -- `Displayable for RiskAnalysis` implements `fmt_compact`, `fmt_table`, `fmt_json`.
- `crates/cli/src/output.rs:703-748` -- `Displayable for Vec<RiskScore>` implements all three.
- `crates/cli/src/output.rs:754-815` -- `Displayable for RiskScore` (single target) implements all three.
- JSON outputs use `serde_json::to_string_pretty` over Serialize-derived types.
- Tests: `risk_analysis_compact_format` (line 1532), `risk_analysis_json_format` (line 1544), `risk_score_vec_compact_format` (line 1555), `risk_score_single_compact_format` (line 1575).

### AC14: `code-graph stats` shows avg_risk and p90_risk

**Verdict: PASS**

- `crates/cli/src/commands/stats.rs:52-59` -- Computes risk analysis (guarded by `symbols <= 5000`), sets `stats.avg_risk` and `stats.p90_risk`.
- `crates/domain/src/model.rs:270-274` -- `GraphStats` has `avg_risk: Option<f64>` and `p90_risk: Option<f64>` with `skip_serializing_if`.
- `crates/cli/src/output.rs:232-238` -- Compact format renders "Avg risk: X.XX | P90 risk: X.XX".
- `crates/cli/src/output.rs:262-268` -- Table format renders "Avg risk | X.XX" and "P90 risk | X.XX".
- Tests: `graph_stats_compact_with_risk_fields` (output.rs:1458), `graph_stats_table_with_risk_fields` (output.rs:1479).

### AC15: Dogfood -- risk exits 0 with results on codebase/fixtures

**Verdict: PASS**

- `cargo run --release -- index` indexed 130 files, 1155 symbols, 1583 edges.
- `cargo run --release -- risk` exits 0, produces 125 file results (top: `crates/cli/src/output.rs` at 0.50).
- `cargo run --release -- risk --symbols` exits 0, produces 1153 symbol results (top: `RiskScoreDetail` at 0.50).
- `cargo run --release -- stats` shows `Avg risk: 0.28 | P90 risk: 0.45`.
- All 637 tests pass across 15 suites.

---

## Structural Observations (not AC failures, noted for completeness)

1. **RiskAnalysis omits `config` field**: The spec's domain model definition shows `config: RiskConfig` in `RiskAnalysis`, but the implementation at model.rs:562-567 does not include it. This is a minor deviation from the spec struct definition but does not violate any AC directly.

2. **Coupling excludes TestedBy**: The spec names three edges to exclude (Contains, ChildOf, HasDecorator). The implementation excludes all Structural-confidence edges, which also captures TestedBy. Since AC6 says "non-structural edges only" and TestedBy is Structural, this is consistent with AC6.

3. **`is_multiple_of` nightly feature**: `crates/domain/src/analysis/risk.rs:240` uses `n.is_multiple_of(2)` which is a nightly/unstable method on `usize`. This compiles in the project's current toolchain but is worth noting for portability.

---

## AC3 Resolution

The AC3 issues identified in the initial review have been resolved:
1. **Matched patterns**: `RiskScoreDetail` carries `matched_patterns: Vec<String>`, computed in `run_risk()` via `split_into_segments` + pattern matching.
2. **Active weights**: `RiskScoreDetail` carries `weights: RiskWeights`, passed from the normalized config.
Both are displayed in compact, table, and JSON output formats.
