use domain::error::Result;
use std::path::Path;
use storage::SqliteStore;

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

/// Registry of all available suites.
pub fn all_suites() -> Vec<Box<dyn EvalSuite>> {
    vec![
        Box::new(search::SearchSuite),
        Box::new(impact::ImpactSuite),
        Box::new(core::CoreSuite),
        Box::new(flows::FlowsSuite),
        Box::new(risk::RiskSuite),
        Box::new(analysis::AnalysisSuite),
        Box::new(invariants::InvariantsSuite),
        Box::new(bench::BenchSuite::new(None)),
    ]
}

pub mod analysis;
pub mod bench;
pub mod core;
pub mod flows;
pub mod impact;
pub mod invariants;
pub mod risk;
pub mod search;

#[cfg(test)]
mod tests {
    use super::*;

    struct DummySuite;
    impl EvalSuite for DummySuite {
        fn name(&self) -> &str {
            "dummy"
        }
        fn run_metrics(&self, _: &SqliteStore, _: &Path, _: &Path) -> Result<Vec<MetricResult>> {
            Ok(vec![MetricResult {
                name: "test_metric".into(),
                value: 0.5,
                target: Some(0.3),
                passed: true,
            }])
        }
        fn run_invariants(&self, _: &SqliteStore, _: &Path) -> Result<Vec<InvariantResult>> {
            Ok(vec![InvariantResult {
                name: "test_inv".into(),
                suite: "dummy".into(),
                passed: true,
                message: None,
            }])
        }
    }

    #[test]
    fn eval_suite_trait_dispatch() {
        let suite: Box<dyn EvalSuite> = Box::new(DummySuite);
        assert_eq!(suite.name(), "dummy");
    }

    #[test]
    fn metric_result_serializes() {
        let m = MetricResult {
            name: "mrr".into(),
            value: 0.5,
            target: Some(0.3),
            passed: true,
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"mrr\""));
    }

    #[test]
    fn all_suites_registry_complete() {
        let suites = all_suites();
        assert!(
            suites.len() >= 8,
            "Expected at least 8 suites, got {}",
            suites.len()
        );
        let names: Vec<&str> = suites.iter().map(|s| s.name()).collect();
        assert!(names.contains(&"search"));
        assert!(names.contains(&"impact"));
        assert!(names.contains(&"core"));
        assert!(names.contains(&"flows"));
        assert!(names.contains(&"risk"));
        assert!(names.contains(&"analysis"));
        assert!(names.contains(&"invariants"));
        assert!(names.contains(&"bench"));
    }

    #[test]
    fn invariant_result_serializes() {
        let i = InvariantResult {
            name: "scores_in_range".into(),
            suite: "risk".into(),
            passed: true,
            message: None,
        };
        let json = serde_json::to_string(&i).unwrap();
        assert!(json.contains("\"scores_in_range\""));
    }
}
