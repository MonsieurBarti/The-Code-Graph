# Spec — M02-S04: Embeddings + Hybrid Search

## Problem Statement

The Code Graph's search is FTS5-only (BM25 keyword matching). It misses semantic matches — searching "authentication handler" won't find `validate_token`. The reference project (code-review-graph) has hybrid search but achieves only MRR 0.35 due to brute-force vector scan in Python, minimal text representation, and no graph-context enrichment.

**Solution**: Add neural embeddings via ONNX Runtime (`all-MiniLM-L6-v2`, 384-dim) with Reciprocal Rank Fusion (RRF) to combine FTS5 BM25 and vector cosine similarity. Transparent upgrade — `code-graph search` automatically uses hybrid when embeddings exist, falls back to FTS5-only otherwise.

**Who benefits**: AI coding agents get semantically relevant results with fewer queries. Developers find symbols by intent ("error handling", "database connection") not just exact names.

## Approach

### Transparent Hybrid (Approach A)

- `code-graph index --embed` generates embeddings during normal indexing (opt-in flag)
- `code-graph search` transparently uses hybrid FTS5+vector when embeddings exist
- Falls back to FTS5-only when no embeddings are present (zero behavioral change for users who haven't opted in)
- ONNX model auto-downloaded on first use, cached locally

### Why This Approach

- Aligns with R4 requirement: "transparent upgrade to existing search command"
- Minimal CLI surface change (one new flag on `index`)
- Users don't need to think about embeddings once set up
- Graceful degradation preserves existing behavior

## Architecture

### New Crate: `crates/embeddings`

Isolates the ONNX Runtime native dependency from the rest of the workspace. Uses the `ort` crate (Rust ONNX Runtime bindings) with static linking. The crate is behind a Cargo feature flag `embeddings` on the `cli` crate so that builds without ONNX remain fast for unrelated work.

| File | Purpose |
|------|---------|
| `crates/embeddings/src/lib.rs` | `OnnxEmbeddingProvider` implementing `EmbeddingProvider` trait |
| `crates/embeddings/src/tokenizer.rs` | WordPiece tokenizer via `tokenizers` crate |
| `crates/embeddings/src/download.rs` | Model auto-download from HuggingFace Hub + cache management |

### New Domain Files

| File | Purpose |
|------|---------|
| `crates/domain/src/analysis/search.rs` | Pure RRF fusion algorithm, text representation builder, kind boosting |
| `crates/domain/src/use_cases/embed.rs` | `EmbedUseCase<S, E, V>` — orchestrates embedding pipeline: load symbols from `GraphStore`, build text representations, call `EmbeddingProvider::embed_batch`, store via `VectorStore` |

### New Storage Files

| File | Purpose |
|------|---------|
| `crates/storage/src/embedding_store.rs` | SQLite vector storage (binary-packed f32 BLOBs), cosine similarity search |

### Modified Files

| File | Change |
|------|--------|
| `crates/domain/src/ports.rs` | Add `EmbeddingProvider` + `VectorStore` traits |
| `crates/domain/src/model.rs` | Add `EmbeddingConfig`, `HybridSearchConfig`, `EmbeddingEntry`, `ScoreSource` enum. Extend `SearchResult` with `score_source: Option<ScoreSource>` (`#[serde(skip_serializing_if = "Option::is_none")]`). **Blast radius**: all sites constructing `SearchResult` must add `score_source: None` — includes `search_index.rs`, `test_support.rs`, `output.rs` tests, `eval/runner.rs`, `eval/adapters.rs` |
| `crates/domain/src/analysis/mod.rs` | Add `pub mod search;` |
| `crates/domain/src/use_cases/mod.rs` | Add `pub mod embed;` |
| `crates/domain/src/use_cases/query.rs` | Accept optional `VectorStore` via `Option<Arc<dyn VectorStore>>` (avoids third generic), hybrid search path. Method grows from pass-through to ~30 lines with runtime branching |
| `crates/domain/src/test_support.rs` | Update `InMemoryGraphStore` `SearchIndex` impl to include `score_source: None` in results. Optionally add `InMemoryVectorStore` for testing |
| `crates/storage/src/schema.rs` | Schema migration v1→v2 (see Schema section below). Introduce `SCHEMA_V2` constant for fresh installs |
| `crates/storage/src/search_index.rs` | Update `SearchResult` construction to include `score_source: None` |
| `crates/cli/src/commands/search.rs` | Pass vector store when embeddings available |
| `crates/cli/src/commands/index.rs` | Add `--embed` flag to trigger embedding generation via `EmbedUseCase` |
| `crates/cli/src/config.rs` | Add `EmbeddingsCliConfig` and `HybridSearchCliConfig` to `CodeGraphConfig` (distinct from domain types to avoid name collision with existing `SearchConfig`) |
| `crates/cli/src/output.rs` | Display `score_source` in JSON output when present. Update `Displayable` impl for `Vec<SearchResult>` |
| `crates/eval/src/runner.rs` | Update `SearchResult` construction to include `score_source: None` |
| `crates/eval/src/adapters.rs` | Update `SearchResult` construction to include `score_source: None` |
| `Cargo.toml` (workspace) | Add `crates/embeddings` member, add `embeddings` feature flag to `crates/cli` |
| `.github/workflows/ci.yml` | Add `embeddings` feature-gated CI job (see CI section below) |

### Key Types

```rust
// Domain ports (crates/domain/src/ports.rs)
pub trait EmbeddingProvider: Send + Sync {
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn embed_query(&self, text: &str) -> Result<Vec<f32>>;
    fn dimension(&self) -> usize;
}

pub trait VectorStore: Send + Sync {
    fn store_embeddings(&self, entries: &[EmbeddingEntry]) -> Result<()>;
    fn search_nearest(&self, query_vec: &[f32], limit: usize) -> Result<Vec<(String, f64)>>;
    fn has_embeddings(&self) -> bool;
    fn count(&self) -> Result<usize>;
    fn remove_embeddings(&self, qualified_names: &[&str]) -> Result<()>;
}

// Named struct for embedding storage (crates/domain/src/model.rs)
pub struct EmbeddingEntry {
    pub qualified_name: String,
    pub vector: Vec<f32>,
    pub text_hash: String,
}

// Score source tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreSource {
    Hybrid,
    Fts5,
    Semantic,
}

// Extended SearchResult — score_source is Option to preserve backward compatibility
pub struct SearchResult {
    // ... existing fields ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_source: Option<ScoreSource>,
}

// Config (crates/domain/src/model.rs)
pub struct EmbeddingConfig {
    pub enabled: bool,              // default false
    pub model: String,              // default "all-MiniLM-L6-v2"
    pub batch_size: usize,          // default 64
}

pub struct HybridSearchConfig {
    pub rrf_k: usize,              // default 60
    pub kind_boost: bool,          // default true
}
```

### Embedding Pipeline Orchestration

```rust
// crates/domain/src/use_cases/embed.rs
pub struct EmbedUseCase<S: GraphStore, E: EmbeddingProvider, V: VectorStore> {
    store: S,
    provider: E,
    vector_store: V,
}

impl<S: GraphStore, E: EmbeddingProvider, V: VectorStore> EmbedUseCase<S, E, V> {
    pub fn embed_all(&self, config: &EmbeddingConfig) -> Result<EmbedStats>;
    pub fn embed_incremental(&self, config: &EmbeddingConfig) -> Result<EmbedStats>;
    pub fn cleanup_orphans(&self) -> Result<usize>;
}

pub struct EmbedStats {
    pub total_symbols: usize,
    pub embedded: usize,
    pub skipped: usize,    // unchanged hash
    pub removed: usize,    // orphan cleanup
}
```

### Data Flow

```
index --embed: symbols → text_repr → EmbeddingProvider.embed_batch → VectorStore.store
search:        query → FTS5(BM25) + VectorStore(cosine) → RRF merge → ranked results
```

### `QueryUseCase` Wiring

The existing `QueryUseCase<S, I>` retains two generics. The `VectorStore` is passed as `Option<Arc<dyn VectorStore>>` in the constructor, avoiding a third generic and keeping all existing call sites unchanged when no vector store is available.

## Embedding Pipeline

### Model

- **all-MiniLM-L6-v2**: 384-dimensional sentence embeddings, ~23MB ONNX file
- Downloaded from HuggingFace Hub on first `code-graph index --embed`
- Cached at `$XDG_CACHE_HOME/code-graph/models/all-MiniLM-L6-v2/` (fallback: `~/.cache/code-graph/models/` when `XDG_CACHE_HOME` is unset, per XDG Base Directory spec)
- Tokenizer JSON + ONNX model file stored together
- Max sequence length: 256 tokens. Text representations exceeding this are truncated (standard tokenizer behavior).

### Text Representation

Key differentiator from reference — includes call-graph context:

```rust
fn symbol_to_text(sym: &SymbolNode, edges: &[Edge]) -> String {
    // "Function validate_token in auth, calls hash_password,
    //  called by login_handler, signature: fn validate_token(token: &str) -> bool"
    let mut parts = vec![
        sym.kind.to_string(),
        sym.name.clone(),
        format!("in {}", file_stem(&sym.location.file_path)),
    ];
    if let Some(sig) = &sym.signature {
        parts.push(format!("signature: {sig}"));
    }
    // Top 3 callers + callees for graph context
    let calls: Vec<_> = edges.iter()
        .filter(|e| e.kind == EdgeKind::Calls && e.source == sym.qualified_name)
        .take(3).map(|e| short_name(&e.target)).collect();
    if !calls.is_empty() {
        parts.push(format!("calls {}", calls.join(", ")));
    }
    let callers: Vec<_> = edges.iter()
        .filter(|e| e.kind == EdgeKind::Calls && e.target == sym.qualified_name)
        .take(3).map(|e| short_name(&e.source)).collect();
    if !callers.is_empty() {
        parts.push(format!("called by {}", callers.join(", ")));
    }
    parts.join(", ")
}
```

### Edge-Loading Strategy for Text Representation

Building text representations requires caller/callee context for every symbol. To avoid an O(n) query storm (20k queries for 10k symbols), `EmbedUseCase::embed_all` loads all edges once via `GraphStore::all_edges()` and builds an in-memory `HashMap<String, Vec<Edge>>` keyed by qualified_name (both source and target directions). This is ~O(E) memory where E is total edge count — acceptable at <10k symbol scale. The text builder then does O(1) lookups per symbol.

### Incremental Embedding

- SHA-256 hash of the text representation stored alongside each embedding
- On re-index, only symbols with changed hash OR changed provider are re-embedded
- Edge changes that alter caller/callee context change the text hash, triggering re-embedding of affected symbols (the text representation includes callers/callees, so any edge change naturally changes the hash)

### Batching

- Symbols processed in batches of 64 (configurable via `batch_size`)
- Progress bar via `indicatif` during embedding generation (displayed on stderr, prints completion summary)

## Vector Storage

### Schema

Added to the existing `graph.db` via schema migration v1→v2.

**Fresh install (version 0)**: Uses `SCHEMA_V2` constant containing the full schema (all v1 tables + embeddings table), sets `user_version = 2` atomically.

**Migration (version 1→2)**: Runs migration DDL only:

```sql
-- Migration DDL: v1 → v2
CREATE TABLE IF NOT EXISTS embeddings (
    qualified_name TEXT PRIMARY KEY,
    vector BLOB NOT NULL,
    text_hash TEXT NOT NULL,
    provider TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_embeddings_provider ON embeddings(provider);
PRAGMA user_version = 2;
```

Migration logic in `ensure_schema`:
1. Read `PRAGMA user_version`
2. If 0: run `SCHEMA_V2` (full schema including embeddings), set `user_version = 2`
3. If 1: run migration DDL (add embeddings table only), set `user_version = 2`
4. If 2: no-op
5. Else: error (unknown version)

Two constants: `SCHEMA_V2` (full) and `MIGRATION_V1_TO_V2` (incremental). Fresh installs never touch v1.

Vectors stored as binary-packed f32 (384 dims * 4 bytes = 1536 bytes per vector).

### Cosine Similarity

Brute-force cosine similarity computed in Rust. For <10k symbols: ~4M float operations, completes in <10ms. No vector index extension needed at this scale.

### Dimension Mismatch Protection

The `provider` column tracks which model produced each embedding. When the provider changes (e.g., user switches `--embed-model`), all embeddings from the old provider are invalidated and re-generated on next `--embed` run.

## Hybrid Search (RRF Fusion)

### Algorithm

1. **FTS5 leg**: Run existing BM25 search, get top-N ranked by BM25 score
2. **Vector leg**: Embed query, compute cosine similarity against all stored vectors, get top-N
3. **RRF merge**: For each result across both lists, compute RRF score = sum of `1/(k + rank + 1)` where rank is 0-based position in each list (so top result has denominator `k + 1`)
4. **Kind boosting**: Apply multipliers based on query pattern
5. **Return**: Merged results sorted by final score

### RRF Implementation

```rust
pub fn rrf_merge(
    lists: &[Vec<(String, f64)>],
    k: usize,
) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();
    for list in lists {
        for (rank, (qn, _)) in list.iter().enumerate() {
            // rank is 0-based, so top result gets 1/(k+1)
            *scores.entry(qn.clone()).or_default() += 1.0 / (k + rank + 1) as f64;
        }
    }
    let mut merged: Vec<_> = scores.into_iter().collect();
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    merged
}
```

### Kind Boosting

- PascalCase query (e.g., `AuthService`) → boost Class/Struct/Trait/Interface by 1.5x
- snake_case query (e.g., `validate_token`) → boost Function/Method by 1.5x
- `::` in query → boost qualified name exact matches by 2.0x (Rust-idiomatic)

## CLI Interface

### Modified Commands

```
code-graph index [OPTIONS]
  --embed              Generate embeddings for all symbols (triggers model download on first use)
  --embed-model <M>    ONNX model name [default: all-MiniLM-L6-v2]

code-graph search <QUERY> [OPTIONS]
  --limit <N>          Max results [default: 20]
  --semantic-only      Use only vector similarity (skip FTS5)
  --fts-only           Use only FTS5 BM25 (skip vectors)
  (default: hybrid when embeddings exist, FTS5-only otherwise)
```

`--semantic-only` and `--fts-only` are mutually exclusive. Passing both returns a CLI error.

### Configuration

In `.code-graph/config.toml`:

```toml
[embeddings]
enabled = true
model = "all-MiniLM-L6-v2"
batch_size = 64

[search]
rrf_k = 60
kind_boost = true
```

### Output

Same `SearchResult` format as today (compact/table/json via global flags). `score_source` field is `Option<ScoreSource>` — present only in JSON output when hybrid or semantic search is active, omitted otherwise (backward compatible via `skip_serializing_if`).

## Acceptance Criteria

1. `code-graph index --embed` generates embeddings for all non-File symbols. Embeddings stored in SQLite `embeddings` table. Progress bar displayed on stderr with completion summary
2. `code-graph search <query>` returns hybrid FTS5+vector results when embeddings exist; falls back to FTS5-only when they don't. No behavioral change for users who haven't run `--embed`
3. `--semantic-only` and `--fts-only` flags restrict to a single retrieval method. Passing both returns a CLI error
4. **MRR >= 0.60** on a committed evaluation dataset in `crates/eval/` (ground-truth query-result pairs built from open-source Rust projects). The dataset and eval harness are in scope for this slice
5. Hybrid search (vector cosine similarity + RRF fusion) completes in **<100ms** for 10k-symbol graphs, measured independently of FTS5 time
6. ONNX model auto-downloads on first `--embed` invocation. Cached in `$XDG_CACHE_HOME/code-graph/models/` (fallback: `~/.cache/code-graph/models/`). Subsequent runs skip download
7. `--embed-model` flag overrides the `[embeddings].model` config value. CLI flags override config file values for all embedding/search settings
8. Incremental embedding: unchanged symbols (same text hash + provider) are not re-embedded. Only new/modified symbols trigger inference
9. Text representation for each embedded symbol includes: symbol kind, name, containing file, signature (if present), top-3 callees, and top-3 callers
10. When a symbol's call-graph context changes (callers or callees added/removed) but the symbol's own code is unchanged, `index --embed` re-embeds that symbol (because the text representation hash changes)
11. Kind boosting: PascalCase queries boost Class/Struct/Trait/Interface results by 1.5x; snake_case queries boost Function/Method results by 1.5x; queries containing `::` boost qualified-name exact matches by 2.0x
12. All three output formats (compact/table/json) produce valid output. JSON includes `score_source` field when hybrid/semantic search is active
13. Empty graph returns zero results without error. Missing embeddings in `--semantic-only` mode returns empty results gracefully
14. `embeddings` table is cleaned up when symbols are removed during re-indexing (orphan cleanup via `EmbedUseCase::cleanup_orphans`)
15. CI builds and all tests pass on macOS (aarch64) and Linux (x86_64), including ONNX model loading and embedding generation
16. When ONNX model download fails (network error), CLI returns a clear error message and does not leave partial files in the cache directory
17. Schema migration from v1→v2 adds the `embeddings` table to existing databases without data loss

## CI Strategy

The `embeddings` feature flag gates ONNX Runtime dependency on the `cli` crate. CI requires:

1. **Default job** (`cargo test --workspace`): Tests all non-embedding code. Embedding-related code is behind `#[cfg(feature = "embeddings")]` and not compiled.
2. **Embeddings job** (`cargo test --workspace --features embeddings`): Tests full pipeline including ONNX. Runs on both macOS-aarch64 and ubuntu-x86_64.
3. **System deps for Ubuntu**: `apt-get install -y libstdc++-dev` (ONNX Runtime static linking). The `ort` crate handles downloading pre-built ONNX Runtime libs.
4. **Model in CI tests**: Unit tests use a mock `EmbeddingProvider` (returns deterministic vectors). Integration tests requiring the real ONNX model are gated behind `#[cfg(feature = "embeddings")]` and the CI embeddings job.

## Acknowledged Concerns

- **MRR 0.60 with NL model on code**: `all-MiniLM-L6-v2` is trained on natural language, not code. The text representation (natural-language descriptions of symbols) mitigates this, and RRF hybrid means FTS5 carries exact matches. The eval dataset should include both exact-name and semantic queries to measure improvement accurately. If MRR 0.60 proves unreachable, a code-specific ONNX model (UniXcoder, CodeBERT) can be swapped in without architecture changes.
- **Binary size**: ONNX Runtime adds ~50-80MB when `embeddings` feature is active. Acceptable for a developer CLI tool. Users who don't need embeddings use the default build without the feature.
- **Cascading re-embedding**: A single new edge can cause re-embedding of multiple symbols (since caller/callee context changes their text hashes). At <10k symbol scale this is negligible. Documented as known behavior.
- **`tokenizers` crate weight**: Adds ~30s compile time. Necessary for correct WordPiece tokenization of the ONNX model. No lighter alternative handles the `tokenizer.json` format.
- **Eval dataset authoring**: Building semantic query ground-truth is manual effort. Scope to 30-50 queries across 2-3 open-source Rust projects. Start with the existing eval infrastructure in `crates/eval/`.

## Non-Goals

- GPU inference (CPU-only ONNX for simplicity)
- Multiple concurrent embedding providers (single provider per DB)
- Custom model training / fine-tuning
- Real-time embedding updates on file watch (batch only during `index`)
- Vector database extensions (HNSW, FAISS) — brute-force is sufficient at our scale
- Pluggable API providers (Google, OpenAI) — local ONNX only for v0.2
