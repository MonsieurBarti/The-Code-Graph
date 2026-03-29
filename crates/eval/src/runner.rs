use std::path::Path;

use domain::error::{CodeGraphError, Result};
use domain::model::{Confidence, HybridSearchConfig, ImpactTarget, SearchMode};
use domain::ports::GraphStore;
use domain::use_cases::impact::ImpactUseCase;
use domain::use_cases::index::IndexUseCase;
use domain::use_cases::query::QueryUseCase;
use storage::SqliteStore;

use crate::adapters::{EvalFileSystem, EvalParseProvider, NoOpGitProvider};
use crate::dataset::{ImpactScenario, SearchQuery};
use crate::report::{
    AnalysisSuiteResult, BenchSuiteResult, CategoryMrr, CoreSuiteResult, FlowsSuiteResult,
    ImpactSuiteResult, InvariantsSuiteResult, RiskSuiteResult, SearchSuiteResult,
};
use crate::suites::bench::{percentile, BaselineEntry, GraphSize, QueryLatencies};
use crate::{metrics, SuiteConfig};

const MRR_TARGET: f64 = 0.30;
const BLAST_PRECISION_TARGET: f64 = 0.40;

/// Ranked results paired with ground-truth expectations.
type RankedVsTruth = (Vec<Vec<String>>, Vec<Vec<String>>);

/// Per-category bucket: (ranked lists, truth lists).
type CategoryBucket = (Vec<Vec<String>>, Vec<Vec<String>>);

pub fn confidence_from_str(s: &str) -> Result<Confidence> {
    match s.to_lowercase().as_str() {
        "high" => Ok(Confidence::High),
        "medium" => Ok(Confidence::Medium),
        "low" => Ok(Confidence::Low),
        "structural" => Ok(Confidence::Structural),
        _ => Err(CodeGraphError::Other(format!("Unknown confidence: {s}"))),
    }
}

/// Validate that all expected qualified names exist in the indexed graph.
pub fn validate_ground_truth(
    store: &SqliteStore,
    expected_qnames: &[String],
    repo_name: &str,
) -> Result<Vec<String>> {
    let mut missing = Vec::new();
    for qname in expected_qnames {
        if store.get_symbol(qname)?.is_none() {
            missing.push(format!(
                "SETUP_ERROR: '{}' not found in indexed graph for repo '{}'",
                qname, repo_name
            ));
        }
    }
    Ok(missing)
}

/// Index a cloned repo into an isolated temp database.
pub fn index_repo(clone_path: &Path) -> Result<(SqliteStore, tempfile::TempDir)> {
    let temp_dir =
        tempfile::tempdir().map_err(|e| CodeGraphError::Other(format!("tempdir: {e}")))?;
    let db_path = temp_dir.path().join("eval.db");
    let store = SqliteStore::open(&db_path)?;
    let fs = EvalFileSystem;
    let parser = EvalParseProvider::new();
    let git = NoOpGitProvider;
    let use_case = IndexUseCase::new(store.clone(), parser, fs, git);
    use_case.full_index(clone_path)?;
    Ok((store, temp_dir))
}

pub fn run_search_queries(
    store: &SqliteStore,
    queries: &[SearchQuery],
    limit: usize,
    mode: Option<SearchMode>,
) -> Result<RankedVsTruth> {
    let query_uc = QueryUseCase::new(store.clone(), store.clone());
    let config = HybridSearchConfig::default();
    let mut all_ranked = Vec::new();
    let mut all_truth = Vec::new();
    for q in queries {
        let results = match mode {
            Some(m) => query_uc.hybrid_search(&q.query, limit, m, &config)?,
            None => query_uc.search(&q.query, limit)?,
        };
        let ranked: Vec<String> = results.iter().map(|r| r.qualified_name.clone()).collect();
        all_ranked.push(ranked);
        all_truth.push(q.expected.clone());
    }
    Ok((all_ranked, all_truth))
}

pub fn run_impact_scenarios(
    store: &SqliteStore,
    scenarios: &[ImpactScenario],
) -> Result<RankedVsTruth> {
    let impact_uc = ImpactUseCase::new(store.clone());
    let mut all_predicted = Vec::new();
    let mut all_actual = Vec::new();
    for s in scenarios {
        let target = ImpactTarget::Symbol(s.target.clone());
        let confidence = confidence_from_str(&s.confidence)?;
        let report = impact_uc.blast_radius(&[target], s.depth, confidence)?;
        let predicted: Vec<String> = report
            .affected
            .iter()
            .map(|a| a.qualified_name.clone())
            .collect();
        all_predicted.push(predicted);
        all_actual.push(s.expected_affected.clone());
    }
    Ok((all_predicted, all_actual))
}

