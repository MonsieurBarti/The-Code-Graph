pub mod download;
pub mod tokenizer;

use std::path::Path;
use std::sync::Mutex;

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use thiserror::Error;
use tokenizers::Tokenizer;
use tracing::debug;

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("model download failed: {0}")]
    Download(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("model not found in cache: {0}")]
    ModelNotFound(String),
    #[error("tokenizer error: {0}")]
    Tokenizer(String),
    #[error("inference error: {0}")]
    Inference(String),
}

pub type Result<T> = std::result::Result<T, EmbeddingError>;

/// Bridge: EmbeddingError -> CodeGraphError so OnnxEmbeddingProvider can implement
/// the EmbeddingProvider port from the domain.
impl From<EmbeddingError> for domain::error::CodeGraphError {
    fn from(e: EmbeddingError) -> Self {
        domain::error::CodeGraphError::Other(e.to_string())
    }
}

/// ONNX-based embedding provider that runs a sentence-transformer model locally.
///
/// The session is guarded by a Mutex because `Session::run` requires `&mut self`,
/// but the `EmbeddingProvider` trait requires `&self` (shared reference).
pub struct OnnxEmbeddingProvider {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    /// Output dimensionality (e.g. 384 for all-MiniLM-L6-v2).
    dimension: usize,
}

impl OnnxEmbeddingProvider {
    /// Create a new provider from pre-downloaded model and tokenizer paths.
    pub fn new(model_path: &Path, tokenizer_path: &Path, dimension: usize) -> Result<Self> {
        let session = Session::builder()
            .map_err(|e| EmbeddingError::Inference(format!("session builder failed: {e}")))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| EmbeddingError::Inference(format!("opt level failed: {e}")))?
            .with_intra_threads(1)
            .map_err(|e| EmbeddingError::Inference(format!("intra threads failed: {e}")))?
            .commit_from_file(model_path)
            .map_err(|e| EmbeddingError::Inference(format!("model load failed: {e}")))?;

        let tokenizer = crate::tokenizer::load_tokenizer(tokenizer_path)?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            dimension,
        })
    }

    /// Create a provider by automatically resolving the model from cache
    /// (downloading if needed) using the `download` module.
    pub fn from_model_name(model_name: &str, dimension: usize) -> Result<Self> {
        let files = download::ensure_model(model_name)?;
        Self::new(&files.model_path, &files.tokenizer_path, dimension)
    }

    fn run_embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // Encode all texts at once; tokenizer is configured with batch-longest padding.
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| EmbeddingError::Tokenizer(format!("encode_batch failed: {e}")))?;

        let batch_size = encodings.len();
        let seq_len = encodings[0].get_ids().len();

        debug!(batch_size, seq_len, "running ONNX inference");

        // Flatten input_ids and attention_mask to [batch, seq_len] row-major.
        let mut input_ids_flat: Vec<i64> = Vec::with_capacity(batch_size * seq_len);
        let mut attention_flat: Vec<i64> = Vec::with_capacity(batch_size * seq_len);

        for enc in &encodings {
            for &id in enc.get_ids() {
                input_ids_flat.push(id as i64);
            }
            for &mask in enc.get_attention_mask() {
                attention_flat.push(mask as i64);
            }
        }

        let shape = vec![batch_size as i64, seq_len as i64];

        let input_ids_tensor =
            Tensor::<i64>::from_array((shape.as_slice(), input_ids_flat.into_boxed_slice()))
                .map_err(|e| EmbeddingError::Inference(format!("input_ids tensor failed: {e}")))?;

        let attention_tensor =
            Tensor::<i64>::from_array((shape.as_slice(), attention_flat.into_boxed_slice()))
                .map_err(|e| {
                    EmbeddingError::Inference(format!("attention_mask tensor failed: {e}"))
                })?;

        // Re-collect attention masks before moving into session scope.
        let attention_masks: Vec<Vec<i64>> = encodings
            .iter()
            .map(|enc| enc.get_attention_mask().iter().map(|&m| m as i64).collect())
            .collect();

        // Run inference and extract data all within the same Mutex guard scope.
        // SessionOutputs borrows from Session, so we must copy the f32 data out
        // before the guard is dropped.
        let (embedding_data, out_batch, out_seq, out_dim) = {
            let mut session = self
                .session
                .lock()
                .map_err(|e| EmbeddingError::Inference(format!("session lock poisoned: {e}")))?;
            let outputs = session
                .run(ort::inputs![input_ids_tensor, attention_tensor])
                .map_err(|e| EmbeddingError::Inference(format!("session run failed: {e}")))?;

            // Extract token_embeddings: prefer named key, fall back to first output.
            let output_ref: &ort::value::DynValue = if outputs.contains_key("token_embeddings") {
                &outputs["token_embeddings"]
            } else {
                &outputs[0usize]
            };

            let (shape_obj, data) = output_ref
                .try_extract_tensor::<f32>()
                .map_err(|e| EmbeddingError::Inference(format!("extract tensor failed: {e}")))?;

            let dims: &[i64] = shape_obj;
            // Expected: [batch_size, seq_len, dim]
            if dims.len() < 3 {
                return Err(EmbeddingError::Inference(format!(
                    "unexpected output shape: {dims:?}"
                )));
            }
            let (out_batch, out_seq, out_dim) =
                (dims[0] as usize, dims[1] as usize, dims[2] as usize);

            // Copy data out before releasing the lock / dropping outputs.
            (data.to_vec(), out_batch, out_seq, out_dim)
        };

        Ok(mean_pool(
            &embedding_data,
            out_batch,
            out_seq,
            out_dim,
            &attention_masks,
        ))
    }
}

