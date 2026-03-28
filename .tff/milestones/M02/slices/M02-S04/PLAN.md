# M02-S04: Embeddings + Hybrid Search — Implementation Plan

> For agentic workers: execute task-by-task with TDD.

**Goal:** Add neural embeddings via ONNX Runtime with RRF hybrid search, transparently upgrading `code-graph search`.
**Architecture:** New `crates/embeddings` crate (ONNX isolation), domain analysis/search.rs (pure RRF), storage embedding_store.rs (SQLite vectors), extended QueryUseCase with optional VectorStore.
**Tech Stack:** `ort` 2.0.0-rc.12, `tokenizers` 0.22.2, `sha2`, all-MiniLM-L6-v2 (384-dim ONNX)

## File Structure

### New Files
| File | Responsibility |
|------|---------------|
| `crates/embeddings/Cargo.toml` | Crate manifest with ort + tokenizers deps |
| `crates/embeddings/src/lib.rs` | `OnnxEmbeddingProvider` implementing `EmbeddingProvider` |
| `crates/embeddings/src/tokenizer.rs` | Tokenizer loading, batch encoding, u32→i64 cast |
| `crates/embeddings/src/download.rs` | Model auto-download from HuggingFace Hub, cache management |
| `crates/domain/src/analysis/search.rs` | `rrf_merge`, `symbol_to_text`, `detect_kind_boost` |
| `crates/domain/src/use_cases/embed.rs` | `EmbedUseCase` orchestration |
| `crates/storage/src/embedding_store.rs` | `VectorStore` impl on `SqliteStore` |

### Modified Files
| File | Change |
|------|--------|
| `crates/domain/src/model.rs` | Add `ScoreSource`, `EmbeddingEntry`, `EmbeddingConfig`, `HybridSearchConfig`; extend `SearchResult` |
| `crates/domain/src/ports.rs` | Add `EmbeddingProvider` + `VectorStore` traits |
| `crates/domain/src/analysis/mod.rs` | Add `pub mod search;` |
| `crates/domain/src/use_cases/mod.rs` | Add `pub mod embed;` |
| `crates/domain/src/use_cases/query.rs` | Add `Option<Arc<dyn VectorStore>>`, hybrid search path |
| `crates/domain/src/test_support.rs` | Update `SearchResult` construction, add `InMemoryVectorStore` |
| `crates/storage/src/schema.rs` | `SCHEMA_V2`, `MIGRATION_V1_TO_V2`, updated `ensure_schema` |
| `crates/storage/src/search_index.rs` | Add `score_source: None` to `SearchResult` construction |
| `crates/cli/src/commands/mod.rs` | Add `--embed` to `IndexArgs`, `--semantic-only`/`--fts-only` to `SearchArgs` |
| `crates/cli/src/commands/search.rs` | Wire hybrid search via VectorStore |
| `crates/cli/src/commands/index.rs` | Wire `--embed` via `EmbedUseCase` |
| `crates/cli/src/config.rs` | Add `EmbeddingsCliConfig`, extend `SearchConfig` with `rrf_k`, `kind_boost` |
| `crates/cli/src/output.rs` | `score_source` in JSON output |
| `Cargo.toml` (workspace) | Add `crates/embeddings` member |
| `crates/cli/Cargo.toml` | Add optional `embeddings` dep behind feature flag |
| `.github/workflows/ci.yml` | Add embeddings feature-gated CI job |

---

## Wave 0 (no dependencies)

### T01: Foundation — types, ports, schema migration, module registration
**Files:** Modify `crates/domain/src/model.rs`, `crates/domain/src/ports.rs`, `crates/domain/src/analysis/mod.rs`, `crates/domain/src/use_cases/mod.rs`, `crates/domain/src/test_support.rs`, `crates/storage/src/schema.rs`, `crates/storage/src/search_index.rs`, `Cargo.toml` (workspace), create `crates/embeddings/Cargo.toml`, `crates/embeddings/src/lib.rs`
**Traces to:** AC17 (schema migration), AC12 (score_source in output)