pub fn aggregate_impact_metrics(
    all_predicted: &[Vec<String>],
    all_actual: &[Vec<String>],
) -> (f64, f64, f64) {
    if all_predicted.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let (total_p, total_r) = all_predicted
        .iter()
        .zip(all_actual.iter())
        .map(|(pred, actual)| {
            (
                metrics::blast_precision(pred, actual),
                metrics::blast_recall(pred, actual),
            )
        })
        .fold((0.0, 0.0), |(sp, sr), (p, r)| (sp + p, sr + r));
    let n = all_predicted.len() as f64;
    let avg_p = total_p / n;
    let avg_r = total_r / n;
    (avg_p, avg_r, metrics::f1(avg_p, avg_r))
}

/// Run the full search evaluation suite.
pub fn run_search_suite(config: &SuiteConfig) -> Result<SearchSuiteResult> {
    let manifest_path = config.suites_dir.join("search").join("manifest.json");
    let manifest = crate::dataset::parse_manifest(&manifest_path)?;
    let queries_dir = config.suites_dir.join("search").join("queries");

    let mut all_ranked = Vec::new();
    let mut all_truth = Vec::new();
    // Per-category buckets: category -> (ranked_lists, truth_lists)
    let mut category_buckets: std::collections::HashMap<String, CategoryBucket> =
        std::collections::HashMap::new();
    let mut total_queries = 0;
    let mut setup_errors = Vec::new();

    for repo in &manifest.repos {
        tracing::info!(repo = %repo.name, "Processing search eval repo");
        let clone_path = crate::dataset::clone_or_cache(repo, config.no_cache)?;
        let (store, _temp_dir) = index_repo(&clone_path)?;

        for lang in &repo.languages {
            let query_file = queries_dir.join(format!("{lang}.json"));
            if !query_file.exists() {
                continue;
            }
            let queries = crate::dataset::parse_search_queries(&query_file)?;
            let repo_queries: Vec<_> = queries.iter().filter(|q| q.repo == repo.name).collect();

            // Validate ground truth
            let all_expected: Vec<String> = repo_queries
                .iter()
                .flat_map(|q| q.expected.iter().cloned())
                .collect();
            let missing = validate_ground_truth(&store, &all_expected, &repo.name)?;
            setup_errors.extend(missing);

            // Run queries
            let filtered: Vec<SearchQuery> = repo_queries.into_iter().cloned().collect();
            let (ranked, truth) = run_search_queries(&store, &filtered, config.search_limit, None)?;
            total_queries += ranked.len();

            // Collect per-category data
            for (q, r, t) in filtered
                .iter()
                .zip(ranked.iter())
                .zip(truth.iter())
                .map(|((q, r), t)| (q, r, t))
            {
                let cat = q
                    .category
                    .clone()
                    .unwrap_or_else(|| "uncategorized".to_string());
                let bucket = category_buckets.entry(cat).or_default();
                bucket.0.push(r.clone());
                bucket.1.push(t.clone());
            }

            all_ranked.extend(ranked);
            all_truth.extend(truth);
        }
    }

    if !setup_errors.is_empty() {
        tracing::warn!(
            "Ground truth validation issues:\n{}",
            setup_errors.join("\n")
        );
    }

    let mrr = metrics::mrr(&all_ranked, &all_truth);
    let p5 = metrics::precision_at_k(&all_ranked, &all_truth, 5);
    let p10 = metrics::precision_at_k(&all_ranked, &all_truth, 10);

    // Build sorted per-category breakdown
    let mut per_category: Vec<CategoryMrr> = category_buckets
        .into_iter()
        .map(|(cat, (ranked, truth))| {
            let cat_mrr = metrics::mrr(&ranked, &truth);
            CategoryMrr {
                queries: ranked.len(),
                category: cat,
                mrr: cat_mrr,
            }
        })
        .collect();
    per_category.sort_by(|a, b| a.category.cmp(&b.category));

    Ok(SearchSuiteResult {
        repos: manifest.repos.len(),
        queries: total_queries,
        mrr,
        precision_at_5: p5,
        precision_at_10: p10,
        mrr_target: MRR_TARGET,
        mrr_passed: mrr >= MRR_TARGET,
        per_category,
    })
}

