# Plan — M01-S09: Eval Framework & CI/CD

> For agentic workers: execute task-by-task with TDD.

**Goal:** Implement the eval framework (R8) and CI/CD pipeline (R9). New `eval` crate with metrics, dataset management, runner, and report. JSON-manifest-driven evaluation against 5 open-source repos (50+ search queries, 20+ impact scenarios). Lefthook for local hooks, GitHub Actions for PR checks and release automation.

**Architecture:** New `eval` crate (7th workspace member) — library, not CLI tool. CLI delegates to `eval::run_suite()`. Clone-at-eval with local caching. Two GitHub Actions workflows (ci.yml, release.yml). Eval runs as release gate.

**Key decisions (from RESEARCH):**
- Shell-out to `git` for cloning — matches existing `ShellGitProvider` pattern, avoids `git2` C dependency
- `tempfile = "3"` for isolated per-repo SQLite databases — RAII cleanup on drop
- Eval crate owns its own adapter implementations (`EvalFileSystem`, `EvalParseProvider`, `NoOpGitProvider`) to avoid circular dependency with cli — adds `ignore`, `rayon`, `sha2` from workspace deps
- `Displayable` trait for eval output — same pattern as all other commands
- Cache at `~/.cache/code-graph-eval/<repo>/<revision>/` with `.revision` marker file
- Lefthook `commands` key (SPEC format) — works in both v1 and v2

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `Cargo.toml` (root) | Modify | Add `"crates/eval"` to workspace members |
| `crates/eval/Cargo.toml` | Create | Crate manifest (depends on domain, parser, storage, serde, serde_json, tracing, tempfile, ignore, rayon, sha2) |
| `crates/eval/src/lib.rs` | Create | Public API: `run_suite()`, `SuiteConfig`, re-exports |
| `crates/eval/src/metrics.rs` | Create | Pure metric functions: MRR, precision@k, blast_precision, blast_recall, F1 |
| `crates/eval/src/dataset.rs` | Create | Manifest types, JSON parsing, repo clone/cache management |
| `crates/eval/src/runner.rs` | Create | Orchestration: index repo -> validate ground truth -> run queries -> collect results |
| `crates/eval/src/report.rs` | Create | `SuiteResult`, `SearchMetrics`, `ImpactMetrics`, quality gate checks |
| `crates/eval/src/adapters.rs` | Create | Eval-owned adapters: `EvalFileSystem`, `EvalParseProvider`, `NoOpGitProvider` |
| `crates/cli/Cargo.toml` | Modify | Add `eval = { path = "../eval" }` |
| `crates/cli/src/commands/mod.rs` | Modify | Add `EvalArgs`, change `Eval` to `Eval(EvalArgs)`, update tests |
| `crates/cli/src/commands/eval.rs` | Create | CLI command handler: parse args, delegate to eval crate, format output |
| `crates/cli/src/lib.rs` | Modify | Wire `Commands::Eval(args)` dispatch |
| `eval/suites/search/manifest.json` | Create | Search suite repo manifest (5 repos) |
| `eval/suites/search/queries/javascript.json` | Create | Express search queries (10+) |
| `eval/suites/search/queries/typescript.json` | Create | tRPC search queries (10+) |
| `eval/suites/search/queries/rust.json` | Create | ripgrep search queries (10+) |
| `eval/suites/search/queries/python.json` | Create | FastAPI search queries (10+) |
| `eval/suites/search/queries/go.json` | Create | Go stdlib search queries (10+) |
| `eval/suites/impact/manifest.json` | Create | Impact suite repo manifest (5 repos) |
| `eval/suites/impact/queries/javascript.json` | Create | Express impact scenarios (4+) |
| `eval/suites/impact/queries/typescript.json` | Create | tRPC impact scenarios (4+) |
| `eval/suites/impact/queries/rust.json` | Create | ripgrep impact scenarios (4+) |
| `eval/suites/impact/queries/python.json` | Create | FastAPI impact scenarios (4+) |
| `eval/suites/impact/queries/go.json` | Create | Go stdlib impact scenarios (4+) |
| `lefthook.yml` | Create | Git hooks: pre-commit (fmt, clippy, test), pre-push (full-test, bench) |
| `.github/workflows/ci.yml` | Create | PR checks: fmt, clippy, test (matrix), coverage, audit, bench |
| `.github/workflows/release.yml` | Create | Release: eval gate, build 4 targets (cross-rs for aarch64-linux), publish |

---

## Acceptance Criteria

> Numbering follows SPEC.md.

