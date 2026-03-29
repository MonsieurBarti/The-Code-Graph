# Research — M02-S04: Embeddings + Hybrid Search

## 1. ONNX Runtime (`ort` crate)

### Version & Compatibility
- **Crate**: `ort` v2.0.0-rc.12 (wraps ONNX Runtime 1.24, 7.7M total downloads)
- **Rust requirement**: 1.88+ (edition 2024) — verify our MSRV
- **API stability**: RC but heavily adopted (2.9M downloads in 90 days)

### Model Loading & Inference

```rust
use ort::session::Session;
use ort::value::TensorRef;

let session = Session::builder()?
    .with_optimization_level(GraphOptimizationLevel::Level3)?
    .with_intra_threads(4)?
    .commit_from_file("model.onnx")?;

let outputs = session.run(inputs![
    "input_ids" => TensorRef::from_array_view(([batch, seq_len], &*input_ids_i64))?,
    "attention_mask" => TensorRef::from_array_view(([batch, seq_len], &*attention_mask_i64))?,
    "token_type_ids" => TensorRef::from_array_view(([batch, seq_len], &*token_type_ids_i64))?
])?;
```

Input tensors are all `i64` with shape `[batch_size, seq_len]`. Output is `last_hidden_state` with shape `[batch, seq_len, 384]` — **mean pooling with attention mask required** for sentence embeddings.

### Quantized Model Variants

| File | Size | Target |
|------|------|--------|
| `model.onnx` | 90.4 MB | FP32 (development) |
| `model_qint8_arm64.onnx` | 23 MB | macOS aarch64 |
| `model_quint8_avx2.onnx` | 23 MB | Linux x86_64 |

**Decision**: Use platform-specific quantized models (23 MB each). Significant speedup with minimal accuracy loss.

### Linking Strategy

| Strategy | Binary Size | System Deps | Recommendation |
|----------|-------------|-------------|----------------|
| `download-binaries` (default) | +200-500 KB (FFI only) + dylib | None (auto-download) | **Use for dev + CI** |
| `load-dynamic` | +200-500 KB | User provides dylib | Consider for distribution |
| Static (build from source) | +20-70 MB | cmake, build-essential | Not worth the complexity |

**Decision**: Use default `download-binaries` for development and CI. Consider `load-dynamic` for crates.io release (M02-S07) where users provide the ONNX Runtime dylib or it auto-downloads.

### CI Impact
- `download-binaries` requires **no system dependencies** on Ubuntu or macOS
- First build downloads 8-30 MB of prebuilt libs (cache `~/.cache/ort` in CI)
- `copy-dylibs` feature (default) places dylibs next to binary for test execution
- `LD_LIBRARY_PATH` may be needed on Linux CI

## 2. Tokenizer (`tokenizers` crate)

### Version & API
- **Crate**: `tokenizers` v0.22.2 (12.7M downloads, HuggingFace official)
- **Use with**: `default-features = false` (drops C/C++ deps: onig, esaxx)
- **tokenizer.json**: 466 KB, download from HuggingFace Hub alongside ONNX model

### Complete Encoding Pipeline

```rust
use tokenizers::Tokenizer;

let mut tokenizer = Tokenizer::from_file("tokenizer.json")?;

// Configure for all-MiniLM-L6-v2
tokenizer.with_truncation(Some(TruncationParams {
    max_length: 256,
    strategy: TruncationStrategy::LongestFirst,
    direction: TruncationDirection::Right,
    stride: 0,
}))?;

tokenizer.with_padding(Some(PaddingParams {
    strategy: PaddingStrategy::BatchLongest,
    direction: PaddingDirection::Right,
    pad_id: 0,
    pad_type_id: 0,
    pad_token: "[PAD]".to_string(),
    pad_to_multiple_of: None,
}));

// Batch encode (parallelized via Rayon internally)
let encodings = tokenizer.encode_batch(texts, true)?;

// Cast u32 → i64 for ONNX
let input_ids: Vec<i64> = encodings.iter()
    .flat_map(|e| e.get_ids().iter().map(|&id| id as i64))
    .collect();
```

### Key Detail: u32 → i64 Cast
`tokenizers` returns `&[u32]` but ONNX models expect `i64` tensors. Explicit cast needed.

### Mean Pooling (Post-Inference)

```rust
let (shape, hidden) = outputs["last_hidden_state"].try_extract_tensor::<f32>()?;
let seq_len = shape[1];
let dim = 384;

let mut embedding = vec![0.0f32; dim];
let mut mask_sum = 0.0f32;
for t in 0..seq_len {
    let mask_val = attention_mask_i64[t] as f32;
    mask_sum += mask_val;
    for d in 0..dim {
        embedding[d] += hidden[t * dim + d] * mask_val;
    }
}
for d in 0..dim { embedding[d] /= mask_sum; }
```

### Dependency Footprint
- Pure Rust with `default-features = false`
- Pulls: `serde`, `rayon`, `regex`, `unicode-normalization` (most overlap with existing deps)
- Adds ~30-45s first compile, ~2-4 MB to binary
- `rayon` dep is shared with existing `cli` crate

## 3. Eval Harness & MRR Measurement

### Existing Infrastructure

