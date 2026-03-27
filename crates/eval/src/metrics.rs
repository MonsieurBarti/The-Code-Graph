use std::collections::HashSet;

/// Mean Reciprocal Rank across multiple queries.
/// `ranked_results`: for each query, the ranked list of qualified names returned.
/// `ground_truth`: for each query, the set of correct qualified names.
pub fn mrr(ranked_results: &[Vec<String>], ground_truth: &[Vec<String>]) -> f64 {
    if ranked_results.is_empty() {
        return 0.0;
    }
    let sum: f64 = ranked_results
        .iter()
        .zip(ground_truth.iter())
        .map(|(ranked, truth)| {
            let truth_set: HashSet<&str> = truth.iter().map(|s| s.as_str()).collect();
            ranked
                .iter()
                .enumerate()
                .find(|(_, name)| truth_set.contains(name.as_str()))
                .map(|(i, _)| 1.0 / (i as f64 + 1.0))
                .unwrap_or(0.0)
        })
        .sum();
    sum / ranked_results.len() as f64
}

/// Precision at K for a single query.
fn precision_at_k_single(ranked: &[String], truth: &[String], k: usize) -> f64 {
    let truth_set: HashSet<&str> = truth.iter().map(|s| s.as_str()).collect();
    let effective_k = k.min(ranked.len());
    if effective_k == 0 {
        return 0.0;
    }
    let relevant = ranked[..effective_k]
        .iter()
        .filter(|name| truth_set.contains(name.as_str()))
        .count();
    relevant as f64 / effective_k as f64
}

/// Precision at K — average across queries.
pub fn precision_at_k(
    ranked_results: &[Vec<String>],
    ground_truth: &[Vec<String>],
    k: usize,
) -> f64 {
    if ranked_results.is_empty() {
        return 0.0;
    }
    let sum: f64 = ranked_results
        .iter()
        .zip(ground_truth.iter())
        .map(|(ranked, truth)| precision_at_k_single(ranked, truth, k))
        .sum();
    sum / ranked_results.len() as f64
}

/// Blast radius precision: |predicted ∩ actual| / |predicted|
pub fn blast_precision(predicted: &[String], actual: &[String]) -> f64 {
    if predicted.is_empty() {
        return 0.0;
    }
    let actual_set: HashSet<&str> = actual.iter().map(|s| s.as_str()).collect();
    let intersection = predicted
        .iter()
        .filter(|p| actual_set.contains(p.as_str()))
        .count();
    intersection as f64 / predicted.len() as f64
}

/// Blast radius recall: |predicted ∩ actual| / |actual|
pub fn blast_recall(predicted: &[String], actual: &[String]) -> f64 {
    if actual.is_empty() {
        return 0.0;
    }
    let actual_set: HashSet<&str> = actual.iter().map(|s| s.as_str()).collect();
    let intersection = predicted
        .iter()
        .filter(|p| actual_set.contains(p.as_str()))
        .count();
    intersection as f64 / actual.len() as f64
}

