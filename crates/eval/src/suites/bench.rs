use std::path::{Path, PathBuf};

use domain::error::Result;
use serde::{Deserialize, Serialize};
use storage::SqliteStore;

use super::{EvalSuite, InvariantResult, MetricResult};

pub struct BenchSuite {
    pub compare_path: Option<PathBuf>,
}

impl BenchSuite {
    pub fn new(compare_path: Option<PathBuf>) -> Self {
        Self { compare_path }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineEntry {
    pub repo: String,
    pub full_index_ms: u64,
    pub incremental_noop_ms: u64,
    pub query_latencies: QueryLatencies,
    pub graph_size: GraphSize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryLatencies {
    pub search_p50_ms: f64,
    pub search_p95_ms: f64,
    pub impact_p50_ms: f64,
    pub impact_p95_ms: f64,
    pub flows_p50_ms: f64,
    pub flows_p95_ms: f64,
    pub callers_p50_ms: f64,
    pub callers_p95_ms: f64,
    pub callees_p50_ms: f64,
    pub callees_p95_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSize {
    pub symbols: usize,
    pub edges: usize,
    pub db_bytes: u64,
}

/// Compute the percentile value from a slice. Mutates the input for sorting.
pub fn percentile(values: &mut [f64], pct: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((pct / 100.0) * (values.len() as f64 - 1.0)).round() as usize;
    values[idx.min(values.len() - 1)]
}

impl EvalSuite for BenchSuite {
    fn name(&self) -> &str {
        "bench"
    }

    fn run_metrics(
        &self,
        _store: &SqliteStore,
        _clone_path: &Path,
        _ground_truth_dir: &Path,
    ) -> Result<Vec<MetricResult>> {
        // Bench metrics are computed at suite dispatch level with full repo access
        Ok(vec![])
    }

    fn run_invariants(
        &self,
        _store: &SqliteStore,
        _clone_path: &Path,
    ) -> Result<Vec<InvariantResult>> {
        // Bench has no invariants
        Ok(vec![])
    }
}

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
                search_p50_ms: 5.0,
                search_p95_ms: 12.0,
                impact_p50_ms: 8.0,
                impact_p95_ms: 20.0,
                flows_p50_ms: 15.0,
                flows_p95_ms: 30.0,
                callers_p50_ms: 2.0,
                callers_p95_ms: 5.0,
                callees_p50_ms: 2.0,
                callees_p95_ms: 5.0,
            },
            graph_size: GraphSize {
                symbols: 1000,
                edges: 5000,
                db_bytes: 1_000_000,
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"full_index_ms\""));
        assert!(json.contains("\"search_p50_ms\""));
    }

    #[test]
    fn baseline_entry_roundtrip() {
        let entry = BaselineEntry {
            repo: "test".into(),
            full_index_ms: 100,
            incremental_noop_ms: 10,
            query_latencies: QueryLatencies {
                search_p50_ms: 1.0,
                search_p95_ms: 2.0,
                impact_p50_ms: 3.0,
                impact_p95_ms: 4.0,
                flows_p50_ms: 5.0,
                flows_p95_ms: 6.0,
                callers_p50_ms: 7.0,
                callers_p95_ms: 8.0,
                callees_p50_ms: 9.0,
                callees_p95_ms: 10.0,
            },
            graph_size: GraphSize {
                symbols: 500,
                edges: 2000,
                db_bytes: 500_000,
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: BaselineEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.repo, "test");
        assert_eq!(back.full_index_ms, 100);
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

    #[test]
    fn percentile_single() {
        let mut values = vec![42.0];
        assert!((percentile(&mut values, 50.0) - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_empty() {
        let mut values: Vec<f64> = vec![];
        assert!((percentile(&mut values, 50.0) - 0.0).abs() < f64::EPSILON);
    }
}
