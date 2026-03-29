pub mod adapters;
pub mod dataset;
pub mod metrics;
pub mod report;
pub mod runner;
pub mod suites;

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
    let search_result = match config.suite {
        Suite::Search | Suite::All => Some(runner::run_search_suite(config)?),
        _ => None,
    };
    let impact_result = match config.suite {
        Suite::Impact | Suite::All => Some(runner::run_impact_suite(config)?),
        _ => None,
    };
    Ok(SuiteResult {
        search: search_result,
        impact: impact_result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Resolve the project-root `eval/suites/` directory from CARGO_MANIFEST_DIR.
    fn suites_dir() -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("eval")
            .join("suites")
    }

    #[test]
    fn eval_crate_is_workspace_member() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root_cargo = manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("Cargo.toml");
        let content =
            std::fs::read_to_string(&root_cargo).expect("root Cargo.toml should be readable");
        let doc: toml::Value =
            toml::from_str(&content).expect("root Cargo.toml should be valid TOML");
        let members = doc["workspace"]["members"]
            .as_array()
            .expect("workspace.members should be an array");
        assert_eq!(members.len(), 9, "workspace should have 9 members");
        let has_eval = members.iter().any(|m| m.as_str() == Some("crates/eval"));
        assert!(has_eval, "workspace members should include crates/eval");
    }

    #[test]
    fn suite_config_search() {
        let config = SuiteConfig {
            suite: Suite::Search,
            no_cache: false,
            suites_dir: PathBuf::from("/tmp/suites"),
            search_limit: 10,
        };
        assert!(matches!(config.suite, Suite::Search));
        assert!(!config.no_cache);
        assert_eq!(config.suites_dir, PathBuf::from("/tmp/suites"));
        assert_eq!(config.search_limit, 10);
    }

    #[test]
    fn suite_config_impact() {
        let config = SuiteConfig {
            suite: Suite::Impact,
            no_cache: true,
            suites_dir: PathBuf::from("/tmp/suites"),
            search_limit: 5,
        };
        assert!(matches!(config.suite, Suite::Impact));
        assert!(config.no_cache);
        assert_eq!(config.suites_dir, PathBuf::from("/tmp/suites"));
        assert_eq!(config.search_limit, 5);
    }

    #[test]
    fn manifest_files_are_valid_json() {
        let base = suites_dir();
        let search_manifest = base.join("search").join("manifest.json");
        let impact_manifest = base.join("impact").join("manifest.json");

        let sm = dataset::parse_manifest(&search_manifest).expect("search manifest should parse");
        assert!(!sm.repos.is_empty(), "search manifest should have repos");

        let im = dataset::parse_manifest(&impact_manifest).expect("impact manifest should parse");
        assert!(!im.repos.is_empty(), "impact manifest should have repos");
    }

    #[test]
    fn search_query_files_are_valid_json() {
        let dir = suites_dir().join("search").join("queries");
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .expect("search queries dir should be readable")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
            .collect();
        assert!(
            !entries.is_empty(),
            "there should be at least one search query file"
        );
        for entry in entries {
            let path = entry.path();
            dataset::parse_search_queries(&path)
                .unwrap_or_else(|e| panic!("{} should parse: {e}", path.display()));
        }
    }

    #[test]
    fn impact_query_files_are_valid_json() {
        let dir = suites_dir().join("impact").join("queries");
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .expect("impact queries dir should be readable")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
            .collect();
        assert!(
            !entries.is_empty(),
            "there should be at least one impact query file"
        );
        for entry in entries {
            let path = entry.path();
            dataset::parse_impact_queries(&path)
                .unwrap_or_else(|e| panic!("{} should parse: {e}", path.display()));
        }
    }

    #[test]
    fn search_query_count_meets_minimum() {
        let dir = suites_dir().join("search").join("queries");
        let total: usize = std::fs::read_dir(&dir)
            .expect("search queries dir should be readable")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
            .map(|e| {
                dataset::parse_search_queries(&e.path())
                    .unwrap_or_else(|err| panic!("parse failed: {err}"))
                    .len()
            })
            .sum();
        assert!(total >= 50, "expected >= 50 search queries, found {total}");
    }

    #[test]
    fn impact_scenario_count_meets_minimum() {
        let dir = suites_dir().join("impact").join("queries");
        let total: usize = std::fs::read_dir(&dir)
            .expect("impact queries dir should be readable")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
            .map(|e| {
                dataset::parse_impact_queries(&e.path())
                    .unwrap_or_else(|err| panic!("parse failed: {err}"))
                    .len()
            })
            .sum();
        assert!(
            total >= 20,
            "expected >= 20 impact scenarios, found {total}"
        );
    }
}