- [ ] Step 1: Add `ScoreSource` enum, `EmbeddingEntry`, `EmbeddingConfig`, `HybridSearchConfig` to `crates/domain/src/model.rs`. Add `score_source: Option<ScoreSource>` with `#[serde(skip_serializing_if = "Option::is_none")]` to `SearchResult`.

```rust
// In model.rs, after SearchResult
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreSource {
    Hybrid,
    Fts5,
    Semantic,
}

#[derive(Debug, Clone)]
pub struct EmbeddingEntry {
    pub qualified_name: String,
    pub vector: Vec<f32>,
    pub text_hash: String,
}

#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub model: String,
    pub batch_size: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: "all-MiniLM-L6-v2".into(),
            batch_size: 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Hybrid,
    FtsOnly,
    SemanticOnly,
}

#[derive(Debug, Clone)]
pub struct HybridSearchConfig {
    pub rrf_k: usize,
    pub kind_boost: bool,
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self { rrf_k: 60, kind_boost: true }
    }
}

pub struct EmbedStats {
    pub total_symbols: usize,
    pub embedded: usize,
    pub skipped: usize,
    pub removed: usize,
}
```

- [ ] Step 2: Add `score_source: None` to `SearchResult` construction in all 4 sites: `crates/storage/src/search_index.rs` (line 57), `crates/domain/src/test_support.rs` (line 189), `crates/domain/src/model.rs` serde test (line 910), `crates/cli/src/output.rs` `sample_search_results()` (line 1182).

- [ ] Step 3: Add `EmbeddingProvider` + `VectorStore` traits to `crates/domain/src/ports.rs`:

```rust
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
```

- [ ] Step 4: Add `pub mod search;` to `crates/domain/src/analysis/mod.rs` and `pub mod embed;` to `crates/domain/src/use_cases/mod.rs` (create empty files as placeholders).

- [ ] Step 5: Schema migration in `crates/storage/src/schema.rs`:

```rust
pub(crate) const MIGRATION_V1_TO_V2: &str = "
CREATE TABLE embeddings (
    qualified_name TEXT PRIMARY KEY REFERENCES symbols(qualified_name) ON DELETE CASCADE,
    vector BLOB NOT NULL,
    text_hash TEXT NOT NULL,
    provider TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_embeddings_provider ON embeddings(provider);
";

// SCHEMA_V2 = SCHEMA_V1 + MIGRATION_V1_TO_V2 (minus the CASCADE ref issue)
// Update ensure_schema match to handle 0→v2, 1→migrate, 2→noop
```

- [ ] Step 6: Create `crates/embeddings/Cargo.toml` (stub crate with `domain = { path = "../domain" }` dep) and `crates/embeddings/src/lib.rs` (empty module). Add to workspace `Cargo.toml` members.

- [ ] Step 7: Run `cargo test --workspace` — verify all existing tests pass with the new `score_source` field.
- [ ] Step 8: Run `cargo test -p storage -- schema` — verify v1→v2 migration and fresh v2 install work.
- [ ] Step 9: Commit: `feat(S04/T01): foundation types, ports, schema v2 migration`

---

## Wave 1 (depends on T01)

### T02: RRF fusion + text representation + kind boosting
**Files:** Create `crates/domain/src/analysis/search.rs`
**Traces to:** AC9 (text repr), AC11 (kind boosting)

- [ ] Step 1: Write tests for `rrf_merge` in `analysis/search.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_merge_single_list() {
        let lists = vec![vec![
            ("a".into(), 1.0), ("b".into(), 0.5),
        ]];
        let merged = rrf_merge(&lists, 60);
        assert_eq!(merged[0].0, "a");
        assert!(merged[0].1 > merged[1].1);
    }

    #[test]
    fn rrf_merge_two_lists_boosts_overlap() {
        let l1 = vec![("a".into(), 1.0), ("b".into(), 0.5)];
        let l2 = vec![("b".into(), 1.0), ("c".into(), 0.5)];
        let merged = rrf_merge(&[l1, l2], 60);
        // "b" appears in both lists → highest RRF score
        assert_eq!(merged[0].0, "b");
    }

    #[test]
    fn rrf_merge_empty_lists() {
        let merged = rrf_merge(&[], 60);
        assert!(merged.is_empty());
    }
}
```

