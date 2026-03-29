use crate::analysis::community::detect_communities;
use crate::error::Result;
use crate::model::{Community, CommunityAnalysis, CommunityConfig};
use crate::ports::GraphStore;

pub struct CommunityUseCase<S> {
    store: S,
}

impl<S: GraphStore> CommunityUseCase<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn analyze(&self, config: &CommunityConfig) -> Result<CommunityAnalysis> {
        let symbols = self.store.all_symbols()?;
        let edges = self.store.all_edges()?;
        Ok(detect_communities(&symbols, &edges, config))
    }

    pub fn community_of(
        &self,
        symbol: &str,
        config: &CommunityConfig,
    ) -> Result<Option<Community>> {
        let analysis = self.analyze(config)?;
        Ok(analysis
            .communities
            .into_iter()
            .find(|c| c.members.contains(&symbol.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::test_support::InMemoryGraphStore;

    fn build_test_store() -> InMemoryGraphStore {
        let mut store = InMemoryGraphStore::new();
        for i in 0..3 {
            store.insert_symbol(SymbolNode {
                name: format!("a{i}"),
                qualified_name: format!("src/auth.rs::a{i}"),
                kind: SymbolKind::Function,
                location: Location {
                    file: "src/auth.rs".into(),
                    line_start: i * 10 + 1,
                    line_end: i * 10 + 10,
                    col_start: 0,
                    col_end: 0,
                },
                visibility: Visibility::Public,
                is_exported: true,
                is_async: false,
                is_test: false,
                decorators: vec![],
                signature: None,
            });
        }
        for i in 0..3 {
            for j in (i + 1)..3 {
                store.insert_edge(Edge {
                    kind: EdgeKind::Calls,
                    source: format!("src/auth.rs::a{i}"),
                    target: format!("src/auth.rs::a{j}"),
                    metadata: None,
                });
            }
        }
        store
    }

    #[test]
    fn analyze_returns_communities() {
        let store = build_test_store();
        let uc = CommunityUseCase::new(store);
        let result = uc.analyze(&CommunityConfig::default()).unwrap();
        assert!(result.stats.count > 0 || result.stats.isolated_nodes > 0);
    }

    #[test]
    fn community_of_finds_symbol() {
        let store = build_test_store();
        let uc = CommunityUseCase::new(store);
        let c = uc
            .community_of("src/auth.rs::a0", &CommunityConfig::default())
            .unwrap();
        assert!(c.is_some());
        assert!(c.unwrap().members.contains(&"src/auth.rs::a0".to_string()));
    }

    #[test]
    fn community_of_returns_none_for_unknown() {
        let store = build_test_store();
        let uc = CommunityUseCase::new(store);
        let c = uc
            .community_of("nonexistent::symbol", &CommunityConfig::default())
            .unwrap();
        assert!(c.is_none());
    }
}
