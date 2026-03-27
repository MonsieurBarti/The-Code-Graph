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

    pub fn find(&self, pattern: &str) -> Result<Vec<SymbolNode>> {
        self.store.find_by_name(pattern)
    }

    pub fn refs(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        let edges = self.store.get_edges_to(qualified_name)?;
        Ok(edges.into_iter().map(|e| Reference {
            symbol: e.source,
            edge_kind: e.kind,
            location: None,
        }).collect())
    }

    pub fn callers(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        let edges = self.store.get_edges_to(qualified_name)?;
        Ok(edges.into_iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .map(|e| Reference { symbol: e.source, edge_kind: e.kind, location: None })
            .collect())
    }

    pub fn callees(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        let edges = self.store.get_edges_from(qualified_name)?;
        Ok(edges.into_iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .map(|e| Reference { symbol: e.target, edge_kind: e.kind, location: None })
            .collect())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.index.search(query, limit)
    }

    pub fn stats(&self) -> Result<GraphStats> {
        self.store.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryGraphStore;

    fn make_symbol(name: &str) -> SymbolNode {
        SymbolNode {
            name: name.into(),
            qualified_name: format!("test.rs::{name}"),
            kind: SymbolKind::Function,
            location: Location { file: "test.rs".into(), line_start: 1, line_end: 5, col_start: 0, col_end: 0 },
            visibility: Visibility::Public,
            is_exported: false, is_async: false, is_test: false,
            decorators: vec![], signature: None,
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
}