- [ ] Step 2: Run `cargo test -p domain -- analysis::search` — verify FAIL (module empty).
- [ ] Step 3: Implement `rrf_merge`, `symbol_to_text`, `detect_kind_boost`:

```rust
use std::collections::HashMap;
use crate::model::*;

pub fn rrf_merge(lists: &[Vec<(String, f64)>], k: usize) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();
    for list in lists {
        for (rank, (qn, _)) in list.iter().enumerate() {
            *scores.entry(qn.clone()).or_default() += 1.0 / (k + rank + 1) as f64;
        }
    }
    let mut merged: Vec<_> = scores.into_iter().collect();
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    merged
}

/// Build NL text representation: "Function validate_token in auth, signature: ..., calls X, called by Y"
pub fn symbol_to_text(sym: &SymbolNode, edges: &[Edge]) -> String {
    let mut parts = vec![sym.kind.to_string(), sym.name.clone()];
    parts.push(format!("in {}", file_stem(&sym.location.file)));
    if let Some(sig) = &sym.signature {
        parts.push(format!("signature: {sig}"));
    }
    let calls: Vec<_> = edges.iter()
        .filter(|e| e.kind == EdgeKind::Calls && e.source == sym.qualified_name)
        .take(3).map(|e| short_name(&e.target)).collect();
    if !calls.is_empty() { parts.push(format!("calls {}", calls.join(", "))); }
    let callers: Vec<_> = edges.iter()
        .filter(|e| e.kind == EdgeKind::Calls && e.target == sym.qualified_name)
        .take(3).map(|e| short_name(&e.source)).collect();
    if !callers.is_empty() { parts.push(format!("called by {}", callers.join(", "))); }
    parts.join(", ")
}

fn file_stem(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string()
}

fn short_name(qualified: &str) -> String {
    qualified.rsplit("::").next().unwrap_or(qualified).to_string()
}

pub struct KindBoost { pub kind: SymbolKind, pub multiplier: f64 }

/// Detect query pattern and return kind boosts:
/// - PascalCase (first char uppercase, no underscores) → Class/Struct/Trait/Interface 1.5x
/// - snake_case (all lowercase with underscores) → Function/Method 1.5x
/// - Contains "::" → qualified-name exact match 2.0x
pub fn detect_kind_boost(query: &str) -> Vec<KindBoost> {
    let mut boosts = Vec::new();
    if query.contains("::") {
        // Qualified name pattern — no kind boost, handled via exact match in RRF
        return boosts;
    }
    let first = query.chars().next().unwrap_or('a');
    if first.is_uppercase() && !query.contains('_') {
        // PascalCase → boost struct-like kinds
        for kind in [SymbolKind::Struct, SymbolKind::Trait, SymbolKind::Interface] {
            boosts.push(KindBoost { kind, multiplier: 1.5 });
        }
    } else if query.contains('_') && query.chars().all(|c| c.is_lowercase() || c == '_') {
        // snake_case → boost function-like kinds
        for kind in [SymbolKind::Function, SymbolKind::Method] {
            boosts.push(KindBoost { kind, multiplier: 1.5 });
        }
    }
    boosts
}
```

- [ ] Step 4: Write tests for `symbol_to_text` (includes kind, name, file, signature, callers, callees) and `detect_kind_boost` (PascalCase, snake_case, `::` patterns).
- [ ] Step 5: Run `cargo test -p domain -- analysis::search` — verify PASS.
- [ ] Step 6: Commit: `feat(S04/T02): RRF fusion, text representation, kind boosting`

---

