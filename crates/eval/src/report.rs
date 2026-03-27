use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SuiteResult {
    pub search: Option<SearchSuiteResult>,
    pub impact: Option<ImpactSuiteResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchSuiteResult {
    pub repos: usize,
    pub queries: usize,
    pub mrr: f64,
    pub precision_at_5: f64,
    pub precision_at_10: f64,
    pub mrr_target: f64,
    pub mrr_passed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactSuiteResult {
    pub repos: usize,
    pub scenarios: usize,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    pub precision_target: f64,
    pub precision_passed: bool,
}