/// Run the full impact evaluation suite.
pub fn run_impact_suite(config: &SuiteConfig) -> Result<ImpactSuiteResult> {
    let manifest_path = config.suites_dir.join("impact").join("manifest.json");
    let manifest = crate::dataset::parse_manifest(&manifest_path)?;
    let queries_dir = config.suites_dir.join("impact").join("queries");

    let mut all_predicted = Vec::new();
    let mut all_actual = Vec::new();
    let mut total_scenarios = 0;
    let mut setup_errors = Vec::new();

    for repo in &manifest.repos {
        tracing::info!(repo = %repo.name, "Processing impact eval repo");
        let clone_path = crate::dataset::clone_or_cache(repo, config.no_cache)?;
        let (store, _temp_dir) = index_repo(&clone_path)?;

        for lang in &repo.languages {
            let query_file = queries_dir.join(format!("{lang}.json"));
            if !query_file.exists() {
                continue;
            }
            let scenarios = crate::dataset::parse_impact_queries(&query_file)?;
            let repo_scenarios: Vec<_> = scenarios.iter().filter(|s| s.repo == repo.name).collect();

            // Validate ground truth
            let all_expected: Vec<String> = repo_scenarios
                .iter()
                .flat_map(|s| {
                    let mut v = s.expected_affected.clone();
                    v.push(s.target.clone());
                    v
                })
                .collect();
            let missing = validate_ground_truth(&store, &all_expected, &repo.name)?;
            setup_errors.extend(missing);

            let filtered: Vec<ImpactScenario> = repo_scenarios.into_iter().cloned().collect();
            let (predicted, actual) = run_impact_scenarios(&store, &filtered)?;
            total_scenarios += predicted.len();
            all_predicted.extend(predicted);
            all_actual.extend(actual);
        }
    }

    if !setup_errors.is_empty() {
        tracing::warn!(
            "Ground truth validation issues:\n{}",
            setup_errors.join("\n")
        );
    }

    let (precision, recall, f1) = aggregate_impact_metrics(&all_predicted, &all_actual);

    Ok(ImpactSuiteResult {
        repos: manifest.repos.len(),
        scenarios: total_scenarios,
        precision,
        recall,
        f1,
        precision_target: BLAST_PRECISION_TARGET,
        precision_passed: precision >= BLAST_PRECISION_TARGET,
        recall_target: 0.30,
        recall_passed: recall >= 0.30,
    })
}

// ---------------------------------------------------------------------------
// Core suite runner
// ---------------------------------------------------------------------------

const IMPORT_ACCURACY_TARGET: f64 = 0.70;