### T03: SQLite vector store
**Files:** Create `crates/storage/src/embedding_store.rs`, modify `crates/storage/src/lib.rs`
**Traces to:** AC14 (orphan cleanup), AC5 (search <100ms)

- [ ] Step 1: Write tests for `VectorStore` impl on `SqliteStore`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use domain::ports::VectorStore;

    #[test]
    fn store_and_retrieve_embeddings() { /* store entries, search_nearest returns them */ }

    #[test]
    fn cosine_similarity_ranking() { /* verify closer vectors rank higher */ }

    #[test]
    fn has_embeddings_false_when_empty() { /* ... */ }

    #[test]
    fn remove_embeddings_deletes_entries() { /* ... */ }

    #[test]
    fn cascade_delete_on_symbol_removal() { /* remove symbol, verify embedding gone */ }
}
```

- [ ] Step 2: Run `cargo test -p storage -- embedding_store` — verify FAIL.
- [ ] Step 3: Add `pub mod embedding_store;` to `crates/storage/src/lib.rs`. Implement `VectorStore` for `SqliteStore`:
  - Binary encode/decode: `pack_f32`/`unpack_f32` using `byteorder` or manual `[u8;4]` conversion
  - `store_embeddings`: INSERT OR REPLACE with binary vector blob
  - `search_nearest`: load all vectors for current provider, compute cosine similarity in Rust, sort, return top-N
  - `has_embeddings`: `SELECT COUNT(*) FROM embeddings > 0`
  - `remove_embeddings`: `DELETE FROM embeddings WHERE qualified_name IN (...)`
- [ ] Step 4: Run `cargo test -p storage -- embedding_store` — verify PASS.
- [ ] Step 5: Commit: `feat(S04/T03): SQLite vector store with cosine similarity`

---

### T04: Model download + cache management
**Files:** Create `crates/embeddings/src/download.rs`, modify `crates/embeddings/Cargo.toml`
**Traces to:** AC6 (auto-download + cache), AC16 (error on download failure)

- [ ] Step 1: Add deps to `crates/embeddings/Cargo.toml`: `ureq` (lightweight sync HTTP, ~5 transitive deps vs reqwest's ~30+), `sha2`, `tracing`, `thiserror`, `dirs` (for XDG cache).
- [ ] Step 2: Write tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: Tests that modify XDG_CACHE_HOME must use a serialization mutex
    // to prevent env var races. See existing pattern in crates/eval/ (commit bd76f12).

    #[test]
    fn model_dir_uses_xdg_cache() { /* verify path is $XDG_CACHE_HOME/code-graph/models/... */ }

    #[test]
    fn model_dir_falls_back_to_home_cache() { /* when XDG unset, uses ~/.cache */ }

    #[test]
    fn cached_model_skips_download() {
        // Create fake cache dir with model + tokenizer files, verify ensure_model returns
        // immediately without HTTP call. Test the cache-hit path only — no HTTP mock needed.
    }

    #[test]
    fn atomic_rename_prevents_partial_files() {
        // Test that download_to_tmp + rename logic works: create a .tmp file, verify
        // cleanup_partial_downloads() removes it. Tests the filesystem logic, not HTTP.
    }
}
```

- [ ] Step 3: Implement `download.rs`:
  - `model_cache_dir(model_name: &str) -> PathBuf`
  - `ensure_model(model_name: &str) -> Result<ModelFiles>` — downloads if not cached
  - `ModelFiles { model_path: PathBuf, tokenizer_path: PathBuf }`
  - Download to `.tmp` first, rename atomically on success (prevents partial files)
  - HuggingFace URLs: `https://huggingface.co/sentence-transformers/{model}/resolve/main/onnx/model_O4.onnx` and `.../tokenizer.json`
- [ ] Step 4: Run `cargo test -p embeddings -- download` — verify PASS.
- [ ] Step 5: Commit: `feat(S04/T04): model auto-download with XDG cache`

---

## Wave 2 (T05 depends on T04; T07 depends on T02+T03)

