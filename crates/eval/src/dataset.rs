use serde::Deserialize;
use std::path::{Path, PathBuf};

use domain::error::{CodeGraphError, Result};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SuiteManifest {
    pub suite: SuiteInfo,
    pub repos: Vec<ManifestRepo>,
}

#[derive(Debug, Deserialize)]
pub struct SuiteInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestRepo {
    pub name: String,
    pub url: String,
    pub revision: String,
    pub languages: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQueryFile {
    pub queries: Vec<SearchQuery>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub repo: String,
    pub query: String,
    pub expected: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImpactQueryFile {
    pub scenarios: Vec<ImpactScenario>,
}

#[derive(Debug, Deserialize)]
pub struct ImpactScenario {
    pub repo: String,
    pub description: String,
    pub target: String,
    pub depth: usize,
    pub confidence: String,
    pub expected_affected: Vec<String>,
}

// ---------------------------------------------------------------------------
// Cache management
// ---------------------------------------------------------------------------

pub fn eval_cache_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(xdg).join("code-graph-eval"));
    }
    let home =
        std::env::var("HOME").map_err(|_| CodeGraphError::Other("HOME not set".into()))?;
    Ok(PathBuf::from(home).join(".cache").join("code-graph-eval"))
}

pub fn repo_cache_path(repo: &ManifestRepo) -> Result<PathBuf> {
    Ok(eval_cache_dir()?.join(&repo.name).join(&repo.revision))
}

pub fn validate_cache(repo: &ManifestRepo) -> Result<bool> {
    let path = repo_cache_path(repo)?;
    if !path.exists() {
        return Ok(false);
    }
    let marker = path.join(".revision");
    if !marker.exists() {
        return Ok(false);
    }
    let stored = std::fs::read_to_string(&marker)
        .map_err(|e| CodeGraphError::Other(format!("read .revision: {e}")))?;
    Ok(stored.trim() == repo.revision)
}

pub fn clone_or_cache(repo: &ManifestRepo, no_cache: bool) -> Result<PathBuf> {
    let cache_path = repo_cache_path(repo)?;

    if no_cache {
        if cache_path.exists() {
            std::fs::remove_dir_all(&cache_path)
                .map_err(|e| CodeGraphError::Other(format!("remove cache: {e}")))?;
        }
    } else if validate_cache(repo)? {
        tracing::info!(repo = %repo.name, "Using cached clone");
        return Ok(cache_path);
    }

    tracing::info!(repo = %repo.name, revision = %repo.revision, "Cloning");
    std::fs::create_dir_all(&cache_path)
        .map_err(|e| CodeGraphError::Other(format!("mkdir: {e}")))?;

    let output = std::process::Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--branch",
            &repo.revision,
            &repo.url,
        ])
        .arg(&cache_path)
        .output()
        .map_err(|e| CodeGraphError::Other(format!("git clone failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CodeGraphError::Other(format!(
            "git clone failed: {stderr}"
        )));
    }

    std::fs::write(cache_path.join(".revision"), &repo.revision)
        .map_err(|e| CodeGraphError::Other(format!("write .revision: {e}")))?;

    Ok(cache_path)
}

pub fn clear_cache(repo: &ManifestRepo) -> Result<()> {
    let path = repo_cache_path(repo)?;
    if path.exists() {
        std::fs::remove_dir_all(&path)
            .map_err(|e| CodeGraphError::Other(format!("clear cache: {e}")))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Manifest / query parsing
// ---------------------------------------------------------------------------

pub fn parse_manifest(path: &Path) -> Result<SuiteManifest> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| CodeGraphError::Other(format!("Failed to read manifest: {e}")))?;
    serde_json::from_str(&content)
        .map_err(|e| CodeGraphError::Other(format!("Invalid manifest JSON: {e}")))
}