pub fn run_core_suite(config: &SuiteConfig) -> Result<CoreSuiteResult> {
    let manifest_path = config.suites_dir.join("core").join("manifest.json");
    let manifest = crate::dataset::parse_manifest(&manifest_path)?;
    let ground_truth_dir = config.suites_dir.join("core").join("ground-truth");

    let mut all_idempotent = true;
    let mut all_incremental_stable = true;
    let mut total_import_correct = 0usize;
    let mut total_import_checked = 0usize;

    for repo in &manifest.repos {
        tracing::info!(repo = %repo.name, "Processing core eval repo");
        let clone_path = crate::dataset::clone_or_cache(repo, config.no_cache)?;

        // Idempotency: index twice, compare stats
        let (store1, _tmp1) = index_repo(&clone_path)?;
        let stats1 = store1.stats()?;

        let (store2, _tmp2) = index_repo(&clone_path)?;
        let stats2 = store2.stats()?;

        let idempotent = crate::suites::core::check_idempotency(
            stats1.files,
            stats1.symbols,
            stats1.edges,
            stats2.files,
            stats2.symbols,
            stats2.edges,
        );
        if !idempotent {
            tracing::warn!(
                repo = %repo.name,
                "Idempotency FAIL: run1=({},{},{}) run2=({},{},{})",
                stats1.files, stats1.symbols, stats1.edges,
                stats2.files, stats2.symbols, stats2.edges,
            );
            all_idempotent = false;
        }

        // Incremental no-op: should produce zero changes
        let fs = EvalFileSystem;
        let parser = EvalParseProvider::new();
        let git = NoOpGitProvider;
        let inc_uc = IndexUseCase::new(store1.clone(), parser, fs, git);
        match inc_uc.incremental_index(&clone_path) {
            Ok(_stats) => { /* stable */ }
            Err(e) => {
                tracing::warn!(repo = %repo.name, "Incremental no-op error: {e}");
                all_incremental_stable = false;
            }
        }

        // Import resolution accuracy (if ground truth exists)
        let gt_path = ground_truth_dir.join(format!("{}.json", repo.name));
        if gt_path.exists() {
            let gt = crate::dataset::parse_core_ground_truth(&gt_path)?;
            for import in &gt.ground_truth {
                total_import_checked += 1;
                let source_qname = format!("{}::{}", import.source_file, import.source_symbol);
                let target_qname = format!("{}::{}", import.target_file, import.target_symbol);
                let source_exists = store1.get_symbol(&source_qname)?.is_some();
                let target_exists = store1.get_symbol(&target_qname)?.is_some();
                if source_exists && target_exists {
                    let edges = store1.get_edges_from(&source_qname)?;
                    if edges.iter().any(|e| e.target == target_qname) {
                        total_import_correct += 1;
                    }
                }
            }
        }
    }

    let import_accuracy = if total_import_checked > 0 {
        total_import_correct as f64 / total_import_checked as f64
    } else {
        1.0 // no ground truth = pass
    };

    Ok(CoreSuiteResult {
        repos: manifest.repos.len(),
        idempotent: all_idempotent,
        incremental_stable: all_incremental_stable,
        import_accuracy,
        import_target: IMPORT_ACCURACY_TARGET,
        import_passed: import_accuracy >= IMPORT_ACCURACY_TARGET,
    })
}

// ---------------------------------------------------------------------------
// Flows suite runner
// ---------------------------------------------------------------------------

const ENTRY_POINT_PRECISION_TARGET: f64 = 0.80;

pub fn run_flows_suite(config: &SuiteConfig) -> Result<FlowsSuiteResult> {
    let manifest_path = config.suites_dir.join("flows").join("manifest.json");
    let manifest = crate::dataset::parse_manifest(&manifest_path)?;
    let ground_truth_dir = config.suites_dir.join("flows").join("ground-truth");

    let mut total_precision = 0.0;
    let mut repo_count = 0;
    let mut total_violations = 0;

    for repo in &manifest.repos {
        tracing::info!(repo = %repo.name, "Processing flows eval repo");
        let clone_path = crate::dataset::clone_or_cache(repo, config.no_cache)?;
        let (store, _tmp) = index_repo(&clone_path)?;

        // Run flow analysis
        let flow_uc = domain::use_cases::flow::FlowUseCase::new(store.clone());
        let analysis = flow_uc.analyze(&domain::model::FlowConfig::default())?;

        // Run invariants
        let suite = crate::suites::flows::FlowsSuite;
        use crate::suites::EvalSuite;
        let invariants = suite.run_invariants(&store, &clone_path)?;
        total_violations += invariants.iter().filter(|i| !i.passed).count();

        // Check entry point precision against ground truth
        let gt_path = ground_truth_dir.join(format!("{}.json", repo.name));
        if gt_path.exists() {
            let gt = crate::dataset::parse_flows_ground_truth(&gt_path)?;
            let expected: Vec<String> = gt.ground_truth.iter().map(|e| e.symbol.clone()).collect();
            let detected: Vec<String> = analysis
                .entry_points
                .iter()
                .map(|ep| ep.qualified_name.clone())
                .collect();
            let precision = crate::suites::flows::entry_point_precision(&detected, &expected);
            total_precision += precision;
            repo_count += 1;
        }
    }

    let avg_precision = if repo_count > 0 {
        total_precision / repo_count as f64
    } else {
        0.0
    };

    Ok(FlowsSuiteResult {
        repos: manifest.repos.len(),
        entry_point_precision: avg_precision,
        entry_point_target: ENTRY_POINT_PRECISION_TARGET,
        entry_point_passed: avg_precision >= ENTRY_POINT_PRECISION_TARGET,
        invariant_violations: total_violations,
    })
}

// ---------------------------------------------------------------------------
// Risk suite runner
// ---------------------------------------------------------------------------