### T05: ONNX embedding provider
**Files:** Modify `crates/embeddings/src/lib.rs`, create `crates/embeddings/src/tokenizer.rs`, modify `crates/embeddings/Cargo.toml`
**Traces to:** AC1 (embeddings generation)

- [ ] Step 1: Add deps to `crates/embeddings/Cargo.toml`: `ort = "2.0.0-rc.12"`, `tokenizers = { version = "0.22.2", default-features = false }`. Verify `domain = { path = "../domain" }` already present from T01 Step 6.
- [ ] Step 2: Implement `tokenizer.rs`:
  - `load_tokenizer(path: &Path) -> Result<Tokenizer>` — load from JSON, configure truncation (256) and padding (BatchLongest)
  - `encode_batch(tokenizer: &Tokenizer, texts: &[String]) -> Result<BatchEncoding>` — returns `input_ids`, `attention_mask`, `token_type_ids` as `Vec<i64>`
- [ ] Step 3: Implement `OnnxEmbeddingProvider` in `lib.rs`:

```rust
pub struct OnnxEmbeddingProvider {
    session: Session,
    tokenizer: Tokenizer,
}

impl OnnxEmbeddingProvider {
    pub fn new(model_name: &str) -> Result<Self> {
        let files = download::ensure_model(model_name)?;
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(&files.model_path)?;
        let tokenizer = tokenizer::load_tokenizer(&files.tokenizer_path)?;
        Ok(Self { session, tokenizer })
    }
}

impl EmbeddingProvider for OnnxEmbeddingProvider {
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // tokenize → run session → mean pool → return
    }
    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        self.embed_batch(&[text.to_string()]).map(|v| v.into_iter().next().unwrap())
    }
    fn dimension(&self) -> usize { 384 }
}
```

- [ ] Step 4: Write tests (unit tests use mock; integration test with real ONNX gated behind `#[cfg(feature = "embeddings")]`):

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn mean_pooling_produces_correct_dim() { /* test with mock hidden states */ }

    #[test]
    fn embed_batch_returns_one_vec_per_input() { /* ... */ }
}
```

- [ ] Step 5: Run `cargo test -p embeddings` — verify PASS.
- [ ] Step 6: Commit: `feat(S04/T05): ONNX embedding provider with tokenizer`

---

### T07: Hybrid search in QueryUseCase
**Files:** Modify `crates/domain/src/use_cases/query.rs`, `crates/domain/src/test_support.rs`
**Traces to:** AC2 (transparent hybrid), AC3 (semantic-only/fts-only), AC5 (<100ms), AC13 (empty/graceful)

- [ ] Step 1: Add `InMemoryVectorStore` to `test_support.rs` for testing. Write tests:

```rust
#[test]
fn search_falls_back_to_fts_when_no_vector_store() { /* ... */ }

#[test]
fn search_uses_hybrid_when_vector_store_has_embeddings() { /* ... */ }

#[test]
fn search_semantic_only_skips_fts() { /* ... */ }

#[test]
fn search_fts_only_skips_vectors() { /* ... */ }

#[test]
fn search_empty_query_returns_empty() { /* ... */ }
```

- [ ] Step 2: Run `cargo test -p domain -- use_cases::query` — verify FAIL (no hybrid path yet).
- [ ] Step 3: Modify `QueryUseCase`. **Key architecture decision**: store `Option<Arc<dyn EmbeddingProvider>>` alongside `Option<Arc<dyn VectorStore>>` on the struct — do NOT use a closure parameter. This follows the same port pattern as every other trait in the codebase. `SearchMode` is defined in `model.rs` (added in T01).

```rust
use std::sync::Arc;
use crate::ports::{VectorStore, EmbeddingProvider};
use crate::analysis::search::{rrf_merge, detect_kind_boost};

pub struct QueryUseCase<S, I> {
    store: S,
    index: I,
    vector_store: Option<Arc<dyn VectorStore>>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
}

