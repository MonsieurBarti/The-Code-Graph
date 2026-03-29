use std::path::Path;

use domain::error::Result;
use storage::SqliteStore;

use super::{EvalSuite, InvariantResult, MetricResult};

pub struct SearchSuite;

impl EvalSuite for SearchSuite {
    fn name(&self) -> &str {
        "search"
    }

    fn run_metrics(
        &self,
        _store: &SqliteStore,
        _clone_path: &Path,
        _ground_truth_dir: &Path,
    ) -> Result<Vec<MetricResult>> {
        // Search metrics are computed via runner::run_search_suite()
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

        // Invariant: FTS index should cover all indexed symbols
        // Note: this is a best-effort check — the FTS table may not be directly queryable here
        // The invariant checks that stats.symbols > 0 when we have a populated store
        results.push(InvariantResult {
            name: "search_symbols_indexed".into(),
            suite: "search".into(),
            passed: stats.symbols > 0 || stats.files == 0,
            message: Some(format!(
                "{} symbols indexed across {} files",
                stats.symbols, stats.files
            )),
        });

        Ok(results)
    }
}

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
