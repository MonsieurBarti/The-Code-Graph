use domain::error::{CodeGraphError, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CodeGraphConfig {
    pub index: Option<IndexConfig>,
    pub search: Option<SearchConfig>,
    pub watch: Option<WatchConfig>,
    pub flows: Option<FlowsConfig>,
    pub risk: Option<RiskCliConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct IndexConfig {
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchConfig {
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WatchConfig {
    pub debounce_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FlowsConfig {
    pub extra_entry_points: Option<Vec<String>>,
    pub excluded_entry_points: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RiskCliConfig {
    pub weight_criticality: Option<f64>,
    pub weight_coupling: Option<f64>,
    pub weight_test_gap: Option<f64>,
    pub weight_sensitivity: Option<f64>,
    pub extra_security_patterns: Option<Vec<String>>,
    pub excluded_security_patterns: Option<Vec<String>>,
}

pub fn load_config(project_root: &Path) -> Result<CodeGraphConfig> {
    let config_path = project_root.join(".code-graph").join("config.toml");
    if !config_path.exists() {
        return Ok(CodeGraphConfig::default());
    }
    let content =
        std::fs::read_to_string(&config_path).map_err(|e| CodeGraphError::FileSystem {
            path: config_path.clone(),
            source: e,
        })?;
    toml::from_str(&content).map_err(|e| CodeGraphError::Other(format!("invalid config: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_returns_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert!(config.index.is_none());
        assert!(config.search.is_none());
    }

    #[test]
    fn valid_config_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".code-graph");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
[index]
exclude = ["target", "node_modules"]

[search]
max_results = 50
"#,
        )
        .unwrap();
        let config = load_config(tmp.path()).unwrap();
        let index = config.index.unwrap();
        assert_eq!(index.exclude.unwrap(), vec!["target", "node_modules"]);
        let search = config.search.unwrap();
        assert_eq!(search.max_results.unwrap(), 50);
    }

    #[test]
    fn flows_config_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".code-graph");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
[flows]
extra_entry_points = ["src/custom.rs::handler"]
excluded_entry_points = ["src/test_helper.rs::setup"]
"#,
        )
        .unwrap();
        let config = load_config(tmp.path()).unwrap();
        let flows = config.flows.unwrap();
        assert_eq!(
            flows.extra_entry_points.unwrap(),
            vec!["src/custom.rs::handler"]
        );
        assert_eq!(
            flows.excluded_entry_points.unwrap(),
            vec!["src/test_helper.rs::setup"]
        );
    }

    #[test]
    fn invalid_toml_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".code-graph");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "not valid toml {{{{").unwrap();
        assert!(load_config(tmp.path()).is_err());
    }

    #[test]
    fn risk_config_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".code-graph");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
[risk]
weight_criticality = 0.40
weight_coupling = 0.20
weight_test_gap = 0.20
weight_sensitivity = 0.20
extra_security_patterns = ["unsafe", "inject"]
excluded_security_patterns = ["hash"]
"#,
        )
        .unwrap();
        let config = load_config(tmp.path()).unwrap();
        let risk = config.risk.unwrap();
        assert!((risk.weight_criticality.unwrap() - 0.40).abs() < f64::EPSILON);
        assert!((risk.weight_coupling.unwrap() - 0.20).abs() < f64::EPSILON);
        assert_eq!(
            risk.extra_security_patterns.unwrap(),
            vec!["unsafe", "inject"]
        );
        assert_eq!(risk.excluded_security_patterns.unwrap(), vec!["hash"]);
    }
}