/// Mean-pool token embeddings weighted by the attention mask.
///
/// `token_embeddings` is flat row-major `[batch * seq_len * dim]`.
/// Returns one `[dim]` vector per batch item.
pub fn mean_pool(
    token_embeddings: &[f32],
    batch: usize,
    seq_len: usize,
    dim: usize,
    attention_masks: &[Vec<i64>],
) -> Vec<Vec<f32>> {
    let mut results = Vec::with_capacity(batch);

    for b in 0..batch {
        let mut pooled = vec![0.0_f32; dim];
        let mut total_weight = 0.0_f32;

        for t in 0..seq_len {
            let mask_val = attention_masks
                .get(b)
                .and_then(|m| m.get(t))
                .copied()
                .unwrap_or(0);
            if mask_val == 0 {
                continue;
            }
            let offset = (b * seq_len + t) * dim;
            let weight = mask_val as f32;
            total_weight += weight;
            for d in 0..dim {
                pooled[d] += token_embeddings[offset + d] * weight;
            }
        }

        if total_weight > 0.0 {
            for v in &mut pooled {
                *v /= total_weight;
            }
        }

        results.push(pooled);
    }

    results
}

impl domain::ports::EmbeddingProvider for OnnxEmbeddingProvider {
    fn embed_batch(&self, texts: &[String]) -> domain::error::Result<Vec<Vec<f32>>> {
        self.run_embed(texts)
            .map_err(domain::error::CodeGraphError::from)
    }

    fn embed_query(&self, text: &str) -> domain::error::Result<Vec<f32>> {
        let mut results = self
            .run_embed(&[text.to_owned()])
            .map_err(domain::error::CodeGraphError::from)?;
        results
            .pop()
            .ok_or_else(|| domain::error::CodeGraphError::Other("empty embed result".into()))
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- mean_pool unit tests (no ONNX model required) ---

    #[test]
    fn mean_pool_single_item_all_masked() {
        // batch=1, seq_len=3, dim=3, all tokens active
        // token 0: [1,2,3], token 1: [4,5,6], token 2: [7,8,9]
        // mean = [4,5,6]
        let embeddings = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let masks = vec![vec![1i64, 1, 1]];
        let result = mean_pool(&embeddings, 1, 3, 3, &masks);
        assert_eq!(result.len(), 1);
        assert!((result[0][0] - 4.0).abs() < 1e-5);
        assert!((result[0][1] - 5.0).abs() < 1e-5);
        assert!((result[0][2] - 6.0).abs() < 1e-5);
    }

    #[test]
    fn mean_pool_ignores_padded_tokens() {
        // batch=1, seq_len=3, dim=2
        // token 0: [1,2], token 1: [3,4], token 2: [0,0] (padded, mask=0)
        // mean of first two = [2, 3]
        let embeddings = vec![1.0, 2.0, 3.0, 4.0, 0.0, 0.0];
        let masks = vec![vec![1i64, 1, 0]];
        let result = mean_pool(&embeddings, 1, 3, 2, &masks);
        assert_eq!(result.len(), 1);
        assert!((result[0][0] - 2.0).abs() < 1e-5, "got {}", result[0][0]);
        assert!((result[0][1] - 3.0).abs() < 1e-5, "got {}", result[0][1]);
    }

    #[test]
    fn mean_pool_batch_two_different_lengths() {
        // batch=2, seq_len=2, dim=2
        // item 0: tokens [1,2],[3,4], both active → mean [2,3]
        // item 1: tokens [5,6],[7,8], only first active → mean [5,6]
        let embeddings = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let masks = vec![vec![1i64, 1], vec![1, 0]];
        let result = mean_pool(&embeddings, 2, 2, 2, &masks);
        assert_eq!(result.len(), 2);
        // item 0
        assert!((result[0][0] - 2.0).abs() < 1e-5);
        assert!((result[0][1] - 3.0).abs() < 1e-5);
        // item 1
        assert!((result[1][0] - 5.0).abs() < 1e-5);
        assert!((result[1][1] - 6.0).abs() < 1e-5);
    }

    #[test]
    fn mean_pool_empty_batch() {
        let result = mean_pool(&[], 0, 0, 384, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn mean_pool_zero_mask_returns_zeros() {
        // All tokens padded → pooled should be all zeros
        let embeddings = vec![9.0, 9.0, 9.0, 9.0];
        let masks = vec![vec![0i64, 0]];
        let result = mean_pool(&embeddings, 1, 2, 2, &masks);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], vec![0.0, 0.0]);
    }
}
