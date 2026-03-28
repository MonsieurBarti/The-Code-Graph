pub mod download;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("model download failed: {0}")]
    Download(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("model not found in cache: {0}")]
    ModelNotFound(String),
}

pub type Result<T> = std::result::Result<T, EmbeddingError>;
