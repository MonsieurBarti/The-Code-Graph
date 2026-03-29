use std::path::Path;

use domain::error::Result;
use storage::SqliteStore;

use super::{EvalSuite, InvariantResult, MetricResult};

pub struct RiskSuite;

/// Top-N precision: fraction of top-N scored symbols that appear in the high-risk set.
pub fn top_n_precision(scored: &[(String, f64)], high_risk: &[String], n: usize) -> f64 {
    if scored.is_empty() || n == 0 {
        return 0.0;
    }
    let risk_set: std::collections::HashSet<&str> = high_risk.iter().map(|s| s.as_str()).collect();
    let effective_n = n.min(scored.len());
    let hits = scored[..effective_n]
        .iter()
        .filter(|(name, _)| risk_set.contains(name.as_str()))
        .count();
    hits as f64 / effective_n as f64
}

impl EvalSuite for RiskSuite {
    fn name(&self) -> &str {
        "risk"
    }

    fn run_metrics(
        &self,
        _store: &SqliteStore,
        _clone_path: &Path,
        _ground_truth_dir: &Path,
    ) -> Result<Vec<MetricResult>> {
        Ok(vec![])
    }

    fn run_invariants(
        &self,
        store: &SqliteStore,
        _clone_path: &Path,
    ) -> Result<Vec<InvariantResult>> {
        use domain::model::RiskConfig;
        use domain::use_cases::risk::RiskUseCase;

        let risk_uc = RiskUseCase::new(store.clone());
        let analysis = risk_uc.analyze(&RiskConfig::default())?;
        let mut results = Vec::new();

        // Invariant: all composite scores in [0.0, 1.0]
        let composites_in_range = analysis
            .symbol_scores
            .iter()
            .all(|s| s.composite >= 0.0 && s.composite <= 1.0);
        results.push(InvariantResult {
            name: "risk_composite_in_range".into(),
            suite: "risk".into(),
            passed: composites_in_range,
            message: if composites_in_range {
                None
            } else {
                let violations: Vec<_> = analysis
                    .symbol_scores
                    .iter()
                    .filter(|s| s.composite < 0.0 || s.composite > 1.0)
                    .map(|s| format!("{}={:.4}", s.qualified_name, s.composite))
                    .take(5)
                    .collect();
                Some(format!("Out of range: {}", violations.join(", ")))
            },
        });

        // Invariant: all RiskFactors components in [0.0, 1.0]
        let factors_in_range = analysis.symbol_scores.iter().all(|s| {
            let f = &s.factors;
            f.criticality >= 0.0
                && f.criticality <= 1.0
                && f.coupling >= 0.0
                && f.coupling <= 1.0
                && f.test_gap >= 0.0
                && f.test_gap <= 1.0
                && f.sensitivity >= 0.0
                && f.sensitivity <= 1.0
        });
        results.push(InvariantResult {
            name: "risk_factors_in_range".into(),
            suite: "risk".into(),
            passed: factors_in_range,
            message: if factors_in_range {
                None
            } else {
                Some("Some risk factor components out of [0.0, 1.0]".into())
            },
        });

        // Invariant: zero-edge symbols must have composite risk < 0.2
        // A symbol with no incoming or outgoing edges is isolated and should
        // not be ranked as high-risk.
        use domain::ports::GraphStore;
        let mut violations: Vec<String> = Vec::new();
        for score in &analysis.symbol_scores {
            let outgoing = store.get_edges_from(&score.qualified_name)?.len();
            let incoming = store.get_edges_to(&score.qualified_name)?.len();
            if outgoing == 0 && incoming == 0 && score.composite >= 0.2 {
                violations.push(format!("{}={:.4}", score.qualified_name, score.composite));
            }
        }
        let zero_edge_ok = violations.is_empty();
        results.push(InvariantResult {
            name: "risk_zero_edge_below_threshold".into(),
            suite: "risk".into(),
            passed: zero_edge_ok,
            message: if zero_edge_ok {
                None
            } else {
                Some(format!(
                    "Zero-edge symbols with composite >= 0.2: {}",
                    violations.join(", ")
                ))
            },
        });

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_suite_name() {
        let suite = RiskSuite;
        assert_eq!(suite.name(), "risk");
    }

    #[test]
    fn top_n_precision_perfect() {
        let scored = vec![("a".to_string(), 0.9), ("b".to_string(), 0.8)];
        let high_risk = vec!["a".to_string(), "b".to_string()];
        let p = top_n_precision(&scored, &high_risk, 2);
        assert!((p - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn top_n_precision_none() {
        let scored = vec![("x".to_string(), 0.9), ("y".to_string(), 0.8)];
        let high_risk = vec!["a".to_string(), "b".to_string()];
        let p = top_n_precision(&scored, &high_risk, 2);
        assert!((p - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn top_n_precision_empty() {
        let scored: Vec<(String, f64)> = vec![];
        let high_risk = vec!["a".to_string()];
        let p = top_n_precision(&scored, &high_risk, 5);
        assert!((p - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn top_n_precision_half() {
        let scored = vec![("a".to_string(), 0.9), ("x".to_string(), 0.8)];
        let high_risk = vec!["a".to_string()];
        let p = top_n_precision(&scored, &high_risk, 2);
        assert!((p - 0.5).abs() < f64::EPSILON);
    }
}
