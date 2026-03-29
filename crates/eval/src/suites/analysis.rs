use std::path::Path;

use domain::error::Result;
use storage::SqliteStore;

use super::{EvalSuite, InvariantResult, MetricResult};

pub struct AnalysisSuite;

/// Fraction of detected dead code symbols that match tagged ground truth.
pub fn dead_code_precision(detected: &[String], tagged: &[String]) -> f64 {
    if detected.is_empty() {
        return 0.0;
    }
    let tagged_set: std::collections::HashSet<&str> = tagged.iter().map(|s| s.as_str()).collect();
    let hits = detected
        .iter()
        .filter(|d| tagged_set.contains(d.as_str()))
        .count();
    hits as f64 / detected.len() as f64
}

impl EvalSuite for AnalysisSuite {
    fn name(&self) -> &str {
        "analysis"
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
        use crate::adapters::EvalFileSystem;
        use domain::model::{CloneConfig, CommunityConfig, DeadCodeConfig};
        use domain::ports::GraphStore;
        use domain::use_cases::clones::CloneUseCase;
        use domain::use_cases::community::CommunityUseCase;
        use domain::use_cases::dead_code::DeadCodeUseCase;

        let mut results = Vec::new();

        // --- Communities ---
        let community_uc = CommunityUseCase::new(store.clone());
        let community_analysis = community_uc.analyze(&CommunityConfig::default())?;

        // Invariant: modularity > 0 when communities exist
        let has_communities = !community_analysis.communities.is_empty();
        let modularity_positive = !has_communities || community_analysis.modularity > 0.0;
        results.push(InvariantResult {
            name: "community_modularity_positive".into(),
            suite: "analysis".into(),
            passed: modularity_positive,
            message: if modularity_positive {
                None
            } else {
                Some(format!(
                    "Expected modularity > 0 when communities exist, got {:.4}",
                    community_analysis.modularity
                ))
            },
        });

        // Invariant: no empty communities
        let no_empty_communities = community_analysis
            .communities
            .iter()
            .all(|c| !c.members.is_empty());
        results.push(InvariantResult {
            name: "community_no_empty".into(),
            suite: "analysis".into(),
            passed: no_empty_communities,
            message: if no_empty_communities {
                None
            } else {
                let empty_ids: Vec<_> = community_analysis
                    .communities
                    .iter()
                    .filter(|c| c.members.is_empty())
                    .map(|c| c.id.to_string())
                    .take(5)
                    .collect();
                Some(format!("Empty communities: {}", empty_ids.join(", ")))
            },
        });

        // --- Dead code ---
        let dead_code_uc = DeadCodeUseCase::new(store.clone());
        let dead_analysis = dead_code_uc.analyze(&DeadCodeConfig::default())?;

        // Invariant: every reported dead symbol has zero incoming edges
        let mut dead_with_incoming: Vec<String> = Vec::new();
        for dead in &dead_analysis.dead_symbols {
            let incoming = store.get_edges_to(&dead.qualified_name)?;
            if !incoming.is_empty() {
                dead_with_incoming.push(dead.qualified_name.clone());
            }
        }
        let dead_zero_incoming = dead_with_incoming.is_empty();
        results.push(InvariantResult {
            name: "dead_code_zero_incoming_edges".into(),
            suite: "analysis".into(),
            passed: dead_zero_incoming,
            message: if dead_zero_incoming {
                None
            } else {
                Some(format!(
                    "Dead symbols with incoming edges: {}",
                    dead_with_incoming[..dead_with_incoming.len().min(5)].join(", ")
                ))
            },
        });

        // --- Clones ---
        let clone_uc = CloneUseCase::new(store.clone(), EvalFileSystem, _clone_path.to_path_buf());
        let clone_analysis = clone_uc.analyze(&CloneConfig::default())?;

        // Invariant: similarity scores in [0.0, 1.0]
        let all_matches: Vec<_> = clone_analysis
            .clusters
            .iter()
            .flat_map(|c| c.intra_matches.iter())
            .collect();
        let similarities_in_range = all_matches
            .iter()
            .all(|m| m.similarity >= 0.0 && m.similarity <= 1.0);
        results.push(InvariantResult {
            name: "clone_similarity_in_range".into(),
            suite: "analysis".into(),
            passed: similarities_in_range,
            message: if similarities_in_range {
                None
            } else {
                let violations: Vec<_> = all_matches
                    .iter()
                    .filter(|m| m.similarity < 0.0 || m.similarity > 1.0)
                    .map(|m| format!("{}↔{}={:.4}", m.source, m.target, m.similarity))
                    .take(5)
                    .collect();
                Some(format!("Out of range: {}", violations.join(", ")))
            },
        });

        // Also check avg_similarity on clusters
        let cluster_avg_in_range = clone_analysis
            .clusters
            .iter()
            .all(|c| c.avg_similarity >= 0.0 && c.avg_similarity <= 1.0);
        results.push(InvariantResult {
            name: "clone_cluster_avg_similarity_in_range".into(),
            suite: "analysis".into(),
            passed: cluster_avg_in_range,
            message: if cluster_avg_in_range {
                None
            } else {
                let violations: Vec<_> = clone_analysis
                    .clusters
                    .iter()
                    .filter(|c| c.avg_similarity < 0.0 || c.avg_similarity > 1.0)
                    .map(|c| format!("cluster{}={:.4}", c.id, c.avg_similarity))
                    .take(5)
                    .collect();
                Some(format!("Out of range: {}", violations.join(", ")))
            },
        });

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_suite_name() {
        let suite = AnalysisSuite;
        assert_eq!(suite.name(), "analysis");
    }

    #[test]
    fn dead_code_precision_all_correct() {
        let detected = vec!["a".to_string(), "b".to_string()];
        let tagged = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let p = dead_code_precision(&detected, &tagged);
        assert!((p - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_precision_none_correct() {
        let detected = vec!["x".to_string(), "y".to_string()];
        let tagged = vec!["a".to_string(), "b".to_string()];
        let p = dead_code_precision(&detected, &tagged);
        assert!((p - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_precision_empty() {
        let detected: Vec<String> = vec![];
        let tagged = vec!["a".to_string()];
        let p = dead_code_precision(&detected, &tagged);
        assert!((p - 0.0).abs() < f64::EPSILON);
    }
}
