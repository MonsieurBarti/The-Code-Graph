use crate::{EmbeddingError, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::info;

/// Maximum download size (500 MB) to prevent disk exhaustion from malicious or oversized files.
const MAX_DOWNLOAD_BYTES: u64 = 500 * 1024 * 1024;

/// Paths to downloaded model files
pub struct ModelFiles {
    pub model_path: PathBuf,
    pub tokenizer_path: PathBuf,
}

/// Get the cache directory for a model.
/// Uses $XDG_CACHE_HOME/code-graph/models/<model>/ or ~/.cache/code-graph/models/<model>/
pub fn model_cache_dir(model_name: &str) -> PathBuf {
    let base = match std::env::var("XDG_CACHE_HOME") {
        Ok(xdg) if !xdg.is_empty() => PathBuf::from(xdg),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".cache")
        }
    };
    base.join("code-graph").join("models").join(model_name)
}

/// Validate that a model name contains only safe characters (alphanumeric, dots, hyphens, underscores).
/// Prevents path traversal in the HuggingFace URL template.
fn validate_model_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(EmbeddingError::Download(
            "model name cannot be empty".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(EmbeddingError::Download(format!(
            "invalid model name '{name}': only alphanumeric, '.', '-', '_' allowed"
        )));
    }
    Ok(())
}

/// Ensure model files are available, downloading if needed.
pub fn ensure_model(model_name: &str) -> Result<ModelFiles> {
    validate_model_name(model_name)?;
    let dir = model_cache_dir(model_name);
    let model_path = dir.join("model.onnx");
    let tokenizer_path = dir.join("tokenizer.json");

    if model_path.exists() && tokenizer_path.exists() {
        info!("Model {model_name} found in cache at {}", dir.display());
        return Ok(ModelFiles {
            model_path,
            tokenizer_path,
        });
    }

    std::fs::create_dir_all(&dir)?;

    // Download model ONNX file
    let model_url = format!(
        "https://huggingface.co/sentence-transformers/{model_name}/resolve/main/onnx/model.onnx"
    );
    download_file(&model_url, &model_path)?;

    // Download tokenizer JSON
    let tokenizer_url = format!(
        "https://huggingface.co/sentence-transformers/{model_name}/resolve/main/tokenizer.json"
    );
    download_file(&tokenizer_url, &tokenizer_path)?;

    info!("Downloaded model {model_name} to {}", dir.display());
    Ok(ModelFiles {
        model_path,
        tokenizer_path,
    })
}

/// Download a file atomically (write to .tmp, then rename).
/// Enforces a size limit of [`MAX_DOWNLOAD_BYTES`] to prevent disk exhaustion.
fn download_file(url: &str, dest: &Path) -> Result<()> {
    let tmp_path = dest.with_extension("tmp");

    // Clean up any leftover partial downloads
    let _ = std::fs::remove_file(&tmp_path);

    info!("Downloading {}...", url);

    let resp = ureq::get(url)
        .call()
        .map_err(|e| EmbeddingError::Download(format!("{url}: {e}")))?;

    // Limit download size to prevent disk exhaustion from oversized files
    let mut reader = resp.into_body().into_reader().take(MAX_DOWNLOAD_BYTES);
    let mut file = std::fs::File::create(&tmp_path)?;
    let bytes_written = std::io::copy(&mut reader, &mut file)?;

    if bytes_written >= MAX_DOWNLOAD_BYTES {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(EmbeddingError::Download(format!(
            "{url}: file exceeds maximum size of {MAX_DOWNLOAD_BYTES} bytes"
        )));
    }

    // Atomic rename
    std::fs::rename(&tmp_path, dest)?;
    Ok(())
}

/// Clean up any partial download files in a model directory.
pub fn cleanup_partial_downloads(model_name: &str) -> Result<()> {
    let dir = model_cache_dir(model_name);
    if dir.exists() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) == Some("tmp") {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: Tests that modify XDG_CACHE_HOME must use a serialization mutex
    // to prevent env var races. See existing pattern in crates/eval/.
    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    unsafe fn set_env(key: &str, val: &str) {
        unsafe { std::env::set_var(key, val) };
    }

    unsafe fn remove_env(key: &str) {
        unsafe { std::env::remove_var(key) };
    }

    fn restore_xdg(old: Option<String>) {
        unsafe {
            match old {
                Some(v) => set_env("XDG_CACHE_HOME", &v),
                None => remove_env("XDG_CACHE_HOME"),
            }
        }
    }

    #[test]
    fn validate_model_name_accepts_valid() {
        assert!(validate_model_name("all-MiniLM-L6-v2").is_ok());
        assert!(validate_model_name("model_name.v1").is_ok());
    }

    #[test]
    fn validate_model_name_rejects_path_traversal() {
        assert!(validate_model_name("../../../evil").is_err());
        assert!(validate_model_name("model/subpath").is_err());
        assert!(validate_model_name("").is_err());
    }

    #[test]
    fn model_dir_uses_xdg_cache() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old = std::env::var("XDG_CACHE_HOME").ok();
        unsafe { set_env("XDG_CACHE_HOME", "/custom/cache") };
        let dir = model_cache_dir("test-model");
        restore_xdg(old);
        assert_eq!(
            dir,
            PathBuf::from("/custom/cache/code-graph/models/test-model")
        );
    }

    #[test]
    fn model_dir_falls_back_to_home_cache() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_xdg = std::env::var("XDG_CACHE_HOME").ok();
        let home = std::env::var("HOME").unwrap();
        unsafe { remove_env("XDG_CACHE_HOME") };
        let dir = model_cache_dir("test-model");
        restore_xdg(old_xdg);
        assert_eq!(
            dir,
            PathBuf::from(format!("{home}/.cache/code-graph/models/test-model"))
        );
    }

    #[test]
    fn cached_model_skips_download() {
        let tmp = tempfile::tempdir().unwrap();
        let _guard = ENV_LOCK.lock().unwrap();
        let old = std::env::var("XDG_CACHE_HOME").ok();
        unsafe { set_env("XDG_CACHE_HOME", tmp.path().to_str().unwrap()) };

        let model_dir = tmp.path().join("code-graph/models/test-model");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.onnx"), b"fake").unwrap();
        std::fs::write(model_dir.join("tokenizer.json"), b"fake").unwrap();

        let result = ensure_model("test-model");
        restore_xdg(old);

        assert!(result.is_ok());
        let files = result.unwrap();
        assert!(files.model_path.exists());
        assert!(files.tokenizer_path.exists());
    }

    #[test]
    fn cleanup_removes_tmp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let _guard = ENV_LOCK.lock().unwrap();
        let old = std::env::var("XDG_CACHE_HOME").ok();
        unsafe { set_env("XDG_CACHE_HOME", tmp.path().to_str().unwrap()) };

        let model_dir = tmp.path().join("code-graph/models/test-cleanup");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.tmp"), b"partial").unwrap();
        std::fs::write(model_dir.join("model.onnx"), b"complete").unwrap();

        cleanup_partial_downloads("test-cleanup").unwrap();
        restore_xdg(old);

        assert!(!model_dir.join("model.tmp").exists());
        assert!(model_dir.join("model.onnx").exists());
    }
}
