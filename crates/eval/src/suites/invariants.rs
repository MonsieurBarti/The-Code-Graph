use std::path::Path;

use domain::error::Result;
use storage::SqliteStore;

use super::{EvalSuite, InvariantResult, MetricResult};

pub struct InvariantsSuite;

impl EvalSuite for InvariantsSuite {
    fn name(&self) -> &str {
        "invariants"
    }

    fn run_metrics(
        &self,
        _store: &SqliteStore,
        _clone_path: &Path,
        _ground_truth_dir: &Path,
    ) -> Result<Vec<MetricResult>> {
        // Invariants meta-suite has no ground-truth-based metrics
        Ok(vec![])
    }

    fn run_invariants(
        &self,
        store: &SqliteStore,
        clone_path: &Path,
    ) -> Result<Vec<InvariantResult>> {
        let all = super::all_suites();
        let mut results = Vec::new();
        for suite in &all {
            // Skip the invariants suite itself to avoid infinite recursion
            if suite.name() == "invariants" {
                continue;
            }
            match suite.run_invariants(store, clone_path) {
                Ok(invariants) => results.extend(invariants),
                Err(e) => {
                    results.push(InvariantResult {
                        name: format!("{}_suite_error", suite.name()),
                        suite: suite.name().to_string(),
                        passed: false,
                        message: Some(format!("Suite error: {e}")),
                    });
                }
            }
        }
        Ok(results)
    }
}

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
        let suites = super::super::all_suites();
        assert!(
            !suites.is_empty(),
            "all_suites() must have registered suites"
        );
        assert!(suites.len() >= 8, "Expected at least 8 suites");
    }
}