impl<S: GraphStore, I: SearchIndex> QueryUseCase<S, I> {
    pub fn new(store: S, index: I) -> Self {
        Self { store, index, vector_store: None, embedding_provider: None }
    }

    pub fn with_hybrid(
        store: S, index: I,
        vs: Arc<dyn VectorStore>,
        ep: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        Self { store, index, vector_store: Some(vs), embedding_provider: Some(ep) }
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        mode: SearchMode,
        config: &HybridSearchConfig,
    ) -> Result<Vec<SearchResult>> {
        match mode {
            SearchMode::FtsOnly => self.index.search(query, limit),
            SearchMode::SemanticOnly => {
                let ep = self.embedding_provider.as_ref()
                    .ok_or_else(|| CodeGraphError::Other("no embedding provider".into()))?;
                let vs = self.vector_store.as_ref()
                    .ok_or_else(|| CodeGraphError::Other("no vector store".into()))?;
                let qvec = ep.embed_query(query)?;
                let hits = vs.search_nearest(&qvec, limit)?;
                self.resolve_hits(hits, ScoreSource::Semantic)
            }
            SearchMode::Hybrid => {
                let fts_results = self.index.search(query, limit)?;
                match (&self.vector_store, &self.embedding_provider) {
                    (Some(vs), Some(ep)) => {
                        let qvec = ep.embed_query(query)?;
                        let vec_hits = vs.search_nearest(&qvec, limit)?;
                        let fts_list: Vec<_> = fts_results.iter()
                            .map(|r| (r.qualified_name.clone(), r.score)).collect();
                        let merged = rrf_merge(&[fts_list, vec_hits], config.rrf_k);
                        // Apply kind boosting, resolve to SearchResult with ScoreSource::Hybrid
                        self.resolve_merged(merged, limit, config)
                    }
                    _ => Ok(fts_results), // graceful fallback to FTS-only
                }
            }
        }
    }
}
```

- [ ] Step 4: Run `cargo test -p domain -- use_cases::query` — verify PASS.
- [ ] Step 5: Commit: `feat(S04/T07): hybrid search with RRF fusion in QueryUseCase`

---

## Wave 3 (T06 depends on T02+T03+T05, not T07)

### T06: EmbedUseCase orchestration
**Files:** Create `crates/domain/src/use_cases/embed.rs`
**Traces to:** AC1 (embed generation), AC8 (incremental), AC10 (edge-change re-embed), AC14 (orphan cleanup)

- [ ] Step 1: Write tests:

```rust
#[test]
fn embed_all_embeds_non_file_symbols() { /* mock provider, verify embed_batch called */ }

#[test]
fn embed_incremental_skips_unchanged() { /* same text_hash → skip */ }

#[test]
fn edge_change_triggers_reembed() { /* change caller → text_hash changes → re-embedded */ }

#[test]
fn cleanup_orphans_removes_stale_entries() { /* delete symbol → embedding should be gone */ }
```

- [ ] Step 2: Run `cargo test -p domain -- use_cases::embed` — verify FAIL.
- [ ] Step 3: Implement `EmbedUseCase`:

```rust
pub struct EmbedUseCase<S: GraphStore, E: EmbeddingProvider, V: VectorStore> {
    store: S,
    provider: E,
    vector_store: V,
}

