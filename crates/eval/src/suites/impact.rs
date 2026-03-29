use std::path::Path;

use domain::error::Result;
use storage::SqliteStore;

use super::{EvalSuite, InvariantResult, MetricResult};

pub struct ImpactSuite;

impl EvalSuite for ImpactSuite {
    fn name(&self) -> &str {
        "impact"
    }

    fn run_metrics(
        &self,
        _store: &SqliteStore,
        _clone_path: &Path,
        _ground_truth_dir: &Path,
    ) -> Result<Vec<MetricResult>> {
        // Impact metrics are computed via runner::run_impact_suite()
        // This trait impl provides a uniform interface but delegates to existing logic
        Ok(vec![])
    }

    fn run_invariants(
        &self,
        store: &SqliteStore,
        _clone_path: &Path,
    ) -> Result<Vec<InvariantResult>> {
        use domain::ports::GraphStore;
        let stats = store.stats()?;

        let mut results = Vec::new();

        // Invariant: graph must have edges for impact analysis to be meaningful
        results.push(InvariantResult {
            name: "impact_graph_has_edges".into(),
            suite: "impact".into(),
            passed: stats.edges > 0 || stats.symbols == 0,
            message: Some(format!("{} edges in graph", stats.edges)),
        });

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_suite_name() {
        let suite = ImpactSuite;
        assert_eq!(suite.name(), "impact");
    }
}
