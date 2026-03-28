use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Validation error: {field} — {message}")]
    Validation { field: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
    pub tls: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { host: "127.0.0.1".into(), port: 8080, workers: 4, tls: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub idle_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub log_level: String,
    pub features: HashMap<String, bool>,
}

impl AppConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&content).map_err(|e| ConfigError::Parse(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        let url = env::var("DATABASE_URL").unwrap_or_default();
        Ok(Self {
            server: ServerConfig::default(),
            database: DatabaseConfig { url, max_connections: 10, idle_timeout_secs: 30 },
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
            features: HashMap::new(),
        })
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.database.url.is_empty() {
            return Err(ConfigError::Validation { field: "database.url".into(), message: "must not be empty".into() });
        }
        if self.server.port == 0 {
            return Err(ConfigError::Validation { field: "server.port".into(), message: "must be > 0".into() });
        }
        Ok(())
    }
}