/// Harmonic mean of precision and recall.
pub fn f1(precision: f64, recall: f64) -> f64 {
    if precision + recall == 0.0 {
        return 0.0;
    }
    2.0 * precision * recall / (precision + recall)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(val: &str) -> String {
        val.to_string()
    }

    // ── MRR tests ──────────────────────────────────────────────

    #[test]
    fn mrr_perfect_ranking() {
        // All first results are correct → MRR = 1.0
        let ranked = vec![vec![s("a"), s("b")], vec![s("c"), s("d")]];
        let truth = vec![vec![s("a")], vec![s("c")]];
        assert!((mrr(&ranked, &truth) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mrr_second_position() {
        // First correct at position 2 → reciprocal rank = 0.5
        let ranked = vec![vec![s("x"), s("a"), s("b")]];
        let truth = vec![vec![s("a")]];
        assert!((mrr(&ranked, &truth) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn mrr_no_match() {
        // No correct result found → reciprocal rank = 0.0
        let ranked = vec![vec![s("x"), s("y")]];
        let truth = vec![vec![s("a")]];
        assert!((mrr(&ranked, &truth) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mrr_mixed() {
        // Query 1: correct at position 1 → 1.0
        // Query 2: correct at position 3 → 1/3
        // Average: (1.0 + 1/3) / 2 = 2/3
        let ranked = vec![vec![s("a"), s("b"), s("c")], vec![s("x"), s("y"), s("a")]];
        let truth = vec![vec![s("a")], vec![s("a")]];
        let expected = (1.0 + 1.0 / 3.0) / 2.0;
        assert!((mrr(&ranked, &truth) - expected).abs() < 1e-10);
    }

    #[test]
    fn mrr_empty_queries() {
        let ranked: Vec<Vec<String>> = vec![];
        let truth: Vec<Vec<String>> = vec![];
        assert!((mrr(&ranked, &truth) - 0.0).abs() < f64::EPSILON);
    }

    // ── Precision@K tests ──────────────────────────────────────

    #[test]
    fn precision_at_k_all_relevant() {
        // All top-k are relevant → 1.0
        let ranked = vec![vec![s("a"), s("b"), s("c")]];
        let truth = vec![vec![s("a"), s("b"), s("c")]];
        assert!((precision_at_k(&ranked, &truth, 3) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn precision_at_k_none_relevant() {
        // No top-k relevant → 0.0
        let ranked = vec![vec![s("x"), s("y"), s("z")]];
        let truth = vec![vec![s("a"), s("b"), s("c")]];
        assert!((precision_at_k(&ranked, &truth, 3) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn precision_at_k_partial() {
        // 3 of 5 relevant → 0.6
        let ranked = vec![vec![s("a"), s("x"), s("b"), s("y"), s("c")]];
        let truth = vec![vec![s("a"), s("b"), s("c")]];
        assert!((precision_at_k(&ranked, &truth, 5) - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn precision_at_k_fewer_results_than_k() {
        // 3 results, k=5 → use actual count (3). All 3 relevant → 1.0
        let ranked = vec![vec![s("a"), s("b"), s("c")]];
        let truth = vec![vec![s("a"), s("b"), s("c"), s("d"), s("e")]];
        assert!((precision_at_k(&ranked, &truth, 5) - 1.0).abs() < f64::EPSILON);
    }

    // ── Blast radius precision/recall tests ────────────────────

    #[test]
    fn blast_precision_perfect() {
        let predicted = vec![s("a"), s("b")];
        let actual = vec![s("a"), s("b")];
        assert!((blast_precision(&predicted, &actual) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn blast_precision_empty_predicted() {
        let predicted: Vec<String> = vec![];
        let actual = vec![s("a")];
        assert!((blast_precision(&predicted, &actual) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn blast_recall_perfect() {
        let predicted = vec![s("a"), s("b")];
        let actual = vec![s("a"), s("b")];
        assert!((blast_recall(&predicted, &actual) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn blast_recall_empty_actual() {
        let predicted = vec![s("a")];
        let actual: Vec<String> = vec![];
        assert!((blast_recall(&predicted, &actual) - 0.0).abs() < f64::EPSILON);
    }

    // ── F1 tests ───────────────────────────────────────────────

    #[test]
    fn f1_balanced() {
        // precision = recall → F1 = precision
        assert!((f1(0.75, 0.75) - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn f1_zero_both() {
        assert!((f1(0.0, 0.0) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn f1_typical() {
        // precision=0.8, recall=0.6 → F1 = 2*0.8*0.6 / (0.8+0.6) ≈ 0.6857142857…
        let expected = 2.0 * 0.8 * 0.6 / (0.8 + 0.6);
        assert!((f1(0.8, 0.6) - expected).abs() < 1e-10);
    }
}