| Component | Status | Notes |
|-----------|--------|-------|
| `crates/eval/src/metrics.rs` | **MRR already implemented** | Works on `Vec<Vec<String>>` (ranked results vs ground truth) |
| `crates/eval/src/runner.rs` | **Search eval exists** | Calls `QueryUseCase::search()`, current target: MRR = 0.30 |
| `crates/eval/src/dataset.rs` | **Manifest + query parsing** | `SearchQuery` struct with repo, query, expected fields |
| `eval/suites/search/queries/` | **72 queries, 5 repos, 5 langs** | ALL exact-name queries (e.g., "SearcherBuilder") |

### Critical Gap: No Semantic Queries
All 72 existing queries are single-token exact-name lookups. FTS5 already handles these well. Hybrid search value comes from **semantic queries** ("authentication handler" → `validate_token`) which don't exist in the dataset.

### Eval Dataset Plan

**Add 40 semantic queries** across 2-3 Rust projects:

| Category | Count | Purpose | Example |
|----------|-------|---------|---------|
| Exact-name | 15 (new) | No regression check | `validate_token` |
| Semantic/intent | 25 | Key differentiator | `"authentication handler"` → validate_token |
| Partial-name | 10 | RRF fusion benefit | `"search build"` → SearcherBuilder |

**Recommended repos**: ripgrep (existing), tokio, serde — diverse symbol kinds, rich semantics, stable APIs.

**Dataset extension**: Add `category: Option<String>` field to `SearchQuery` (backward-compatible).

### Measurement Strategy

Run each query through three modes, report 3x3 matrix:

| | MRR-exact | MRR-semantic | MRR-overall |
|---|-----------|-------------|-------------|
| FTS5-only | baseline | expect low | |
| Semantic-only | expect moderate | key metric | |
| Hybrid (RRF) | should not regress | should improve | **≥0.60 target** |

### MRR 0.60 Feasibility Concern
`all-MiniLM-L6-v2` is NL-trained, not code-trained. Our text representation converts symbols to NL descriptions (mitigating this), but MRR 0.60 on semantic queries may be ambitious. If unreachable, the architecture allows swapping to a code-specific model (UniXcoder, CodeBERT ONNX) without structural changes.

## 4. Codebase Integration Points

### SearchResult Construction — Simpler Than Expected

**Only ONE construction site**: `crates/storage/src/search_index.rs` (lines 57-63). No other code constructs `SearchResult` directly. The stress test's concern about "blast radius" was overestimated.

Additional touch points for the new `score_source` field:
- `model.rs` struct definition + serde test
- `output.rs` display logic
- No eval/test_support changes needed (they consume, not construct)

### Schema Migration — Straightforward

Current `ensure_schema` in `schema.rs` uses `PRAGMA user_version` with a clean match statement. Adding v2 is a direct extension:
- Version 0 → run `SCHEMA_V2` (full schema including embeddings)
- Version 1 → run `MIGRATION_V1_TO_V2` (add embeddings table only)
- Version 2 → no-op

The `REFERENCES symbols(qualified_name) ON DELETE CASCADE` on the embeddings table enables automatic orphan cleanup.

### QueryUseCase Wiring — Clean

Current: `QueryUseCase<S, I>` with `search()` as a 3-line pass-through. Add `vector_store: Option<Arc<dyn VectorStore>>` to the struct. Constructor remains backward-compatible via `new()` (sets `None`) and `with_vector_store()` (sets `Some`).

The `search()` method grows to ~30 lines with the hybrid path but the branching is clean: check `has_embeddings()` → yes: hybrid RRF → no: FTS5 pass-through.

### Config Extension — Follows Existing Pattern

Add `embeddings: Option<EmbeddingsCliConfig>` to `CodeGraphConfig`. Existing test at `config.rs:78` demonstrates the pattern. Config is deserialized from TOML, CLI args override.

**Note**: Existing `SearchConfig` has only `max_results`. Rename domain type to `HybridSearchConfig` to avoid collision.

### CI — Feature Matrix

Current CI runs `cargo test --workspace` without feature flags. Add a matrix dimension:
```yaml
strategy:
  matrix:
    features: ['', 'embeddings']
```
Cache `~/.cache/ort` to avoid re-downloading ONNX Runtime on every build.

## 5. Cargo.toml for New Crate

```toml
[package]
name = "embeddings"
version = "0.1.0"
edition = "2024"

[dependencies]
domain = { path = "../domain" }
ort = { version = "2.0.0-rc.12" }
tokenizers = { version = "0.22.2", default-features = false }
sha2 = "0.10"
tracing = "0.1"
thiserror = "2"

[dev-dependencies]
tempfile = "3"
```

## 6. Key Risks & Mitigations

| Risk | Severity | Mitigation |
|------|----------|------------|
| Rust 1.88 MSRV requirement from `ort` | Medium | Verify current toolchain; update if needed |
| MRR 0.60 unreachable with NL model | Medium | Architecture allows model swap; measure early |
| ONNX Runtime download flaky in CI | Low | Cache `~/.cache/ort`; retry logic in CI |
| Binary size +50-80 MB with ONNX | Low | Feature-gated; users opt in |
| Quantized model accuracy loss | Low | Benchmark against FP32; MiniLM quantization is well-tested |

## 7. Summary of Decisions

1. **`ort` 2.0.0-rc.12** with default features for dev/CI
2. **`tokenizers` 0.22.2** with `default-features = false`
3. **Quantized ONNX models** (23 MB) platform-specific
4. **Auto-download** model + tokenizer.json from HuggingFace Hub
5. **40 new semantic queries** for eval dataset, 3x3 MRR matrix
6. **CASCADE delete** on embeddings table for orphan cleanup
7. **Feature flag** `embeddings` on CLI crate for CI gating
