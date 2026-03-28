use crate::{EmbeddingError, Result};
use std::path::Path;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

/// Load a tokenizer from file, configured for batch inference.
///
/// Truncation is set to a max of 256 tokens; padding is set to the
/// longest sequence in each batch.
pub fn load_tokenizer(path: &Path) -> Result<Tokenizer> {
    let mut tokenizer = Tokenizer::from_file(path)
        .map_err(|e| EmbeddingError::Tokenizer(format!("tokenizer load failed: {e}")))?;

    tokenizer
        .with_truncation(Some(TruncationParams {
            max_length: 256,
            ..Default::default()
        }))
        .map_err(|e| EmbeddingError::Tokenizer(format!("truncation config failed: {e}")))?;

    tokenizer.with_padding(Some(PaddingParams {
        strategy: PaddingStrategy::BatchLongest,
        ..Default::default()
    }));

    Ok(tokenizer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_tokenizer_fails_on_missing_file() {
        let result = load_tokenizer(Path::new("/nonexistent/tokenizer.json"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("tokenizer load failed"),
            "unexpected error: {err}"
        );
    }
}