impl<S: GraphStore, E: EmbeddingProvider, V: VectorStore> EmbedUseCase<S, E, V> {
    pub fn embed_all(&self, config: &EmbeddingConfig) -> Result<EmbedStats> {
        let symbols = self.store.all_symbols()?;
        let edges = self.store.all_edges()?;
        let edge_map = build_edge_map(&edges); // HashMap<String, Vec<Edge>>

        // all_symbols() already returns only SymbolNode (not FileNode), so no File filtering needed.
        // Embed ALL symbol kinds — spec AC1 says "all non-File symbols".
        let mut to_embed = Vec::new();
        for sym in &symbols {
            let text = symbol_to_text(sym, edge_map.get(&sym.qualified_name).unwrap_or(&vec![]));
            let hash = sha256(&text);
            // Check if hash changed vs stored
            to_embed.push((sym.qualified_name.clone(), text, hash));
        }

        // Batch embed
        for chunk in to_embed.chunks(config.batch_size) {
            let texts: Vec<String> = chunk.iter().map(|(_, t, _)| t.clone()).collect();
            let vectors = self.provider.embed_batch(&texts)?;
            let entries: Vec<EmbeddingEntry> = chunk.iter().zip(vectors)
                .map(|((qn, _, hash), vec)| EmbeddingEntry {
                    qualified_name: qn.clone(), vector: vec, text_hash: hash.clone(),
                }).collect();
            self.vector_store.store_embeddings(&entries)?;
        }

        Ok(EmbedStats { /* ... */ })
    }

    pub fn cleanup_orphans(&self) -> Result<usize> { /* ... */ }
}
```

- [ ] Step 4: Run `cargo test -p domain -- use_cases::embed` — verify PASS.
- [ ] Step 5: Commit: `feat(S04/T06): EmbedUseCase with incremental embedding + orphan cleanup`

---

## Wave 4 (depends on Wave 3)

### T08: CLI commands + config + output
**Files:** Modify `crates/cli/src/commands/mod.rs`, `crates/cli/src/commands/search.rs`, `crates/cli/src/commands/index.rs`, `crates/cli/src/config.rs`, `crates/cli/src/output.rs`, `crates/cli/Cargo.toml`
**Traces to:** AC3 (flag conflict error), AC7 (config override), AC12 (output formats)

- [ ] Step 1: Add `EmbeddingsCliConfig` to `crates/cli/src/config.rs`:

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmbeddingsCliConfig {
    pub enabled: Option<bool>,
    pub model: Option<String>,
    pub batch_size: Option<usize>,
}

// Extend SearchConfig:
pub struct SearchConfig {
    pub max_results: Option<usize>,
    pub rrf_k: Option<usize>,
    pub kind_boost: Option<bool>,
}

// Add to CodeGraphConfig:
pub embeddings: Option<EmbeddingsCliConfig>,
```

- [ ] Step 2: Add `--embed` and `--embed-model` to `IndexArgs`, `--semantic-only` and `--fts-only` to `SearchArgs` in `mod.rs`. Add mutual exclusion validation for search flags.

- [ ] Step 3: Update `run_search` in `search.rs`. The `EmbeddingProvider` is wired into `QueryUseCase` via the struct (not a closure):

```rust
pub fn run_search(args: &SearchArgs, output_format: OutputFormat) -> Result<()> {
    let (store, root) = open_graph()?;
    let config = load_config(&root)?;

    let mode = match (args.semantic_only, args.fts_only) {
        (true, true) => return Err(CodeGraphError::Other(
            "--semantic-only and --fts-only are mutually exclusive".into())),
        (true, false) => SearchMode::SemanticOnly,
        (false, true) => SearchMode::FtsOnly,
        (false, false) => SearchMode::Hybrid,
    };

    // Wire VectorStore + EmbeddingProvider into QueryUseCase
    let vs: Arc<dyn VectorStore> = Arc::new(store.clone());
    let uc = if vs.has_embeddings() {
        #[cfg(feature = "embeddings")]
        {
            let model = config.embeddings.as_ref()
                .and_then(|e| e.model.clone())
                .unwrap_or_else(|| "all-MiniLM-L6-v2".into());
            let ep: Arc<dyn EmbeddingProvider> = Arc::new(
                OnnxEmbeddingProvider::new(&model)?);
            QueryUseCase::with_hybrid(store.clone(), store, vs, ep)
        }
        #[cfg(not(feature = "embeddings"))]
        QueryUseCase::new(store.clone(), store)
    } else {
        QueryUseCase::new(store.clone(), store)
    };

    let hybrid_config = HybridSearchConfig::from_cli_config(&config);
    let results = uc.search(&args.query, args.limit, mode, &hybrid_config)?;
    print(&results, output_format);
    Ok(())
}
```

