use crate::analysis::search::{detect_kind_boost, rrf_merge};
use crate::error::Result;
use crate::model::*;
use crate::ports::{EmbeddingProvider, GraphStore, SearchIndex, VectorStore};
use std::sync::Arc;

pub struct QueryUseCase<S, I> {
    store: S,
    index: I,
    vector_store: Option<Arc<dyn VectorStore>>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
}

impl<S: GraphStore, I: SearchIndex> QueryUseCase<S, I> {
    pub fn new(store: S, index: I) -> Self {
        Self {
            store,
            index,
            vector_store: None,
            embedding_provider: None,
        }
    }

    pub fn with_hybrid(
        store: S,
        index: I,
        vector_store: Option<Arc<dyn VectorStore>>,
        embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    ) -> Self {
        Self {
            store,
            index,
            vector_store,
            embedding_provider,
        }
    }

    pub fn find(&self, pattern: &str) -> Result<Vec<SymbolNode>> {
        self.store.find_by_name(pattern)
    }

    pub fn refs(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        let edges = self.store.get_edges_to(qualified_name)?;
        Ok(edges
            .into_iter()
            .map(|e| Reference {
                symbol: e.source,
                edge_kind: e.kind,
                location: None,
            })
            .collect())
    }

    pub fn callers(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        let edges = self.store.get_edges_to(qualified_name)?;
        Ok(edges
            .into_iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .map(|e| Reference {
                symbol: e.source,
                edge_kind: e.kind,
                location: None,
            })
            .collect())
    }

    pub fn callees(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        let edges = self.store.get_edges_from(qualified_name)?;
        Ok(edges
            .into_iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .map(|e| Reference {
                symbol: e.target,
                edge_kind: e.kind,
                location: None,
            })
            .collect())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.index.search(query, limit)
    }