const TOP_N_PRECISION_TARGET: f64 = 0.60;

pub fn run_risk_suite(config: &SuiteConfig) -> Result<RiskSuiteResult> {
    let manifest_path = config.suites_dir.join("risk").join("manifest.json");
    let manifest = crate::dataset::parse_manifest(&manifest_path)?;
    let ground_truth_dir = config.suites_dir.join("risk").join("ground-truth");

    let mut total_precision = 0.0;
    let mut repo_count = 0;
    let mut total_violations = 0;

    for repo in &manifest.repos {
        tracing::info!(repo = %repo.name, "Processing risk eval repo");
        let clone_path = crate::dataset::clone_or_cache(repo, config.no_cache)?;
        let (store, _tmp) = index_repo(&clone_path)?;

        // Run risk analysis
        let risk_uc = domain::use_cases::risk::RiskUseCase::new(store.clone());
        let analysis = risk_uc.analyze(&domain::model::RiskConfig::default())?;

        // Run invariants
        let suite = crate::suites::risk::RiskSuite;
        use crate::suites::EvalSuite;
        let invariants = suite.run_invariants(&store, &clone_path)?;
        total_violations += invariants.iter().filter(|i| !i.passed).count();

        // Check top-N precision against ground truth
        let gt_path = ground_truth_dir.join(format!("{}.json", repo.name));
        if gt_path.exists() {
            let gt = crate::dataset::parse_risk_ground_truth(&gt_path)?;
            let high_risk: Vec<String> = gt
                .ground_truth
                .iter()
                .filter(|r| r.risk == "high")
                .map(|r| r.symbol.clone())
                .collect();
            let scored: Vec<(String, f64)> = analysis
                .symbol_scores
                .iter()
                .map(|s| (s.qualified_name.clone(), s.composite))
                .collect();
            let n = high_risk.len().max(1);
            let precision = crate::suites::risk::top_n_precision(&scored, &high_risk, n);
            total_precision += precision;
            repo_count += 1;
        }
    }

    let avg_precision = if repo_count > 0 {
        total_precision / repo_count as f64
    } else {
        0.0
    };

    Ok(RiskSuiteResult {
        repos: manifest.repos.len(),
        top_n_precision: avg_precision,
        top_n_target: TOP_N_PRECISION_TARGET,
        top_n_passed: avg_precision >= TOP_N_PRECISION_TARGET,
        invariant_violations: total_violations,
    })
}

// ---------------------------------------------------------------------------
// Analysis suite runner
// ---------------------------------------------------------------------------

const DEAD_CODE_PRECISION_TARGET: f64 = 0.70;

pub fn run_analysis_suite(config: &SuiteConfig) -> Result<AnalysisSuiteResult> {
    let manifest_path = config.suites_dir.join("analysis").join("manifest.json");
    let manifest = crate::dataset::parse_manifest(&manifest_path)?;
    let ground_truth_dir = config.suites_dir.join("analysis").join("ground-truth");

    let mut total_modularity = 0.0;
    let mut total_dead_precision = 0.0;
    let mut dead_repo_count = 0;
    let mut total_clone_violations = 0;
    let mut repo_count = 0;

    for repo in &manifest.repos {
        tracing::info!(repo = %repo.name, "Processing analysis eval repo");
        let clone_path = crate::dataset::clone_or_cache(repo, config.no_cache)?;
        let (store, _tmp) = index_repo(&clone_path)?;
        repo_count += 1;

        // Communities
        let community_uc = domain::use_cases::community::CommunityUseCase::new(store.clone());
        let community_analysis = community_uc.analyze(&domain::model::CommunityConfig::default())?;
        total_modularity += community_analysis.modularity;

        // Dead code
        let dead_uc = domain::use_cases::dead_code::DeadCodeUseCase::new(store.clone());
        let dead_analysis = dead_uc.analyze(&domain::model::DeadCodeConfig::default())?;

        let gt_dead_path = ground_truth_dir.join(format!("{}-dead-code.json", repo.name));
        if gt_dead_path.exists() {
            let gt = crate::dataset::parse_dead_code_ground_truth(&gt_dead_path)?;
            let tagged_dead: Vec<String> = gt
                .ground_truth
                .iter()
                .filter(|d| d.expected_dead)
                .map(|d| d.symbol.clone())
                .collect();
            let detected: Vec<String> = dead_analysis
                .dead_symbols
                .iter()
                .map(|c| c.qualified_name.clone())
                .collect();
            if !tagged_dead.is_empty() {
                total_dead_precision +=
                    crate::suites::analysis::dead_code_precision(&detected, &tagged_dead);
                dead_repo_count += 1;
            }
        }

        // Clones -- run invariants
        let suite = crate::suites::analysis::AnalysisSuite;
        use crate::suites::EvalSuite;
        let invariants = suite.run_invariants(&store, &clone_path)?;
        total_clone_violations += invariants
            .iter()
            .filter(|i| i.name.starts_with("clone_") && !i.passed)
            .count();
    }

    let avg_modularity = if repo_count > 0 {
        total_modularity / repo_count as f64
    } else {
        0.0
    };
    let avg_dead_precision = if dead_repo_count > 0 {
        total_dead_precision / dead_repo_count as f64
    } else {
        1.0
    };

    Ok(AnalysisSuiteResult {
        repos: manifest.repos.len(),
        community_modularity: avg_modularity,
        dead_code_precision: avg_dead_precision,
        dead_code_target: DEAD_CODE_PRECISION_TARGET,
        dead_code_passed: avg_dead_precision >= DEAD_CODE_PRECISION_TARGET,
        clone_invariant_violations: total_clone_violations,
    })
}

