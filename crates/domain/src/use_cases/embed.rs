use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};

use crate::analysis::search::symbol_to_text;
use crate::error::Result;
use crate::model::{Edge, EmbedStats, EmbeddingConfig, EmbeddingEntry};
use crate::ports::{EmbeddingProvider, GraphStore, VectorStore};

// ---------------------------------------------------------------------------
// EmbedUseCase
// ---------------------------------------------------------------------------

pub struct EmbedUseCase<S: GraphStore, E: EmbeddingProvider, V: VectorStore> {
    store: S,
    provider: E,
    vector_store: V,
}

impl<S: GraphStore, E: EmbeddingProvider, V: VectorStore> EmbedUseCase<S, E, V> {
    pub fn new(store: S, provider: E, vector_store: V) -> Self {
        Self {
            store,
            provider,
            vector_store,
        }
    }

    /// Embed all symbols, skipping those whose text representation is unchanged.
    pub fn embed_all(&self, config: &EmbeddingConfig) -> Result<EmbedStats> {
        let symbols = self.store.all_symbols()?;
        let edges = self.store.all_edges()?;
        let edge_map = build_edge_map(&edges);

        // Build a map of already-stored text hashes for incremental skipping.
        let stored: HashMap<String, String> =
            self.vector_store.get_stored_hashes()?.into_iter().collect();

        let mut to_embed: Vec<(String, String, String)> = Vec::new(); // (qn, text, hash)
        let mut skipped = 0usize;

        for sym in &symbols {
            let sym_edges = edge_map
                .get(&sym.qualified_name)
                .cloned()
                .unwrap_or_default();
            let text = symbol_to_text(sym, &sym_edges);
            let hash = sha256_hex(&text);

            if stored
                .get(&sym.qualified_name)
                .map(|h| h == &hash)
                .unwrap_or(false)
            {
                skipped += 1;
                continue;
            }
            to_embed.push((sym.qualified_name.clone(), text, hash));
        }

        let mut embedded = 0usize;
        for chunk in to_embed.chunks(config.batch_size) {
            let texts: Vec<String> = chunk.iter().map(|(_, t, _)| t.clone()).collect();
            let vectors = self.provider.embed_batch(&texts)?;
            let entries: Vec<EmbeddingEntry> = chunk
                .iter()
                .zip(vectors)
                .map(|((qn, _, hash), vec)| EmbeddingEntry {
                    qualified_name: qn.clone(),
                    vector: vec,
                    text_hash: hash.clone(),
                })
                .collect();
            self.vector_store.store_embeddings(&entries)?;
            embedded += entries.len();
        }

        Ok(EmbedStats {
            total_symbols: symbols.len(),
            embedded,
            skipped,
            removed: 0,
        })
    }

    /// Remove embeddings whose qualified name no longer exists in the graph.
    pub fn cleanup_orphans(&self) -> Result<usize> {
        let symbols: HashSet<String> = self
            .store
            .all_symbols()?
            .iter()
            .map(|s| s.qualified_name.clone())
            .collect();

        let stored = self.vector_store.get_stored_hashes()?;
        let orphans: Vec<&str> = stored
            .iter()
            .filter(|(qn, _)| !symbols.contains(qn.as_str()))
            .map(|(qn, _)| qn.as_str())
            .collect();

        let count = orphans.len();
        if !orphans.is_empty() {
            self.vector_store.remove_embeddings(&orphans)?;
        }
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_edge_map(edges: &[Edge]) -> HashMap<String, Vec<Edge>> {
    let mut map: HashMap<String, Vec<Edge>> = HashMap::new();
    for edge in edges {
        map.entry(edge.source.clone())
            .or_default()
            .push(edge.clone());
        map.entry(edge.target.clone())
            .or_default()
            .push(edge.clone());
    }
    map
}

fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::model::*;
    use crate::test_support::*;

    fn setup() -> (
        InMemoryGraphStore,
        InMemoryEmbeddingProvider,
        Arc<InMemoryVectorStore>,
    ) {
        let store = InMemoryGraphStore::new();
        let provider = InMemoryEmbeddingProvider::new(4);
        let vs = Arc::new(InMemoryVectorStore::new());
        (store, provider, vs)
    }

    fn make_symbol(name: &str) -> SymbolNode {
        SymbolNode {
            name: name.to_string(),
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
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }
    }

    #[test]
    fn embed_all_embeds_symbols() {
        let (mut store, provider, vs) = setup();
        store.insert_symbol(make_symbol("foo"));
        store.insert_symbol(make_symbol("bar"));
        let uc = EmbedUseCase::new(store, provider, vs);
        let stats = uc.embed_all(&EmbeddingConfig::default()).unwrap();
        assert_eq!(stats.total_symbols, 2);
        assert_eq!(stats.embedded, 2);
        assert_eq!(stats.skipped, 0);
    }

    #[test]
    fn embed_incremental_skips_unchanged() {
        let (mut store, provider, vs) = setup();
        store.insert_symbol(make_symbol("foo"));
        let uc = EmbedUseCase::new(store, provider, Arc::clone(&vs));
        // First run
        let stats1 = uc.embed_all(&EmbeddingConfig::default()).unwrap();
        assert_eq!(stats1.embedded, 1);
        // Second run — same symbols, same text → should skip
        let stats2 = uc.embed_all(&EmbeddingConfig::default()).unwrap();
        assert_eq!(stats2.skipped, 1);
        assert_eq!(stats2.embedded, 0);
    }

    #[test]
    fn edge_change_triggers_reembed() {
        let (mut store, provider, vs) = setup();
        store.insert_symbol(make_symbol("foo"));
        store.insert_symbol(make_symbol("bar"));

        let uc = EmbedUseCase::new(store.clone(), provider.clone(), Arc::clone(&vs));
        let _ = uc.embed_all(&EmbeddingConfig::default()).unwrap();

        // Add an edge: foo calls bar — changes foo's text representation
        store.insert_edge(Edge {
            kind: EdgeKind::Calls,
            source: "test.rs::foo".into(),
            target: "test.rs::bar".into(),
            metadata: None,
        });

        // Re-embed with the updated store but same vector store
        let uc2 = EmbedUseCase::new(store, provider, Arc::clone(&vs));
        let stats2 = uc2.embed_all(&EmbeddingConfig::default()).unwrap();
        // foo's text changed (now includes "calls bar"), bar's callers changed too
        assert!(stats2.embedded > 0);
    }

    #[test]
    fn cleanup_orphans_removes_stale() {
        let (mut store, provider, vs) = setup();
        store.insert_symbol(make_symbol("foo"));
        store.insert_symbol(make_symbol("bar"));

        let uc = EmbedUseCase::new(store, provider.clone(), Arc::clone(&vs));
        uc.embed_all(&EmbeddingConfig::default()).unwrap();
        assert_eq!(vs.count().unwrap(), 2);

        // Create a new store with only "foo" — "bar" becomes an orphan
        let mut store2 = InMemoryGraphStore::new();
        store2.insert_symbol(make_symbol("foo"));

        let uc2 = EmbedUseCase::new(store2, provider, Arc::clone(&vs));
        let removed = uc2.cleanup_orphans().unwrap();
        assert_eq!(removed, 1);
        assert_eq!(vs.count().unwrap(), 1);
    }

    #[test]
    fn embed_empty_store_returns_zero() {
        let (store, provider, vs) = setup();
        let uc = EmbedUseCase::new(store, provider, vs);
        let stats = uc.embed_all(&EmbeddingConfig::default()).unwrap();
        assert_eq!(stats.total_symbols, 0);
        assert_eq!(stats.embedded, 0);
    }
}
