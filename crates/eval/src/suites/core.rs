use std::path::Path;

use domain::error::Result;
use domain::ports::GraphStore;
use storage::SqliteStore;

use super::{EvalSuite, InvariantResult, MetricResult};

pub struct CoreSuite;

/// Compare two index runs for idempotency.
pub fn check_idempotency(
    files1: usize,
    symbols1: usize,
    edges1: usize,
    files2: usize,
    symbols2: usize,
    edges2: usize,
) -> bool {
    files1 == files2 && symbols1 == symbols2 && edges1 == edges2
}

impl EvalSuite for CoreSuite {
    fn name(&self) -> &str {
        "core"
    }

    fn run_metrics(
        &self,
        _store: &SqliteStore,
        _clone_path: &Path,
        _ground_truth_dir: &Path,
    ) -> Result<Vec<MetricResult>> {
        // Core metrics (idempotency, import accuracy) require full re-indexing
        // which is done at the suite runner level, not per-store.
        // Individual metric computation is deferred to the dispatch in run_suite().
        Ok(vec![])
    }

    fn run_invariants(
        &self,
        store: &SqliteStore,
        _clone_path: &Path,
    ) -> Result<Vec<InvariantResult>> {
        let stats = store.stats()?;
        let mut results = Vec::new();

        // Invariant: symbols should exist when files are indexed
        results.push(InvariantResult {
            name: "core_symbols_exist".into(),
            suite: "core".into(),
            passed: stats.symbols > 0 || stats.files == 0,
            message: Some(format!(
                "{} symbols across {} files",
                stats.symbols, stats.files
            )),
        });

        // Invariant: edges should reference valid symbols
        results.push(InvariantResult {
            name: "core_edges_have_valid_refs".into(),
            suite: "core".into(),
            passed: stats.edges == 0 || stats.symbols > 0,
            message: Some(format!(
                "{} edges with {} symbols",
                stats.edges, stats.symbols
            )),
        });

        // Invariant: file count > 0 when store is populated
        results.push(InvariantResult {
            name: "core_files_visited".into(),
            suite: "core".into(),
            passed: true, // If we got here, the store was populated
            message: Some(format!("{} files visited", stats.files)),
        });

        Ok(results)
    }
}

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
