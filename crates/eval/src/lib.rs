pub mod metrics;
pub mod dataset;
pub mod runner;
pub mod report;
pub mod adapters;

use domain::error::Result;
use report::SuiteResult;

/// Which evaluation suite to run.
#[derive(Debug, Clone)]
pub enum Suite {
    Search,
    Impact,
    All,
}

/// Configuration for an eval run.
#[derive(Debug, Clone)]
pub struct SuiteConfig {
    pub suite: Suite,
    pub no_cache: bool,
    pub suites_dir: std::path::PathBuf,
    pub search_limit: usize,
}

/// Run the evaluation suite. Entry point called by CLI.
pub fn run_suite(config: &SuiteConfig) -> Result<SuiteResult> {
    Err(domain::error::CodeGraphError::Other(
        "eval: not yet implemented".into(),
    ))
}