### Eval Crate
- AC1: `eval` crate added as 7th workspace member with metrics, dataset, runner, report modules
- AC2: `code-graph eval --suite search` runs against 5 repos with 50+ queries
- AC3: Search suite reports MRR and precision@k metrics
- AC4: `code-graph eval --suite impact` runs against 5 repos with 20+ scenarios
- AC5: Impact suite reports precision, recall, and F1 metrics
- AC6: Search MRR > 0.30 on the curated dataset (M01 baseline)
- AC7: Blast radius precision > 0.40 at high confidence (M01 baseline)
- AC8: Eval supports compact, table, and JSON output formats
- AC9: `code-graph eval --no-cache` re-clones repos even when cache exists
- AC10: Eval validates ground truth before computing metrics — mismatched qualified names reported as setup errors
- AC11: Eval repo caching works (second run doesn't re-clone)

### CI/CD
- AC12: `lefthook.yml` runs fmt + clippy + test on pre-commit
- AC13: `lefthook.yml` runs full-test + bench-check on pre-push
- AC14: GitHub Actions CI workflow passes (fmt, clippy, test, coverage, audit, bench)
- AC15: Coverage gate enforces 80% minimum line coverage
- AC16: GitHub Actions release workflow builds 4 targets (including aarch64-linux via cross-rs) and creates GitHub Release
- AC17: Eval runs as release gate — release blocked if quality thresholds fail
- AC18: Release workflow includes `cargo publish` step for crates.io

---

## Wave 0 — Crate Setup + CLI Wiring

### T01: Eval crate skeleton + workspace member + CLI wiring
**AC coverage:** AC1 (partially), structural prerequisite
**Files:** `Cargo.toml`, `crates/eval/Cargo.toml`, `crates/eval/src/lib.rs`, `crates/eval/src/metrics.rs`, `crates/eval/src/dataset.rs`, `crates/eval/src/runner.rs`, `crates/eval/src/report.rs`, `crates/cli/Cargo.toml`, `crates/cli/src/commands/mod.rs`, `crates/cli/src/commands/eval.rs`, `crates/cli/src/lib.rs`

1. Create `crates/eval/Cargo.toml`:
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
   tempfile = "3"
   ignore = "0.4"
   rayon = "1.10"
   sha2 = "0.10"
   ```

2. Create `crates/eval/src/lib.rs` with stub:
   ```rust
   pub mod metrics;
   pub mod dataset;
   pub mod runner;
   pub mod report;
   pub mod adapters;

   use domain::error::Result;
   use report::SuiteResult;

   /// Which evaluation suite to run.
   #[derive(Debug, Clone)]
   pub enum Suite {
       Search,
       Impact,
       All,
   }

   /// Configuration for an eval run.
   #[derive(Debug, Clone)]
   pub struct SuiteConfig {
       pub suite: Suite,
       pub no_cache: bool,
       pub suites_dir: std::path::PathBuf,
       pub search_limit: usize,  // default: 20
   }

   /// Run the evaluation suite. Entry point called by CLI.
   pub fn run_suite(config: &SuiteConfig) -> Result<SuiteResult> {
       Err(domain::error::CodeGraphError::Other(
           "eval: not yet implemented".into(),
       ))
   }
   ```

3. Create stub module files (`metrics.rs`, `dataset.rs`, `runner.rs`, `report.rs`, `adapters.rs`):
   ```rust
   // metrics.rs — stub
   // dataset.rs — stub
   // runner.rs — stub
   // adapters.rs — stub
   ```
   ```rust
   // report.rs — stub
   use serde::Serialize;

   #[derive(Debug, Clone, Serialize)]
   pub struct SuiteResult {
       pub search: Option<SearchSuiteResult>,
       pub impact: Option<ImpactSuiteResult>,
   }

   #[derive(Debug, Clone, Serialize)]
   pub struct SearchSuiteResult {
       pub repos: usize,
       pub queries: usize,
       pub mrr: f64,
       pub precision_at_5: f64,
       pub precision_at_10: f64,
       pub mrr_target: f64,
       pub mrr_passed: bool,
   }

   #[derive(Debug, Clone, Serialize)]
   pub struct ImpactSuiteResult {
       pub repos: usize,
       pub scenarios: usize,
       pub precision: f64,
       pub recall: f64,
       pub f1: f64,
       pub precision_target: f64,
       pub precision_passed: bool,
   }
   ```

4. Add `"crates/eval"` to root `Cargo.toml` workspace members:
   ```toml
   members = ["crates/domain", "crates/storage", "crates/parser", "crates/watch", "crates/cli", "crates/binary", "crates/eval"]
   ```

5. Add `eval` dependency to `crates/cli/Cargo.toml`:
   ```toml
   eval = { path = "../eval" }
   ```

6. Add `pub mod eval;` to `crates/cli/src/commands/mod.rs` module declarations.

7. Add `EvalArgs` struct in `crates/cli/src/commands/mod.rs`:
   ```rust
   #[derive(clap::Args)]
   pub struct EvalArgs {
       /// Which suite to run: search, impact, or all
       #[arg(long, default_value = "all")]
       pub suite: String,
       /// Force re-clone of eval repos (ignore cache)
       #[arg(long)]
       pub no_cache: bool,
   }
   ```

8. Change `Commands::Eval,` (unit variant) to `Eval(EvalArgs),` and update doc comment to `/// Run evaluation suite`.

9. Update `all_subcommands_parse` test:
   - Change `vec!["code-graph", "eval"]` to `vec!["code-graph", "eval"]` (still works — suite has default)
   - Add: `vec!["code-graph", "eval", "--suite", "search"]`
   - Add: `vec!["code-graph", "eval", "--no-cache"]`

10. Create `crates/cli/src/commands/eval.rs` with stub:
    ```rust
    use domain::error::Result;
    use crate::output::{OutputFormat, print, Displayable};
    use super::EvalArgs;

    pub fn run_eval(args: &EvalArgs, output_format: OutputFormat) -> Result<()> {
        Err(domain::error::CodeGraphError::Other(
            "eval: not yet implemented".into(),
        ))
    }
    ```

11. Wire dispatch in `crates/cli/src/lib.rs`:
    ```rust
    Commands::Eval(args) => commands::eval::run_eval(args, output_format),
    ```

12. `cargo test -p cli` — passes (all arg parsing tests including new eval variants)
13. `cargo check --workspace` — passes (eval crate compiles)

---

## Wave 1 — Core Modules (parallel)

### T02: Pure metric functions (metrics.rs)
**AC coverage:** AC3, AC5 (metric computation)
**Files:** `crates/eval/src/metrics.rs`
**Depends on:** T01

1. Write unit tests first:
   - `mrr_perfect_ranking`: all first results correct → MRR = 1.0
   - `mrr_second_position`: first correct at position 2 → MRR = 0.5
   - `mrr_no_match`: no correct result found → reciprocal rank = 0.0
   - `mrr_mixed`: varied positions → correct average
   - `mrr_empty_queries`: no queries → MRR = 0.0
   - `precision_at_k_all_relevant`: all top-k are relevant → 1.0
   - `precision_at_k_none_relevant`: no top-k relevant → 0.0
   - `precision_at_k_partial`: 3 of 5 relevant → 0.6
   - `precision_at_k_fewer_results_than_k`: 3 results, k=5 → use actual count
   - `blast_precision_perfect`: predicted == actual → 1.0
   - `blast_precision_empty_predicted`: no predictions → 0.0
   - `blast_recall_perfect`: all actual found → 1.0
   - `blast_recall_empty_actual`: no actual → 0.0 (handle zero division)
   - `f1_balanced`: precision = recall → F1 = precision
   - `f1_zero_both`: precision and recall both 0 → F1 = 0.0
   - `f1_typical`: precision=0.8, recall=0.6 → F1 = 0.6857...

2. Implement metric functions (all take `&[String]` or `&[Vec<String>]` for ranked results and ground truth):
   ```rust
   use std::collections::HashSet;

   /// Mean Reciprocal Rank across multiple queries.
   /// `ranked_results`: for each query, the ranked list of qualified names.
   /// `ground_truth`: for each query, the set of correct qualified names.
   pub fn mrr(ranked_results: &[Vec<String>], ground_truth: &[Vec<String>]) -> f64 {
       if ranked_results.is_empty() { return 0.0; }
       let sum: f64 = ranked_results.iter().zip(ground_truth.iter()).map(|(ranked, truth)| {
           let truth_set: HashSet<&str> = truth.iter().map(|s| s.as_str()).collect();
           ranked.iter().enumerate()
               .find(|(_, name)| truth_set.contains(name.as_str()))
               .map(|(i, _)| 1.0 / (i as f64 + 1.0))
               .unwrap_or(0.0)
       }).sum();
       sum / ranked_results.len() as f64
   }

   /// Precision at K — average across queries.
   pub fn precision_at_k(ranked_results: &[Vec<String>], ground_truth: &[Vec<String>], k: usize) -> f64 { ... }

   /// Blast radius precision: |predicted ∩ actual| / |predicted|
   pub fn blast_precision(predicted: &[String], actual: &[String]) -> f64 { ... }

   /// Blast radius recall: |predicted ∩ actual| / |actual|
   pub fn blast_recall(predicted: &[String], actual: &[String]) -> f64 { ... }

   /// Harmonic mean of precision and recall.
   pub fn f1(precision: f64, recall: f64) -> f64 {
       if precision + recall == 0.0 { return 0.0; }
       2.0 * precision * recall / (precision + recall)
   }
   ```

3. `cargo test -p eval -- metrics` — all 16 tests pass

### T03: Dataset management (dataset.rs)
**AC coverage:** AC9, AC11 (caching), AC10 (manifest parsing prerequisite)
**Files:** `crates/eval/src/dataset.rs`
**Depends on:** T01

1. Write unit tests first:
   - `parse_search_manifest`: valid JSON → `SuiteManifest` with repos
   - `parse_search_manifest_invalid_json`: broken JSON → clear error
   - `parse_search_queries`: valid JSON → `Vec<SearchQuery>` with expected fields
   - `parse_impact_queries`: valid JSON → `Vec<ImpactScenario>` with expected fields
   - `cache_dir_resolution`: returns `~/.cache/code-graph-eval/<name>/<revision>/`
   - `cache_dir_respects_xdg`: XDG_CACHE_HOME set → uses it
   - `validate_cache_missing_dir`: dir doesn't exist → false
   - `validate_cache_wrong_revision`: `.revision` marker mismatch → false
   - `validate_cache_valid`: dir exists + `.revision` matches → true
   - `clear_cache_removes_dir`: removes directory and contents

2. Implement types:
   ```rust
   use serde::Deserialize;
   use std::path::{Path, PathBuf};
   use domain::error::{CodeGraphError, Result};

   #[derive(Debug, Deserialize)]
   pub struct SuiteManifest {
       pub suite: SuiteInfo,
       pub repos: Vec<ManifestRepo>,
   }

   #[derive(Debug, Deserialize)]
   pub struct SuiteInfo {
       pub name: String,
       pub description: String,
   }

   #[derive(Debug, Deserialize)]
   pub struct ManifestRepo {
       pub name: String,
       pub url: String,
       pub revision: String,
       pub languages: Vec<String>,
   }

   #[derive(Debug, Deserialize)]
   pub struct SearchQueryFile {
       pub queries: Vec<SearchQuery>,
   }

   #[derive(Debug, Deserialize)]
   pub struct SearchQuery {
       pub repo: String,
       pub query: String,
       pub expected: Vec<String>,
   }

   #[derive(Debug, Deserialize)]
   pub struct ImpactQueryFile {
       pub scenarios: Vec<ImpactScenario>,
   }

   #[derive(Debug, Deserialize)]
   pub struct ImpactScenario {
       pub repo: String,
       pub description: String,
       pub target: String,
       pub depth: usize,
       pub confidence: String,
       pub expected_affected: Vec<String>,
   }
   ```

3. Implement cache management:
   ```rust
   pub fn eval_cache_dir() -> Result<PathBuf> {
       if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
           return Ok(PathBuf::from(xdg).join("code-graph-eval"));
       }
       let home = std::env::var("HOME")
           .map_err(|_| CodeGraphError::Other("HOME not set".into()))?;
       Ok(PathBuf::from(home).join(".cache").join("code-graph-eval"))
   }

   pub fn repo_cache_path(repo: &ManifestRepo) -> Result<PathBuf> {
       Ok(eval_cache_dir()?.join(&repo.name).join(&repo.revision))
   }

   pub fn validate_cache(repo: &ManifestRepo) -> Result<bool> { ... }

   pub fn clone_or_cache(repo: &ManifestRepo, no_cache: bool) -> Result<PathBuf> {
       let cache_path = repo_cache_path(repo)?;
       if no_cache {
           if cache_path.exists() { std::fs::remove_dir_all(&cache_path)?; }
       } else if validate_cache(repo)? {
           tracing::info!(repo = %repo.name, "Using cached clone");
           return Ok(cache_path);
       }
       tracing::info!(repo = %repo.name, revision = %repo.revision, "Cloning");
       std::fs::create_dir_all(&cache_path)?;
       let output = std::process::Command::new("git")
           .args(["clone", "--depth", "1", "--branch", &repo.revision, &repo.url])
           .arg(&cache_path)
           .output()
           .map_err(|e| CodeGraphError::Other(format!("git clone failed: {e}")))?;
       if !output.status.success() {
           let stderr = String::from_utf8_lossy(&output.stderr);
           return Err(CodeGraphError::Other(format!("git clone failed: {stderr}")));
       }
       std::fs::write(cache_path.join(".revision"), &repo.revision)?;
       Ok(cache_path)
   }

   pub fn clear_cache(repo: &ManifestRepo) -> Result<()> { ... }
   ```

4. Implement manifest/query parsing:
   ```rust
   pub fn parse_manifest(path: &Path) -> Result<SuiteManifest> {
       let content = std::fs::read_to_string(path)
           .map_err(|e| CodeGraphError::Other(format!("Failed to read manifest: {e}")))?;
       serde_json::from_str(&content)
           .map_err(|e| CodeGraphError::Other(format!("Invalid manifest JSON: {e}")))
   }

   pub fn parse_search_queries(path: &Path) -> Result<Vec<SearchQuery>> {
       let content = std::fs::read_to_string(path)?;
       let file: SearchQueryFile = serde_json::from_str(&content)?;
       Ok(file.queries)
   }

   pub fn parse_impact_queries(path: &Path) -> Result<Vec<ImpactScenario>> {
       let content = std::fs::read_to_string(path)?;
       let file: ImpactQueryFile = serde_json::from_str(&content)?;
       Ok(file.scenarios)
   }
   ```

5. `cargo test -p eval -- dataset` — all 10 tests pass

### T04: Report types + Displayable (report.rs)
**AC coverage:** AC8 (output formats), AC3, AC5 (metric reporting)
**Files:** `crates/eval/src/report.rs`
**Depends on:** T01

1. Write unit tests first:
   - `suite_result_compact_search_only`: search metrics → compact format matches SPEC section 8
   - `suite_result_compact_impact_only`: impact metrics → compact format
   - `suite_result_compact_all`: both suites → combined format
   - `suite_result_table_format`: tabular per-repo breakdown
   - `suite_result_json_format`: valid JSON with all fields
   - `quality_gate_all_pass`: metrics above targets → all_passed = true
   - `quality_gate_mrr_fail`: MRR below target → all_passed = false
   - `quality_gate_precision_fail`: blast precision below target → all_passed = false

2. Expand `SuiteResult` with quality gate:
   ```rust
   use serde::Serialize;
   use std::io::Write;

   #[derive(Debug, Clone, Serialize)]
   pub struct SuiteResult {
       pub search: Option<SearchSuiteResult>,
       pub impact: Option<ImpactSuiteResult>,
   }

   impl SuiteResult {
       /// Returns true if all quality targets are met.
       pub fn all_passed(&self) -> bool {
           let search_ok = self.search.as_ref().map_or(true, |s| s.mrr_passed);
           let impact_ok = self.impact.as_ref().map_or(true, |i| i.precision_passed);
           search_ok && impact_ok
       }
   }

   // Displayable impl for CLI output formatting:
   impl cli::output::Displayable for SuiteResult { ... }
   ```

   Note: `Displayable` is defined in cli crate. Eval crate cannot depend on cli (circular). Two options:
   - **Option A**: Implement `Displayable` in `crates/cli/src/commands/eval.rs` instead of in eval crate
   - **Option B**: Add formatting methods directly on `SuiteResult` (not trait-based)

   **Decision: Option A** — implement `Displayable for SuiteResult` in `crates/cli/src/commands/eval.rs`. Eval crate provides the data types and a `fmt_compact`/`fmt_table`/`fmt_json` helper module that writes to `&mut dyn Write`. CLI imports these helpers. This is consistent with how domain types get their `Displayable` impl in `cli/output.rs`.

   In `report.rs`:
   ```rust
   impl SuiteResult {
       pub fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
           if let Some(search) = &self.search {
               let status = if search.mrr_passed { "PASS" } else { "FAIL" };
               writeln!(w, "Search Suite — {} repos, {} queries", search.repos, search.queries)?;
               writeln!(w, "  MRR:          {:.2} (target: >{:.2}) {}", search.mrr, search.mrr_target, status)?;
               writeln!(w, "  Precision@5:  {:.2}", search.precision_at_5)?;
               writeln!(w, "  Precision@10: {:.2}", search.precision_at_10)?;
           }
           if let Some(impact) = &self.impact {
               let status = if impact.precision_passed { "PASS" } else { "FAIL" };
               if self.search.is_some() { writeln!(w)?; }
               writeln!(w, "Impact Suite — {} repos, {} scenarios", impact.repos, impact.scenarios)?;
               writeln!(w, "  Precision:    {:.2} (target: >{:.2}) {}", impact.precision, impact.precision_target, status)?;
               writeln!(w, "  Recall:       {:.2}", impact.recall)?;
               writeln!(w, "  F1:           {:.2}", impact.f1)?;
           }
           Ok(())
       }

       pub fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> { ... }

       pub fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
           let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
           writeln!(w, "{json}")
       }
   }
   ```

3. `cargo test -p eval -- report` — all 8 tests pass

---

## Wave 2 — Runner + Datasets (parallel)

### T05: Adapters + Runner orchestration (adapters.rs, runner.rs)
**AC coverage:** AC2, AC4 (run queries), AC10 (ground truth validation), AC6, AC7 (quality targets)
**Files:** `crates/eval/src/adapters.rs` (full implementation), `crates/eval/src/runner.rs`, `crates/eval/src/lib.rs`
**Depends on:** T02, T03, T04
**Scope note:** This task includes implementing the full adapters.rs (copy EvalFileSystem from cli/adapters/fs.rs:8-43, copy EvalParseProvider from cli/adapters/parse.rs:36-107). Total ~100 lines of adapter code + ~150 lines of runner orchestration.

1. Write unit tests first:
   - `validate_ground_truth_all_found`: all expected QNames exist → Ok(())
   - `validate_ground_truth_missing_qname`: expected QName absent → setup error
   - `confidence_from_string_high`: "high" → `Confidence::High`
   - `confidence_from_string_invalid`: "unknown" → error
   - `compute_search_metrics`: given ranked results + truth → correct SuiteResult
   - `compute_impact_metrics`: given predicted + actual → correct precision/recall/F1

   Note: Full integration tests (actual clone + index + query) deferred to T12. Unit tests use pre-built results.

2. Implement runner:
   ```rust
   use std::path::Path;
   use domain::error::{CodeGraphError, Result};
   use domain::model::{Confidence, ImpactTarget};
   use domain::ports::GraphStore;
   use domain::use_cases::index::IndexUseCase;
   use domain::use_cases::query::QueryUseCase;
   use domain::use_cases::impact::ImpactUseCase;
   use storage::SqliteStore;
   use crate::adapters::{EvalFileSystem, EvalParseProvider, NoOpGitProvider};
   use crate::dataset::{ManifestRepo, SearchQuery, ImpactScenario};
   use crate::metrics;
   use crate::report::{SuiteResult, SearchSuiteResult, ImpactSuiteResult};

   const MRR_TARGET: f64 = 0.30;
   const BLAST_PRECISION_TARGET: f64 = 0.40;

   pub fn confidence_from_str(s: &str) -> Result<Confidence> {
       match s.to_lowercase().as_str() {
           "high" => Ok(Confidence::High),
           "medium" => Ok(Confidence::Medium),
           "low" => Ok(Confidence::Low),
           "structural" => Ok(Confidence::Structural),
           _ => Err(CodeGraphError::Other(format!("Unknown confidence: {s}"))),
       }
   }

   /// Validate that all expected qualified names in queries exist in the indexed graph.
   pub fn validate_ground_truth(
       store: &impl GraphStore,
       expected_qnames: &[String],
       repo_name: &str,
   ) -> Result<()> {
       let mut errors = Vec::new();
       for qname in expected_qnames {
           if store.get_symbol(qname)?.is_none() {
               errors.push(format!(
                   "SETUP_ERROR: '{}' not found in indexed graph for repo '{}'",
                   qname, repo_name
               ));
           }
       }
       if !errors.is_empty() {
           return Err(CodeGraphError::Other(errors.join("\n")));
       }
       Ok(())
   }

   /// Index a cloned repo into an isolated temp database.
   /// Uses eval-local adapters (not CLI adapters) to avoid circular dependency.
   pub fn index_repo(clone_path: &Path) -> Result<(SqliteStore, tempfile::TempDir)> {
       let temp_dir = tempfile::tempdir()
           .map_err(|e| CodeGraphError::Other(format!("tempdir: {e}")))?;
       let db_path = temp_dir.path().join("eval.db");
       let store = SqliteStore::open(&db_path)?;
       let fs = EvalFileSystem;
       let parser = EvalParseProvider::new();
       let git = NoOpGitProvider;
       let use_case = IndexUseCase::new(store.clone(), parser, fs, git);
       use_case.full_index(clone_path)?;
       Ok((store, temp_dir))
   }
   ```

   **Adapters module** (`crates/eval/src/adapters.rs`) — eval-owned implementations to avoid circular dependency with cli:
   ```rust
   use std::path::{Path, PathBuf};
   use domain::error::{CodeGraphError, Result};
   use domain::ports::{FileSystem, ParseProvider, GitProvider, FileData};
   use domain::model::{DiffHunk, Edge, FileNode};
   use parser::{ParserRegistry, ParseResult};
   use parser::resolver::{ResolveContext, ResolverRegistry};
   use sha2::{Sha256, Digest};
   use rayon::prelude::*;
   use std::collections::HashMap;

   /// Minimal FileSystem for eval — mirrors cli's RealFileSystem including .code-graphignore.
   pub struct EvalFileSystem;

   impl FileSystem for EvalFileSystem {
       fn list_files(&self, root: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
           let mut builder = ignore::WalkBuilder::new(root);
           builder.add_custom_ignore_filename(".code-graphignore");
           let files: Vec<PathBuf> = builder.build()
               .filter_map(|e| e.ok())
               .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
               .filter(|e| {
                   e.path().extension().and_then(|ext| ext.to_str())
                       .is_some_and(|ext| extensions.contains(&ext))
               })
               .map(|e| e.path().strip_prefix(root).unwrap_or(e.path()).to_path_buf())
               .collect();
           Ok(files)
       }

       fn read_file(&self, path: &Path) -> Result<String> {
           std::fs::read_to_string(path)
               .map_err(|e| CodeGraphError::FileSystem { path: path.into(), source: e })
       }

       fn file_hash(&self, path: &Path) -> Result<String> {
           let content = std::fs::read(path)
               .map_err(|e| CodeGraphError::FileSystem { path: path.into(), source: e })?;
           let mut hasher = Sha256::new();
           hasher.update(&content);
           Ok(format!("{:x}", hasher.finalize()))
       }
   }

   /// Parallel parse provider — mirrors cli's RayonParseProvider exactly.
   /// Copy the full 3-phase pipeline from `crates/cli/src/adapters/parse.rs:36-107`.
   pub struct EvalParseProvider { registry: ParserRegistry }

   impl EvalParseProvider {
       pub fn new() -> Self { Self { registry: ParserRegistry::new() } }
       fn compute_hash(content: &[u8]) -> String {
           let mut hasher = Sha256::new();
           hasher.update(content);
           format!("{:x}", hasher.finalize())
       }
   }

   impl ParseProvider for EvalParseProvider {
       fn parse_and_resolve(&self, files: &[(PathBuf, Vec<u8>)], project_root: &Path) -> Result<Vec<FileData>> {
           // EXACT copy of cli::adapters::parse::RayonParseProvider::parse_and_resolve
           // Source: crates/cli/src/adapters/parse.rs lines 36-107
           // Phase 1: parallel parse via rayon + registry
           // Phase 2: build ResolveContext (HashMap of parsed files + file tree)
           // Phase 3: parallel resolve imports via ResolverRegistry
           // Merge structural edges + resolved edges per file
           // Return Vec<FileData>
           //
           // The full implementation (~70 lines) is a direct copy.
           // Reference: crates/cli/src/adapters/parse.rs
       }
   }

   /// No-op git provider — eval indexes full clones, no incremental.
   pub struct NoOpGitProvider;

   impl GitProvider for NoOpGitProvider {
       fn current_head(&self) -> Result<String> { Ok("eval".into()) }
       fn changed_files(&self, _from: &str, _to: &str) -> Result<Vec<PathBuf>> { Ok(vec![]) }
       fn diff_hunks(&self, _from: &str, _to: Option<&str>) -> Result<Vec<DiffHunk>> { Ok(vec![]) }
       fn modified_files(&self) -> Result<Vec<PathBuf>> { Ok(vec![]) }
   }
   ```

   Note: `EvalFileSystem` and `EvalParseProvider` mirror the cli adapters. This duplication is intentional — eval must not depend on cli (which depends on eval). Critical: `EvalFileSystem` includes `.code-graphignore` support for behavior parity with the production indexing path. `EvalParseProvider::parse_and_resolve` is a direct copy of `cli/adapters/parse.rs:36-107` — reference that file during implementation.

3. Implement search + impact query execution:
   ```rust
   /// Run search queries against an indexed repo.
   pub fn run_search_queries(
       store: &SqliteStore,
       queries: &[SearchQuery],
       limit: usize,
   ) -> Result<(Vec<Vec<String>>, Vec<Vec<String>>)> {
       let query_uc = QueryUseCase::new(store.clone(), store.clone());
       let mut all_ranked = Vec::new();
       let mut all_truth = Vec::new();
       for q in queries {
           let results = query_uc.search(&q.query, limit)?;
           let ranked: Vec<String> = results.iter().map(|r| r.qualified_name.clone()).collect();
           all_ranked.push(ranked);
           all_truth.push(q.expected.clone());
       }
       Ok((all_ranked, all_truth))
   }

   /// Run impact scenarios against an indexed repo.
   /// Returns per-scenario (predicted, actual) pairs for metric computation.
   pub fn run_impact_scenarios(
       store: &SqliteStore,
       scenarios: &[ImpactScenario],
   ) -> Result<(Vec<Vec<String>>, Vec<Vec<String>>)> {
       let impact_uc = ImpactUseCase::new(store.clone());
       let mut all_predicted = Vec::new();
       let mut all_actual = Vec::new();
       for s in scenarios {
           let target = ImpactTarget::Symbol(s.target.clone());
           let confidence = confidence_from_str(&s.confidence)?;
           let report = impact_uc.blast_radius(&[target], s.depth, confidence)?;
           let predicted: Vec<String> = report.affected.iter().map(|a| a.qualified_name.clone()).collect();
           all_predicted.push(predicted);
           all_actual.push(s.expected_affected.clone());
       }
       Ok((all_predicted, all_actual))
   }

   /// Aggregate blast metrics across scenarios (per-scenario precision/recall, then average).
   pub fn aggregate_impact_metrics(
       all_predicted: &[Vec<String>],
       all_actual: &[Vec<String>],
   ) -> (f64, f64, f64) {
       if all_predicted.is_empty() { return (0.0, 0.0, 0.0); }
       let (total_p, total_r) = all_predicted.iter().zip(all_actual.iter())
           .map(|(pred, actual)| {
               (metrics::blast_precision(pred, actual), metrics::blast_recall(pred, actual))
           })
           .fold((0.0, 0.0), |(sp, sr), (p, r)| (sp + p, sr + r));
       let n = all_predicted.len() as f64;
       let avg_p = total_p / n;
       let avg_r = total_r / n;
       (avg_p, avg_r, metrics::f1(avg_p, avg_r))
   }
   ```

4. Wire `run_suite()` in `lib.rs`:
   ```rust
   pub fn run_suite(config: &SuiteConfig) -> Result<SuiteResult> {
       let search_result = match config.suite {
           Suite::Search | Suite::All => Some(runner::run_search_suite(config)?),
           _ => None,
       };
       let impact_result = match config.suite {
           Suite::Impact | Suite::All => Some(runner::run_impact_suite(config)?),
           _ => None,
       };
       Ok(SuiteResult { search: search_result, impact: impact_result })
   }
   ```

   `run_search_suite` and `run_impact_suite` in runner.rs:
   - Parse manifest
   - For each repo: clone_or_cache → index_repo → validate ground truth → run queries
   - Aggregate metrics across repos using `metrics::mrr()`, `metrics::precision_at_k()`
   - Build `SearchSuiteResult` / `ImpactSuiteResult`

5. `cargo test -p eval -- runner` — all 6 unit tests pass

### T06: Search eval dataset files
**AC coverage:** AC2 (50+ queries), AC6 (curated for MRR > 0.30)
**Files:** `eval/suites/search/manifest.json`, `eval/suites/search/queries/*.json` (5 files)
**Depends on:** T01 (types must compile)

1. Create `eval/suites/search/manifest.json`:
   ```json
   {
     "suite": {
       "name": "search",
       "description": "Search quality evaluation — MRR and precision@k"
     },
     "repos": [
       { "name": "express", "url": "https://github.com/expressjs/express", "revision": "v4.21.2", "languages": ["javascript"] },
       { "name": "trpc", "url": "https://github.com/trpc/trpc", "revision": "v11.0.0", "languages": ["typescript"] },
       { "name": "ripgrep", "url": "https://github.com/BurntSushi/ripgrep", "revision": "14.1.1", "languages": ["rust"] },
       { "name": "fastapi", "url": "https://github.com/fastapi/fastapi", "revision": "0.115.0", "languages": ["python"] },
       { "name": "go-stdlib", "url": "https://github.com/golang/go", "revision": "go1.23.0", "languages": ["go"] }
     ]
   }
   ```

2. **Ground truth generation process** (MUST do before writing queries):
   - For each repo: `git clone --depth 1 --branch <revision> <url> /tmp/eval-<name>`
   - Run `cargo run -- index --path /tmp/eval-<name>` to index it
   - Run `cargo run -- search <query>` to discover actual qualified names
   - Run `cargo run -- find <pattern>` to verify symbol existence
   - Record exact qualified names as ground truth in the JSON files
   - For Go stdlib: index the full clone but only write queries targeting `src/net/http/` symbols (the Go repo is large — `--depth 1` clone is ~500MB, but indexing only takes a few minutes)

3. Create 5 query files with 10+ queries each. Queries must use exact qualified name format (`file_path::symbol_name`) verified in step 2. Curate for FTS5 strengths (exact name matches, common symbol names).

   Example `eval/suites/search/queries/javascript.json`:
   ```json
   {
     "queries": [
       { "repo": "express", "query": "Router", "expected": ["lib/router/index.js::Router"] },
       { "repo": "express", "query": "createApplication", "expected": ["lib/express.js::createApplication"] },
       ...
     ]
   }
   ```

   Repeat for typescript.json (tRPC), rust.json (ripgrep), python.json (FastAPI), go.json (Go stdlib net/http subset).

4. Validate all JSON files parse correctly: `python3 -c "import json, glob; [json.load(open(f)) for f in glob.glob('eval/suites/search/queries/*.json')]"`

### T07: Impact eval dataset files
**AC coverage:** AC4 (20+ scenarios), AC7 (curated for precision > 0.40)
**Files:** `eval/suites/impact/manifest.json`, `eval/suites/impact/queries/*.json` (5 files)
**Depends on:** T01 (types must compile)

1. Create `eval/suites/impact/manifest.json` (same repos as search manifest).

2. **Ground truth generation process** (same repos indexed in T06 step 2):
   - For each target symbol: run `cargo run -- impact <target> --depth 3 --confidence high`
   - Record actual affected symbols as ground truth `expected_affected`
   - Verify symbols exist with `cargo run -- find <name>`

3. Create 5 scenario files with 4+ scenarios each. Each scenario specifies a target symbol, depth, confidence level, and expected affected symbols verified in step 2.

   Example `eval/suites/impact/queries/javascript.json`:
   ```json
   {
     "scenarios": [
       {
         "repo": "express",
         "description": "Changing Router.route affects downstream handlers",
         "target": "lib/router/index.js::route",
         "depth": 3,
         "confidence": "high",
         "expected_affected": [
           "lib/router/index.js::Router",
           "lib/application.js::lazyrouter"
         ]
       },
       ...
     ]
   }
   ```

4. Validate all JSON files parse correctly.

---

## Wave 3 — CLI Command + CI/CD (parallel)

### T08: CLI eval command handler
**AC coverage:** AC8 (output formats), AC2, AC4 (end-to-end wiring)
**Files:** `crates/cli/src/commands/eval.rs`
**Depends on:** T05

1. Write unit tests first:
   - `suite_from_string_search`: "search" → `Suite::Search`
   - `suite_from_string_impact`: "impact" → `Suite::Impact`
   - `suite_from_string_all`: "all" → `Suite::All`
   - `suite_from_string_invalid`: "unknown" → error
   - `displayable_compact_output`: SuiteResult → compact format matches SPEC section 8
   - `displayable_json_output`: SuiteResult → valid JSON

2. Implement `run_eval`:
   ```rust
   use domain::error::{CodeGraphError, Result};
   use eval::{Suite, SuiteConfig};
   use eval::report::SuiteResult;
   use crate::output::{OutputFormat, Displayable};
   use super::EvalArgs;
   use std::io::Write;

   fn suite_from_str(s: &str) -> Result<Suite> {
       match s {
           "search" => Ok(Suite::Search),
           "impact" => Ok(Suite::Impact),
           "all" => Ok(Suite::All),
           _ => Err(CodeGraphError::Other(format!(
               "Unknown suite '{}'. Valid: search, impact, all", s
           ))),
       }
   }

   impl Displayable for SuiteResult {
       fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> { self.fmt_compact(w) }
       fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> { self.fmt_table(w) }
       fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> { self.fmt_json(w) }
   }

   pub fn run_eval(args: &EvalArgs, output_format: OutputFormat) -> Result<()> {
       let suite = suite_from_str(&args.suite)?;

       // Locate suites directory: check ./eval/suites/ first, then alongside binary
       let suites_dir = find_suites_dir()?;

       let config = SuiteConfig {
           suite,
           no_cache: args.no_cache,
           suites_dir,
       };

       let result = eval::run_suite(&config)?;
       crate::output::print(&result, output_format);

       if !result.all_passed() {
           return Err(CodeGraphError::Other(
               "Quality targets not met — see results above".into(),
           ));
       }
       Ok(())
   }

   fn find_suites_dir() -> Result<std::path::PathBuf> {
       // Check relative to CWD
       let cwd_suites = std::path::PathBuf::from("eval/suites");
       if cwd_suites.is_dir() {
           return Ok(cwd_suites);
       }
       // Check relative to binary
       if let Ok(exe) = std::env::current_exe() {
           let exe_suites = exe.parent().unwrap_or(exe.as_ref()).join("eval/suites");
           if exe_suites.is_dir() {
               return Ok(exe_suites);
           }
       }
       Err(CodeGraphError::Other(
           "eval/suites/ directory not found — run from project root".into(),
       ))
   }
   ```

3. `cargo test -p cli -- eval` — all 6 tests pass

### T09: Lefthook configuration
**AC coverage:** AC12, AC13
**Files:** `lefthook.yml`
**Depends on:** none

1. Create `lefthook.yml` at project root:
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

2. Verify YAML is valid: `python3 -c "import yaml; yaml.safe_load(open('lefthook.yml'))"`

### T10: GitHub Actions CI workflow
**AC coverage:** AC14, AC15
**Files:** `.github/workflows/ci.yml`
**Depends on:** none

1. Create `.github/workflows/ci.yml`:
   - Trigger: `pull_request` to `main` and `milestone/*` branches
   - Jobs: fmt, clippy, test (matrix: ubuntu-latest + macos-latest), coverage (80% gate), audit, bench
   - Caching: `actions/cache@v4` keyed by `Cargo.lock`
   - Coverage: `taiki-e/install-action@cargo-llvm-cov` + `--fail-under-lines 80`

2. Full workflow structure per SPEC sections 10 and 12.

### T11: GitHub Actions Release workflow
**AC coverage:** AC16, AC17, AC18
**Files:** `.github/workflows/release.yml`
**Depends on:** none

1. Create `.github/workflows/release.yml`:
   - Trigger: tag push `v*`
   - **Stage 1 — Eval Gate:** build release binary, run `code-graph eval --suite all`, assert exit 0
   - **Stage 2 — Build Matrix:** 4 targets:
     - `x86_64-unknown-linux-gnu` (ubuntu, native)
     - `aarch64-unknown-linux-gnu` (ubuntu, cross-rs)
     - `x86_64-apple-darwin` (macos, native)
     - `aarch64-apple-darwin` (macos, native)
   - **Stage 3 — Publish:** `softprops/action-gh-release@v2` with 4 binaries + `cargo publish` in topological order

2. Full workflow structure per SPEC sections 11 and 12.

---

## Wave 4 — Integration + Quality

### T12: Integration tests + clippy + final validation
**AC coverage:** AC1, AC16, AC17
**Files:** `crates/eval/src/lib.rs` (final wiring), `crates/cli/src/commands/eval.rs` (integration tests)
**Depends on:** T05, T06, T07, T08

1. Write integration tests (in eval crate `tests/` or as `#[cfg(test)]` in lib.rs):
   - `eval_crate_is_workspace_member`: verify 7 members in workspace
   - `suite_config_search_parses`: SuiteConfig with search suite
   - `suite_config_impact_parses`: SuiteConfig with impact suite
   - `manifest_files_are_valid_json`: parse all manifest.json files
   - `query_files_are_valid_json`: parse all query/*.json files
   - `query_count_meets_minimum`: search >= 50 queries, impact >= 20 scenarios

2. `cargo test --workspace` passes (AC16 equivalent at local level)
3. `cargo clippy --workspace -- -Dwarnings` passes (AC17 equivalent)

---

## Task Dependency Graph

```
T01 (crate skeleton + adapters) ┬──► T02 (metrics)
                                │
                                ├──► T03 (dataset)
                                │
                                ├──► T04 (report)
                                │
                                ├──► T06 (search dataset)
                                │
                                └──► T07 (impact dataset)

T02 + T03 + T04 ───────────────────► T05 (runner — uses eval adapters, NOT cli)

T05 ────────────────────────────────► T08 (CLI eval command)

(independent) ──────────────────┬──► T09 (lefthook)
                                ├──► T10 (CI workflow)
                                └──► T11 (release workflow)

T05 + T06 + T07 + T08 ─────────────► T12 (integration + quality)
```

## Wave Summary

| Wave | Tasks | Parallelism |
|------|-------|-------------|
| **0** | T01 | Sequential (structural prerequisite) |
| **1** | T02, T03, T04 | Parallel (metrics, dataset, report — no cross-deps) |
| **2** | T05, T06, T07 | Parallel (runner depends on W1; datasets are JSON-only) |
| **3** | T08, T09, T10, T11 | Parallel (CLI depends on T05; CI/CD fully independent) |
| **4** | T12 | Sequential (integration — depends on all) |

## Complexity Estimate

| Task | Size | Notes |
|------|------|-------|
| T01 | S | Cargo.toml + stubs + wiring, ~80 lines new |
| T02 | M | 5 metric functions + 16 tests, ~150 lines |
| T03 | M-L | Types + manifest parsing + clone/cache + 10 tests, ~200 lines |
| T04 | M | SuiteResult types + 3 format methods + 8 tests, ~150 lines |
| T05 | L | Adapters (copy from cli ~100 lines) + runner orchestration + ground truth validation + 6 tests, ~350 lines |
| T06 | M | 6 JSON files (manifest + 5 query files), ~300 lines JSON |
| T07 | M | 6 JSON files (manifest + 5 scenario files), ~200 lines JSON |
| T08 | M | CLI handler + Displayable impl + 6 tests, ~100 lines |
| T09 | S | Single YAML file, ~20 lines |
| T10 | M | GitHub Actions workflow, ~100 lines YAML |
| T11 | M-L | GitHub Actions workflow (3 stages, matrix build), ~150 lines YAML |
| T12 | M | Integration tests + validation, ~80 lines |

**Total estimated:** ~1,800 lines of new code/config/data across 26 files

## AC Traceability Matrix

| AC | Task(s) | Verified By |
|----|---------|-------------|
| AC1 | T01 | T12: workspace has 7 members |
| AC2 | T05, T06 | T12: search suite runs with 50+ queries |
| AC3 | T02, T04 | T02: metric tests; T04: format tests |
| AC4 | T05, T07 | T12: impact suite runs with 20+ scenarios |
| AC5 | T02, T04 | T02: metric tests; T04: format tests |
| AC6 | T06 | Eval run: MRR > 0.30 on curated dataset |
| AC7 | T07 | Eval run: blast precision > 0.40 on curated dataset |
| AC8 | T04, T08 | T04: format tests; T08: Displayable impl |
| AC9 | T03 | T03: `--no-cache` removes cache before clone |
| AC10 | T05 | T05: ground truth validation tests |
| AC11 | T03 | T03: cache validation tests |
| AC12 | T09 | Lefthook YAML: pre-commit commands |
| AC13 | T09 | Lefthook YAML: pre-push commands |
| AC14 | T10 | CI workflow: all 6 jobs defined |
| AC15 | T10 | CI workflow: `--fail-under-lines 80` |
| AC16 | T11 | Release workflow: 4-target build matrix |
| AC17 | T11 | Release workflow: eval gate stage |
| AC18 | T11 | Release workflow: cargo publish step |