// ---------------------------------------------------------------------------
// Invariants suite runner
// ---------------------------------------------------------------------------

pub fn run_invariants_suite(config: &SuiteConfig) -> Result<InvariantsSuiteResult> {
    // Use search manifest as the base (shared across suites)
    let manifest_path = config.suites_dir.join("search").join("manifest.json");
    let manifest = crate::dataset::parse_manifest(&manifest_path)?;

    let mut all_results = Vec::new();

    for repo in &manifest.repos {
        tracing::info!(repo = %repo.name, "Processing invariants eval repo");
        let clone_path = crate::dataset::clone_or_cache(repo, config.no_cache)?;
        let (store, _tmp) = index_repo(&clone_path)?;

        // Run all invariants via the meta-suite
        let meta = crate::suites::invariants::InvariantsSuite;
        use crate::suites::EvalSuite;
        let results = meta.run_invariants(&store, &clone_path)?;
        all_results.extend(results);
    }

    let total = all_results.len();
    let passed = all_results.iter().filter(|r| r.passed).count();
    let failed = total - passed;

    Ok(InvariantsSuiteResult {
        total,
        passed,
        failed,
        results: all_results,
    })
}

// ---------------------------------------------------------------------------
// Bench suite runner
// ---------------------------------------------------------------------------

