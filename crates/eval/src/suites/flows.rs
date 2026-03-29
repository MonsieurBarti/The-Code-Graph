use std::path::Path;

use domain::error::Result;
use storage::SqliteStore;

use super::{EvalSuite, InvariantResult, MetricResult};

pub struct FlowsSuite;

/// Compute precision of detected entry points vs expected.
pub fn entry_point_precision(detected: &[String], expected: &[String]) -> f64 {
    if detected.is_empty() {
        return 0.0;
    }
    let expected_set: std::collections::HashSet<&str> =
        expected.iter().map(|s| s.as_str()).collect();
    let hits = detected
        .iter()
        .filter(|d| expected_set.contains(d.as_str()))
        .count();
    hits as f64 / detected.len() as f64
}

/// Check if a path (sequence of symbol names) is acyclic.
pub fn is_acyclic(path: &[String]) -> bool {
    let mut seen = std::collections::HashSet::new();
    path.iter().all(|node| seen.insert(node.as_str()))
}

impl EvalSuite for FlowsSuite {
    fn name(&self) -> &str {
        "flows"
    }

    fn run_metrics(
        &self,
        _store: &SqliteStore,
        _clone_path: &Path,
        _ground_truth_dir: &Path,
    ) -> Result<Vec<MetricResult>> {
        // Flow metrics are computed at the suite dispatch level
        Ok(vec![])
    }

    fn run_invariants(
        &self,
        store: &SqliteStore,
        _clone_path: &Path,
    ) -> Result<Vec<InvariantResult>> {
        use domain::model::FlowConfig;
        use domain::use_cases::flow::FlowUseCase;

        let flow_uc = FlowUseCase::new(store.clone());
        let analysis = flow_uc.analyze(&FlowConfig::default())?;
        let mut results = Vec::new();

        // Invariant: betweenness scores are non-negative
        let all_non_negative = analysis.criticality.iter().all(|c| c.betweenness >= 0.0);
        results.push(InvariantResult {
            name: "flows_betweenness_non_negative".into(),
            suite: "flows".into(),
            passed: all_non_negative,
            message: if all_non_negative {
                None
            } else {
                Some("Found negative betweenness scores".into())
            },
        });

        // Invariant: betweenness in [0.0, 1.0]
        let all_in_range = analysis
            .criticality
            .iter()
            .all(|c| c.betweenness >= 0.0 && c.betweenness <= 1.0);
        results.push(InvariantResult {
            name: "flows_betweenness_in_range".into(),
            suite: "flows".into(),
            passed: all_in_range,
            message: if all_in_range {
                None
            } else {
                let out_of_range: Vec<_> = analysis
                    .criticality
                    .iter()
                    .filter(|c| c.betweenness < 0.0 || c.betweenness > 1.0)
                    .map(|c| format!("{}={:.4}", c.qualified_name, c.betweenness))
                    .take(5)
                    .collect();
                Some(format!("Out of range: {}", out_of_range.join(", ")))
            },
        });

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flows_suite_name() {
        let suite = FlowsSuite;
        assert_eq!(suite.name(), "flows");
    }

    #[test]
    fn entry_point_precision_perfect() {
        let detected = vec!["main".to_string(), "handler".to_string()];
        let expected = vec!["main".to_string(), "handler".to_string()];
        let p = entry_point_precision(&detected, &expected);
        assert!((p - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn entry_point_precision_empty() {
        let detected: Vec<String> = vec![];
        let expected = vec!["main".to_string()];
        let p = entry_point_precision(&detected, &expected);
        assert!((p - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn entry_point_precision_half() {
        let detected = vec!["main".to_string(), "unknown".to_string()];
        let expected = vec!["main".to_string()];
        let p = entry_point_precision(&detected, &expected);
        assert!((p - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn flow_path_is_acyclic() {
        let path = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert!(is_acyclic(&path));
    }

    #[test]
    fn flow_path_with_cycle() {
        let path = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        assert!(!is_acyclic(&path));
    }

    #[test]
    fn flow_path_empty() {
        let path: Vec<String> = vec![];
        assert!(is_acyclic(&path));
    }
}
