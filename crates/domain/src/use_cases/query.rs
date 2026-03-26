use crate::error::Result;
use crate::model::*;
use crate::ports::{GraphStore, SearchIndex};

pub struct QueryUseCase<S, I> {
    store: S,
    index: I,
}

impl<S: GraphStore, I: SearchIndex> QueryUseCase<S, I> {
    pub fn new(store: S, index: I) -> Self {
        Self { store, index }
    }

    pub fn find(&self, qualified_name: &str) -> Result<Option<SymbolNode>> {
        self.store.get_symbol(qualified_name)
    }

    pub fn refs(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        let edges = self.store.get_edges_to(qualified_name)?;
        Ok(edges.into_iter().map(|e| Reference {
            source: e.source,
            edge_kind: e.kind,
            location: None,
        }).collect())
    }

    pub fn callers(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        let edges = self.store.get_edges_to(qualified_name)?;
        Ok(edges.into_iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .map(|e| Reference { source: e.source, edge_kind: e.kind, location: None })
            .collect())
    }

    pub fn callees(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        let edges = self.store.get_edges_from(qualified_name)?;
        Ok(edges.into_iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .map(|e| Reference { source: e.target, edge_kind: e.kind, location: None })
            .collect())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.index.search(query, limit)
    }

    pub fn stats(&self) -> Result<GraphStats> {
        self.store.stats()
    }
}
