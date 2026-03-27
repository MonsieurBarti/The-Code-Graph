use std::path::Path;

use domain::error::{CodeGraphError, Result};
use domain::model::{Confidence, ImpactTarget};
use domain::ports::GraphStore;
use domain::use_cases::impact::ImpactUseCase;
use domain::use_cases::index::IndexUseCase;
use domain::use_cases::query::QueryUseCase;
use storage::SqliteStore;

use crate::adapters::{EvalFileSystem, EvalParseProvider, NoOpGitProvider};
use crate::dataset::{ImpactScenario, SearchQuery};
use crate::report::{ImpactSuiteResult, SearchSuiteResult};
use crate::{metrics, SuiteConfig};

const MRR_TARGET: f64 = 0.30;
const BLAST_PRECISION_TARGET: f64 = 0.40;

/// Ranked results paired with ground-truth expectations.
type RankedVsTruth = (Vec<Vec<String>>, Vec<Vec<String>>);

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
) -> Result<RankedVsTruth> {
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
            let (ranked, truth) = run_search_queries(&store, &filtered, config.search_limit)?;
            total_queries += ranked.len();
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

    Ok(SearchSuiteResult {
        repos: manifest.repos.len(),
        queries: total_queries,
        mrr,
        precision_at_5: p5,
        precision_at_10: p10,
        mrr_target: MRR_TARGET,
        mrr_passed: mrr >= MRR_TARGET,
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