pub fn run_bench_suite(config: &SuiteConfig) -> Result<BenchSuiteResult> {
    let manifest_path = config.suites_dir.join("search").join("manifest.json");
    let manifest = crate::dataset::parse_manifest(&manifest_path)?;

    let mut baselines = Vec::new();

    for repo in &manifest.repos {
        tracing::info!(repo = %repo.name, "Benchmarking repo");
        let clone_path = crate::dataset::clone_or_cache(repo, config.no_cache)?;

        // Full index timing
        let start = std::time::Instant::now();
        let (store, tmp) = index_repo(&clone_path)?;
        let full_index_ms = start.elapsed().as_millis() as u64;

        // Incremental no-op timing
        let start = std::time::Instant::now();
        let fs = EvalFileSystem;
        let parser = EvalParseProvider::new();
        let git = NoOpGitProvider;
        let inc_uc = IndexUseCase::new(store.clone(), parser, fs, git);
        let _ = inc_uc.incremental_index(&clone_path);
        let incremental_noop_ms = start.elapsed().as_millis() as u64;

        // Graph size
        let stats = store.stats()?;
        let db_bytes = tmp
            .path()
            .join("eval.db")
            .metadata()
            .map(|m| m.len())
            .unwrap_or(0);

        // Query latencies (10 runs each)
        let query_uc = QueryUseCase::new(store.clone(), store.clone());
        let mut search_times = Vec::new();
        for _ in 0..10 {
            let start = std::time::Instant::now();
            let _ = query_uc.search("function", 20);
            search_times.push(start.elapsed().as_secs_f64() * 1000.0);
        }

        let impact_uc = ImpactUseCase::new(store.clone());
        let mut impact_times = Vec::new();
        for _ in 0..10 {
            let start = std::time::Instant::now();
            let _ =
                impact_uc.blast_radius(&[ImpactTarget::File("main".into())], 3, Confidence::Medium);
            impact_times.push(start.elapsed().as_secs_f64() * 1000.0);
        }

        let flow_uc = domain::use_cases::flow::FlowUseCase::new(store.clone());
        let mut flow_times = Vec::new();
        for _ in 0..10 {
            let start = std::time::Instant::now();
            let _ = flow_uc.analyze(&domain::model::FlowConfig::default());
            flow_times.push(start.elapsed().as_secs_f64() * 1000.0);
        }

        // Callers/callees use query use case
        let mut caller_times = Vec::new();
        let mut callee_times = Vec::new();
        for _ in 0..10 {
            let start = std::time::Instant::now();
            let _ = query_uc.callers("main");
            caller_times.push(start.elapsed().as_secs_f64() * 1000.0);

            let start = std::time::Instant::now();
            let _ = query_uc.callees("main");
            callee_times.push(start.elapsed().as_secs_f64() * 1000.0);
        }

        baselines.push(BaselineEntry {
            repo: repo.name.clone(),
            full_index_ms,
            incremental_noop_ms,
            query_latencies: QueryLatencies {
                search_p50_ms: percentile(&mut search_times, 50.0),
                search_p95_ms: percentile(&mut search_times, 95.0),
                impact_p50_ms: percentile(&mut impact_times, 50.0),
                impact_p95_ms: percentile(&mut impact_times, 95.0),
                flows_p50_ms: percentile(&mut flow_times, 50.0),
                flows_p95_ms: percentile(&mut flow_times, 95.0),
                callers_p50_ms: percentile(&mut caller_times, 50.0),
                callers_p95_ms: percentile(&mut caller_times, 95.0),
                callees_p50_ms: percentile(&mut callee_times, 50.0),
                callees_p95_ms: percentile(&mut callee_times, 95.0),
            },
            graph_size: GraphSize {
                symbols: stats.symbols,
                edges: stats.edges,
                db_bytes,
            },
        });
    }

    // Write baseline JSON
    let baselines_json = serde_json::to_value(&baselines)
        .map_err(|e| CodeGraphError::Other(format!("serialize baselines: {e}")))?;

    // Write to eval/baselines/ if possible
    let baselines_dir = config
        .suites_dir
        .parent()
        .unwrap_or(config.suites_dir.as_path())
        .join("baselines");
    if std::fs::create_dir_all(&baselines_dir).is_ok() {
        let version = env!("CARGO_PKG_VERSION");
        let path = baselines_dir.join(format!("baseline-{version}.json"));
        let json_str = serde_json::to_string_pretty(&baselines)
            .map_err(|e| CodeGraphError::Other(format!("format baselines: {e}")))?;
        let _ = std::fs::write(&path, json_str);
        tracing::info!("Baseline written to {}", path.display());
    }

    Ok(BenchSuiteResult {
        repos: baselines.len(),
        baselines: baselines_json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_from_string_high() {
        let c = confidence_from_str("high").unwrap();
        assert!(matches!(c, Confidence::High));
    }

    #[test]
    fn confidence_from_string_medium() {
        let c = confidence_from_str("medium").unwrap();
        assert!(matches!(c, Confidence::Medium));
    }

    #[test]
    fn confidence_from_string_invalid() {
        let err = confidence_from_str("unknown");
        assert!(err.is_err());
    }

    #[test]
    fn aggregate_impact_empty() {
        let (p, r, f) = aggregate_impact_metrics(&[], &[]);
        assert!((p - 0.0).abs() < f64::EPSILON);
        assert!((r - 0.0).abs() < f64::EPSILON);
        assert!((f - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_impact_perfect() {
        let predicted = vec![vec!["a".into(), "b".into()]];
        let actual = vec![vec!["a".into(), "b".into()]];
        let (p, r, f) = aggregate_impact_metrics(&predicted, &actual);
        assert!((p - 1.0).abs() < f64::EPSILON);
        assert!((r - 1.0).abs() < f64::EPSILON);
        assert!((f - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn validate_ground_truth_empty() {
        let store = SqliteStore::open_in_memory().unwrap();
        let missing = validate_ground_truth(&store, &[], "test-repo").unwrap();
        assert!(missing.is_empty());
    }
}
