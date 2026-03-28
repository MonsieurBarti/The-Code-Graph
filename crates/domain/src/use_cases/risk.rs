use crate::analysis::risk::{
    aggregate_file_scores, compute_coupling_scores, compute_criticality_scores, compute_risk_stats,
    compute_sensitivity, compute_test_gaps, score_symbols,
};
use crate::error::{CodeGraphError, Result};
use crate::model::*;
use crate::ports::GraphStore;

pub struct RiskUseCase<S> {
    store: S,
}

impl<S: GraphStore> RiskUseCase<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Full risk analysis: compute all factors, score symbols, aggregate files.
    pub fn analyze(&self, config: &RiskConfig) -> Result<RiskAnalysis> {
        let symbols = self.store.all_symbols()?;
        let edges = self.store.all_edges()?;

        let criticality = compute_criticality_scores(&symbols, &edges);
        let coupling = compute_coupling_scores(&symbols, &edges);
        let test_gaps = compute_test_gaps(&symbols, &edges);
        let sensitivity = compute_sensitivity(&symbols, &config.security_patterns);

        let symbol_scores = score_symbols(
            &symbols,
            &criticality,
            &coupling,
            &test_gaps,
            &sensitivity,
            &config.weights,
        );
        let file_scores = aggregate_file_scores(&symbol_scores, &symbols);
        let stats = compute_risk_stats(&symbol_scores, file_scores.len());

        Ok(RiskAnalysis {
            symbol_scores,
            file_scores,
            stats,
        })
    }

    /// Score a single symbol (loads full graph, filters to one result).
    pub fn score_symbol(&self, qualified_name: &str, config: &RiskConfig) -> Result<RiskScore> {
        let analysis = self.analyze(config)?;
        analysis
            .symbol_scores
            .into_iter()
            .find(|s| s.qualified_name == qualified_name)
            .ok_or_else(|| {
                CodeGraphError::Resolution(format!("symbol not found: {qualified_name}"))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryGraphStore;

    fn build_store() -> InMemoryGraphStore {
        let mut store = InMemoryGraphStore::new();
        // Symbol A: main function, calls B, untested, name has "auth"
        store.insert_symbol(SymbolNode {
            name: "auth_handler".into(),
            qualified_name: "src/auth.rs::auth_handler".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/auth.rs".into(),
                line_start: 1,
                line_end: 20,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        // Symbol B: utility function, tested
        store.insert_symbol(SymbolNode {
            name: "parse_input".into(),
            qualified_name: "src/util.rs::parse_input".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/util.rs".into(),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        // Edge: auth_handler calls parse_input
        store.insert_edge(Edge {
            kind: EdgeKind::Calls,
            source: "src/auth.rs::auth_handler".into(),
            target: "src/util.rs::parse_input".into(),
            metadata: None,
        });
        // Edge: parse_input is tested
        store.insert_edge(Edge {
            kind: EdgeKind::TestedBy,
            source: "test::test_parse".into(),
            target: "src/util.rs::parse_input".into(),
            metadata: None,
        });
        store
    }

    #[test]
    fn test_use_case_analyze() {
        let store = build_store();
        let uc = RiskUseCase::new(store);
        let analysis = uc.analyze(&RiskConfig::default()).unwrap();
        assert_eq!(analysis.symbol_scores.len(), 2);
        assert_eq!(analysis.file_scores.len(), 2);
        // auth_handler should have higher risk (untested + security sensitive)
        let auth_score = analysis
            .symbol_scores
            .iter()
            .find(|s| s.qualified_name == "src/auth.rs::auth_handler")
            .unwrap();
        let util_score = analysis
            .symbol_scores
            .iter()
            .find(|s| s.qualified_name == "src/util.rs::parse_input")
            .unwrap();
        assert!(auth_score.composite > util_score.composite);
        // auth_handler should have sensitivity = 1.0
        assert!((auth_score.factors.sensitivity - 1.0).abs() < f64::EPSILON);
        // parse_input should have test_gap = 0.0 (it's tested)
        assert!((util_score.factors.test_gap).abs() < f64::EPSILON);
    }

    #[test]
    fn test_use_case_score_symbol() {
        let store = build_store();
        let uc = RiskUseCase::new(store);
        let score = uc
            .score_symbol("src/auth.rs::auth_handler", &RiskConfig::default())
            .unwrap();
        assert_eq!(score.qualified_name, "src/auth.rs::auth_handler");
        assert!(score.composite > 0.0);
    }

    #[test]
    fn test_use_case_score_symbol_not_found() {
        let store = build_store();
        let uc = RiskUseCase::new(store);
        let result = uc.score_symbol("nonexistent::symbol", &RiskConfig::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_weight_normalization() {
        let store = build_store();
        let uc = RiskUseCase::new(store);
        // All weights = 1.0 should produce same relative ranking as defaults
        let config = RiskConfig {
            weights: RiskWeights {
                criticality: 1.0,
                coupling: 1.0,
                test_gap: 1.0,
                sensitivity: 1.0,
            },
            ..RiskConfig::default()
        };
        let analysis = uc.analyze(&config).unwrap();
        // Should still work — normalized internally to 0.25 each
        assert!(!analysis.symbol_scores.is_empty());
        for score in &analysis.symbol_scores {
            assert!(score.composite >= 0.0 && score.composite <= 1.0);
        }
    }
}