- [ ] Step 4: Update `run_index` in `index.rs` to handle `--embed` flag.

- [ ] Step 5: Update `output.rs` — `score_source` is auto-handled by serde (`skip_serializing_if`). Update compact/table format if needed.

- [ ] Step 6: Write config parsing test for embeddings section.
- [ ] Step 7: Run `cargo test -p cli` — verify PASS.
- [ ] Step 8: Commit: `feat(S04/T08): CLI embed flag, hybrid search flags, config`

---

## Wave 5 (depends on Wave 4)

### T09: Eval dataset + harness extensions
**Files:** Modify `crates/eval/src/runner.rs`, `crates/eval/src/dataset.rs`, `crates/eval/src/report.rs`, create `eval/suites/search/queries/rust_semantic.json`
**Traces to:** AC4 (MRR >= 0.60)

- [ ] Step 1: Add `category: Option<String>` field to `SearchQuery` in `dataset.rs`.
- [ ] Step 2: Create `eval/suites/search/queries/rust_semantic.json` with 40 queries:
  - 15 exact-name (Rust symbols from ripgrep/tokio)
  - 15 semantic ("authentication handler", "async task spawning", etc.)
  - 10 partial-name ("search build", "walk ignore")
- [ ] Step 3: Add `SearchMode` support to eval runner — run each query in FTS5-only, semantic-only, and hybrid modes.
- [ ] Step 4: Extend `report.rs` with per-category MRR breakdown.
- [ ] Step 5: Run eval suite, measure MRR. Tune RRF k parameter and kind boost multipliers if needed.
- [ ] Step 6: Run `cargo test -p eval` — verify PASS.
- [ ] Step 7: Commit: `feat(S04/T09): eval dataset with semantic queries + per-mode MRR`

---

### T10: CI workflow updates
**Files:** Modify `.github/workflows/ci.yml`
**Traces to:** AC15 (cross-platform CI)

- [ ] Step 1: Add embeddings feature matrix to CI:

```yaml
  test-embeddings:
    name: Test embeddings (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            ~/.cache/ort
            target
          key: ${{ runner.os }}-cargo-embeddings-${{ hashFiles('**/Cargo.lock') }}
      - run: cargo test -p embeddings
      - run: cargo test -p cli --features embeddings
```

- [ ] Step 2: Verify CI passes on both platforms.
- [ ] Step 3: Commit: `chore(S04/T10): CI workflow for embeddings feature`

---

## Dependency Graph

```
T01 (foundation)
 ├── T02 (RRF + text repr)
 ├── T03 (SQLite vector store)
 └── T04 (model download)
      └── T05 (ONNX provider)
           │
T02 + T03 ─┤
           └── T07 (hybrid QueryUseCase)
                │
T02 + T03 + T05 → T06 (EmbedUseCase)
                    │
T06 + T07 ────────→ T08 (CLI)
                      │
                      ├── T09 (eval)
                      └── T10 (CI)
```

## AC Traceability Matrix

| AC | Tasks |
|----|-------|
| AC1 (embed generates) | T05, T06 |
| AC2 (transparent hybrid) | T07, T08 |
| AC3 (flag restriction) | T07, T08 |
| AC4 (MRR >= 0.60) | T09 |
| AC5 (<100ms) | T03, T07 |
| AC6 (auto-download) | T04 |
| AC7 (config override) | T08 |
| AC8 (incremental) | T06 |
| AC9 (text representation) | T02 |
| AC10 (edge-change re-embed) | T06 |
| AC11 (kind boosting) | T02, T07 |
| AC12 (output formats) | T01, T08 |
| AC13 (empty/graceful) | T07 |
| AC14 (orphan cleanup) | T03, T06 |
| AC15 (CI cross-platform) | T10 |
| AC16 (download error) | T04 |
| AC17 (schema migration) | T01 |