    /// Hybrid search combining FTS and/or semantic vector search with RRF fusion.
    /// Falls back to FTS when `mode == Hybrid` but no vector store is available.
    pub fn hybrid_search(
        &self,
        query: &str,
        limit: usize,
        mode: SearchMode,
        config: &HybridSearchConfig,
    ) -> Result<Vec<SearchResult>> {
        if query.is_empty() {
            return Ok(vec![]);
        }

        // Collect FTS results unless semantic-only
        let fts_results: Vec<(String, f64)> = if mode != SearchMode::SemanticOnly {
            self.index
                .search(query, limit)?
                .into_iter()
                .map(|r| (r.qualified_name, r.score))
                .collect()
        } else {
            vec![]
        };

        // Collect vector results unless FTS-only, and only when a vector store is present
        let vec_results: Vec<(String, f64)> = if mode != SearchMode::FtsOnly {
            if let (Some(vs), Some(ep)) =
                (self.vector_store.as_ref(), self.embedding_provider.as_ref())
            {
                if vs.has_embeddings() {
                    let query_vec = ep.embed_query(query)?;
                    vs.search_nearest(&query_vec, limit)?
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Build the merged ranked list
        let merged: Vec<(String, f64)> = match mode {
            SearchMode::FtsOnly => fts_results,
            SearchMode::SemanticOnly => vec_results,
            SearchMode::Hybrid => {
                if vec_results.is_empty() {
                    // Graceful fallback: no vector store / no embeddings → return FTS
                    fts_results
                } else {
                    rrf_merge(&[fts_results, vec_results], config.rrf_k)
                }
            }
        };

        // Compute kind boosts once
        let kind_boosts = if config.kind_boost {
            detect_kind_boost(query)
        } else {
            vec![]
        };

        // Resolve qualified names to SearchResult, applying kind boost
        let mut results: Vec<SearchResult> = merged
            .into_iter()
            .take(limit)
            .filter_map(|(qn, mut score)| {
                let sym = self.store.get_symbol(&qn).ok().flatten()?;
                if !kind_boosts.is_empty() {
                    for kb in &kind_boosts {
                        if sym.kind == kb.kind {
                            score *= kb.multiplier;
                        }
                    }
                }
                Some(SearchResult {
                    qualified_name: sym.qualified_name.clone(),
                    name: sym.name.clone(),
                    kind: sym.kind,
                    file_path: sym.location.file.clone(),
                    score,
                    score_source: Some(match mode {
                        SearchMode::FtsOnly => ScoreSource::Fts5,
                        SearchMode::SemanticOnly => ScoreSource::Semantic,
                        SearchMode::Hybrid => ScoreSource::Hybrid,
                    }),
                })
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    pub fn stats(&self) -> Result<GraphStats> {
        self.store.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{InMemoryEmbeddingProvider, InMemoryGraphStore, InMemoryVectorStore};
    use std::sync::Arc;

    fn make_symbol(name: &str) -> SymbolNode {
        SymbolNode {
            name: name.into(),
            qualified_name: format!("test.rs::{name}"),
            kind: SymbolKind::Function,
            location: Location {
                file: "test.rs".into(),
                line_start: 1,
                line_end: 5,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }
    }

    #[test]
    fn find_exact_match() {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(make_symbol("foo"));
        let uc = QueryUseCase::new(store.clone(), store);
        let results = uc.find("foo").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "foo");
    }

    #[test]
    fn find_prefix_fallback() {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(make_symbol("foobar"));
        let uc = QueryUseCase::new(store.clone(), store);
        let results = uc.find("foo").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "foobar");
    }

    #[test]
    fn find_no_match_returns_empty() {
        let store = InMemoryGraphStore::new();
        let uc = QueryUseCase::new(store.clone(), store);
        let results = uc.find("bar").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn find_exact_takes_priority_over_prefix() {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(make_symbol("foo"));
        store.insert_symbol(make_symbol("foobar"));
        let uc = QueryUseCase::new(store.clone(), store);
        let results = uc.find("foo").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "foo");
    }

    // -----------------------------------------------------------------------
    // Hybrid search tests
    // -----------------------------------------------------------------------

    #[test]
    fn search_falls_back_to_fts_when_no_vector_store() {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(make_symbol("foo"));
        let ep: Arc<dyn crate::ports::EmbeddingProvider> =
            Arc::new(InMemoryEmbeddingProvider::new(4));
        let uc = QueryUseCase::with_hybrid(store.clone(), store, None, Some(ep));
        let cfg = HybridSearchConfig::default();
        let results = uc
            .hybrid_search("foo", 10, SearchMode::Hybrid, &cfg)
            .unwrap();
        // Falls back to FTS — must still find the symbol
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "foo");
    }

    #[test]
    fn search_uses_hybrid_when_vector_store_has_embeddings() {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(make_symbol("foo"));
        store.insert_symbol(make_symbol("bar"));

        let vs = Arc::new(InMemoryVectorStore::new());
        // Seed the vector store with embeddings for both symbols
        vs.store_embeddings(&[
            EmbeddingEntry {
                qualified_name: "test.rs::foo".into(),
                vector: vec![1.0, 0.0, 0.0, 0.0],
                text_hash: "h1".into(),
            },
            EmbeddingEntry {
                qualified_name: "test.rs::bar".into(),
                vector: vec![0.0, 1.0, 0.0, 0.0],
                text_hash: "h2".into(),
            },
        ])
        .unwrap();

        let ep: Arc<dyn crate::ports::EmbeddingProvider> =
            Arc::new(InMemoryEmbeddingProvider::new(4));
        let vs_arc: Arc<dyn crate::ports::VectorStore> = vs;
        let uc = QueryUseCase::with_hybrid(store.clone(), store, Some(vs_arc), Some(ep));
        let cfg = HybridSearchConfig::default();
        let results = uc
            .hybrid_search("foo", 10, SearchMode::Hybrid, &cfg)
            .unwrap();
        // Both symbols should appear in merged results
        assert!(!results.is_empty());
        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"foo"));
    }

    #[test]
    fn search_semantic_only_skips_fts() {
        let mut store = InMemoryGraphStore::new();
        // "alpha" does NOT match "foo" text-search wise, but is the only symbol in vector store
        store.insert_symbol(make_symbol("alpha"));

        let vs = Arc::new(InMemoryVectorStore::new());
        vs.store_embeddings(&[EmbeddingEntry {
            qualified_name: "test.rs::alpha".into(),
            vector: vec![1.0, 0.0, 0.0, 0.0],
            text_hash: "h1".into(),
        }])
        .unwrap();

        let ep: Arc<dyn crate::ports::EmbeddingProvider> =
            Arc::new(InMemoryEmbeddingProvider::new(4));
        let vs_arc: Arc<dyn crate::ports::VectorStore> = vs;
        let uc = QueryUseCase::with_hybrid(store.clone(), store, Some(vs_arc), Some(ep));
        let cfg = HybridSearchConfig::default();
        // SemanticOnly: only vector results are returned
        let results = uc
            .hybrid_search("foo", 10, SearchMode::SemanticOnly, &cfg)
            .unwrap();
        // "alpha" found via vector, not via FTS
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "alpha");
    }

    #[test]
    fn search_fts_only_skips_vectors() {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(make_symbol("foo"));

        // Vector store has "bar" — should NOT appear in FtsOnly results
        let vs = Arc::new(InMemoryVectorStore::new());
        vs.store_embeddings(&[EmbeddingEntry {
            qualified_name: "test.rs::bar".into(),
            vector: vec![1.0, 0.0, 0.0, 0.0],
            text_hash: "h1".into(),
        }])
        .unwrap();

        let ep: Arc<dyn crate::ports::EmbeddingProvider> =
            Arc::new(InMemoryEmbeddingProvider::new(4));
        let vs_arc: Arc<dyn crate::ports::VectorStore> = vs;
        let uc = QueryUseCase::with_hybrid(store.clone(), store, Some(vs_arc), Some(ep));
        let cfg = HybridSearchConfig::default();
        let results = uc
            .hybrid_search("foo", 10, SearchMode::FtsOnly, &cfg)
            .unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "foo");
        assert!(results.iter().all(|r| r.name != "bar"));
    }

    #[test]
    fn search_empty_query_returns_empty() {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(make_symbol("foo"));
        let uc = QueryUseCase::new(store.clone(), store);
        let cfg = HybridSearchConfig::default();
        let results = uc.hybrid_search("", 10, SearchMode::Hybrid, &cfg).unwrap();
        assert!(results.is_empty());
    }
}