pub fn parse_search_queries(path: &Path) -> Result<Vec<SearchQuery>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| CodeGraphError::Other(format!("Failed to read queries: {e}")))?;
    let file: SearchQueryFile = serde_json::from_str(&content)
        .map_err(|e| CodeGraphError::Other(format!("Invalid query JSON: {e}")))?;
    Ok(file.queries)
}

pub fn parse_impact_queries(path: &Path) -> Result<Vec<ImpactScenario>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| CodeGraphError::Other(format!("Failed to read scenarios: {e}")))?;
    let file: ImpactQueryFile = serde_json::from_str(&content)
        .map_err(|e| CodeGraphError::Other(format!("Invalid scenario JSON: {e}")))?;
    Ok(file.scenarios)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_repo() -> ManifestRepo {
        ManifestRepo {
            name: "test-repo".into(),
            url: "https://github.com/example/test-repo.git".into(),
            revision: "abc123".into(),
            languages: vec!["rust".into()],
        }
    }

    const MANIFEST_JSON: &str = r#"{
        "suite": {
            "name": "search-v1",
            "description": "Search evaluation suite"
        },
        "repos": [
            {
                "name": "sample-repo",
                "url": "https://github.com/example/sample.git",
                "revision": "v1.0.0",
                "languages": ["rust", "python"]
            }
        ]
    }"#;

    const SEARCH_QUERIES_JSON: &str = r#"{
        "queries": [
            {
                "repo": "sample-repo",
                "query": "find all error handlers",
                "expected": ["src/error.rs", "src/handler.rs"]
            }
        ]
    }"#;

    const IMPACT_QUERIES_JSON: &str = r#"{
        "scenarios": [
            {
                "repo": "sample-repo",
                "description": "Change error type",
                "target": "src/error.rs::AppError",
                "depth": 3,
                "confidence": "high",
                "expected_affected": ["src/handler.rs", "src/main.rs"]
            }
        ]
    }"#;

    // -- Manifest parsing ---------------------------------------------------

    #[test]
    fn parse_search_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        std::fs::write(&path, MANIFEST_JSON).unwrap();

        let manifest = parse_manifest(&path).unwrap();
        assert_eq!(manifest.suite.name, "search-v1");
        assert_eq!(manifest.suite.description, "Search evaluation suite");
        assert_eq!(manifest.repos.len(), 1);
        assert_eq!(manifest.repos[0].name, "sample-repo");
        assert_eq!(manifest.repos[0].revision, "v1.0.0");
        assert_eq!(manifest.repos[0].languages, vec!["rust", "python"]);
    }

    #[test]
    fn parse_search_manifest_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{ not valid json }").unwrap();

        let err = parse_manifest(&path).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("Invalid manifest JSON"),
            "expected clear error, got: {msg}"
        );
    }

    // -- Query parsing ------------------------------------------------------

    #[test]
    fn parse_search_queries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("search.json");
        std::fs::write(&path, SEARCH_QUERIES_JSON).unwrap();

        let queries = super::parse_search_queries(&path).unwrap();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].repo, "sample-repo");
        assert_eq!(queries[0].query, "find all error handlers");
        assert_eq!(
            queries[0].expected,
            vec!["src/error.rs", "src/handler.rs"]
        );
    }

    #[test]
    fn parse_impact_queries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("impact.json");
        std::fs::write(&path, IMPACT_QUERIES_JSON).unwrap();

        let scenarios = super::parse_impact_queries(&path).unwrap();
        assert_eq!(scenarios.len(), 1);
        assert_eq!(scenarios[0].repo, "sample-repo");
        assert_eq!(scenarios[0].description, "Change error type");
        assert_eq!(scenarios[0].target, "src/error.rs::AppError");
        assert_eq!(scenarios[0].depth, 3);
        assert_eq!(scenarios[0].confidence, "high");
        assert_eq!(
            scenarios[0].expected_affected,
            vec!["src/handler.rs", "src/main.rs"]
        );
    }

    // -- Cache dir resolution -----------------------------------------------

    #[test]
    fn cache_dir_resolution() {
        let repo = test_repo();
        let path = repo_cache_path(&repo).unwrap();
        // Must end with <name>/<revision>
        assert!(
            path.ends_with("test-repo/abc123"),
            "unexpected cache path: {path:?}"
        );
    }

    #[test]
    fn cache_dir_respects_xdg() {
        let dir = tempfile::tempdir().unwrap();
        let xdg_path = dir.path().to_str().unwrap().to_string();

        // Temporarily set XDG_CACHE_HOME — safe because this test only reads
        // env, doesn't write to the filesystem.
        unsafe { std::env::set_var("XDG_CACHE_HOME", &xdg_path) };
        let result = eval_cache_dir().unwrap();
        unsafe { std::env::remove_var("XDG_CACHE_HOME") };

        assert_eq!(
            result,
            PathBuf::from(&xdg_path).join("code-graph-eval"),
            "XDG_CACHE_HOME should be respected"
        );
    }

    // -- Cache validation ---------------------------------------------------

    #[test]
    fn validate_cache_missing_dir() {
        // Point HOME at a tempdir so repo_cache_path resolves somewhere
        // that doesn't exist.
        let dir = tempfile::tempdir().unwrap();
        let fake_home = dir.path().to_str().unwrap().to_string();

        // Use XDG to control where the cache dir resolves.
        unsafe { std::env::set_var("XDG_CACHE_HOME", &fake_home) };
        let repo = test_repo();
        let valid = validate_cache(&repo).unwrap();
        unsafe { std::env::remove_var("XDG_CACHE_HOME") };

        assert!(!valid, "cache should be invalid when directory is missing");
    }

    #[test]
    fn validate_cache_wrong_revision() {
        let dir = tempfile::tempdir().unwrap();
        let cache_root = dir.path().to_str().unwrap().to_string();
        let repo = test_repo();

        // Create the cache directory with a wrong revision marker.
        let cache_dir = dir.path().join("code-graph-eval").join(&repo.name).join(&repo.revision);
        std::fs::create_dir_all(&cache_dir).unwrap();
        let mut f = std::fs::File::create(cache_dir.join(".revision")).unwrap();
        f.write_all(b"wrong-revision").unwrap();

        unsafe { std::env::set_var("XDG_CACHE_HOME", &cache_root) };
        let valid = validate_cache(&repo).unwrap();
        unsafe { std::env::remove_var("XDG_CACHE_HOME") };

        assert!(!valid, "cache should be invalid when revision doesn't match");
    }

    #[test]
    fn validate_cache_valid() {
        let dir = tempfile::tempdir().unwrap();
        let cache_root = dir.path().to_str().unwrap().to_string();
        let repo = test_repo();

        let cache_dir = dir.path().join("code-graph-eval").join(&repo.name).join(&repo.revision);
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join(".revision"), &repo.revision).unwrap();

        unsafe { std::env::set_var("XDG_CACHE_HOME", &cache_root) };
        let valid = validate_cache(&repo).unwrap();
        unsafe { std::env::remove_var("XDG_CACHE_HOME") };

        assert!(valid, "cache should be valid when dir exists and revision matches");
    }

    // -- Clear cache --------------------------------------------------------

    #[test]
    fn clear_cache_removes_dir() {
        let dir = tempfile::tempdir().unwrap();
        let cache_root = dir.path().to_str().unwrap().to_string();
        let repo = test_repo();

        let cache_dir = dir.path().join("code-graph-eval").join(&repo.name).join(&repo.revision);
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join(".revision"), &repo.revision).unwrap();
        assert!(cache_dir.exists(), "setup: cache dir should exist");

        unsafe { std::env::set_var("XDG_CACHE_HOME", &cache_root) };
        clear_cache(&repo).unwrap();
        unsafe { std::env::remove_var("XDG_CACHE_HOME") };

        assert!(!cache_dir.exists(), "cache dir should be removed after clear");
    }
}
