use domain::error::{CodeGraphError, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CodeGraphConfig {
    pub index: Option<IndexConfig>,
    pub search: Option<SearchConfig>,
    pub watch: Option<WatchConfig>,
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
    fn invalid_toml_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".code-graph");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "not valid toml {{{{").unwrap();
        assert!(load_config(tmp.path()).is_err());
    }
}
