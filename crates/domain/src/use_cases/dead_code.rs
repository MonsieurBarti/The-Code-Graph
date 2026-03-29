use crate::analysis::dead_code::detect_dead_code;
use crate::error::Result;
use crate::model::*;
use crate::ports::GraphStore;

pub struct DeadCodeUseCase<S> {
    store: S,
}

impl<S: GraphStore> DeadCodeUseCase<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Run dead code analysis: load all symbols + edges, detect dead code.
    pub fn analyze(&self, config: &DeadCodeConfig) -> Result<DeadCodeAnalysis> {
        let symbols = self.store.all_symbols()?;
        let edges = self.store.all_edges()?;
        Ok(detect_dead_code(&symbols, &edges, config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, EdgeKind, Location, SymbolKind, SymbolNode, Visibility};
    use crate::test_support::InMemoryGraphStore;

    fn build_store() -> InMemoryGraphStore {
        let mut store = InMemoryGraphStore::new();

        // Unused function — should be dead
        store.insert_symbol(SymbolNode {
            name: "orphan_fn".into(),
            qualified_name: "src/old.rs::orphan_fn".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/old.rs".into(),
                line_start: 10,
                line_end: 20,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Private,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });

        // Used function — should be alive
        store.insert_symbol(SymbolNode {
            name: "active_fn".into(),
            qualified_name: "src/core.rs::active_fn".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/core.rs".into(),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });

        // Edge: something calls active_fn
        store.insert_edge(Edge {
            kind: EdgeKind::Calls,
            source: "src/main.rs::main".into(),
            target: "src/core.rs::active_fn".into(),
            metadata: None,
        });

        store
    }

    #[test]
    fn use_case_detects_dead_code() {
        let store = build_store();
        let uc = DeadCodeUseCase::new(store);
        let result = uc.analyze(&DeadCodeConfig::default()).unwrap();

        assert_eq!(result.summary.total_symbols, 2);
        assert_eq!(result.dead_symbols.len(), 1);
        assert_eq!(
            result.dead_symbols[0].qualified_name,
            "src/old.rs::orphan_fn"
        );
    }

    #[test]
    fn use_case_with_include_tests() {
        let mut store = build_store();
        store.insert_symbol(SymbolNode {
            name: "old_test".into(),
            qualified_name: "tests/old.rs::old_test".into(),
            kind: SymbolKind::Test,
            location: Location {
                file: "tests/old.rs".into(),
                line_start: 1,
                line_end: 5,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Private,
            is_exported: false,
            is_async: false,
            is_test: true,
            decorators: vec![],
            signature: None,
        });

        // Default: test excluded
        let uc = DeadCodeUseCase::new(store.clone());
        let result = uc.analyze(&DeadCodeConfig::default()).unwrap();
        assert_eq!(result.dead_symbols.len(), 1); // only orphan_fn

        // With include_tests: test also flagged
        let config = DeadCodeConfig {
            include_tests: true,
            ..DeadCodeConfig::default()
        };
        let result = uc.analyze(&config).unwrap();
        assert_eq!(result.dead_symbols.len(), 2);
    }
}
