use domain::error::Result;
use domain::model::*;
use domain::ports::SearchIndex;

use crate::mapping::*;
use crate::SqliteStore;

impl SearchIndex for SqliteStore {
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        if query.is_empty() {
            return Ok(vec![]);
        }

        let conn = self.conn()?;

        // Quote the query to prevent FTS5 syntax injection, then append * for prefix matching
        let sanitized = query.replace('"', "\"\"");
        let fts_query = format!("\"{sanitized}\"*");

        let mut stmt = conn
            .prepare_cached(
                "SELECT s.qualified_name, s.name, s.kind, s.file_path, rank
                 FROM symbols_fts
                 JOIN symbols s ON symbols_fts.rowid = s.rowid
                 WHERE symbols_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .map_err(map_rusqlite_error)?;

        let rows = stmt
            .query_map(rusqlite::params![&fts_query, limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, f64>(4)?,
                ))
            })
            .map_err(map_rusqlite_error)?;

        let mut results = Vec::new();
        for row in rows {
            let (qn, name, kind, file, score) = row.map_err(map_rusqlite_error)?;
            results.push(SearchResult {
                qualified_name: qn,
                name,
                kind: symbol_kind_from_str(&kind)?,
                file_path: file.into(),
                score: -score, // FTS5 rank is negative (lower = better), invert for display
            });
        }
        Ok(results)
    }

    fn index_symbol(&self, _symbol: &SymbolNode) -> Result<()> {
        // No-op: FTS5 triggers handle sync automatically
        Ok(())
    }

    fn rebuild(&self) -> Result<()> {
        // Stub: real rebuild deferred to when needed
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ports::{GraphStore, SearchIndex};

    fn test_store() -> SqliteStore {
        SqliteStore::open_in_memory().unwrap()
    }

    fn file_and_symbol(name: &str, qn: &str) -> (FileNode, SymbolNode) {
        let file_path = qn.split("::").next().unwrap();
        (
            FileNode {
                path: file_path.into(),
                language: Language::Rust,
                hash: "h".into(),
            },
            SymbolNode {
                name: name.into(),
                qualified_name: qn.into(),
                kind: SymbolKind::Function,
                location: Location {
                    file: file_path.into(),
                    line_start: 1,
                    line_end: 10,
                    col_start: 0,
                    col_end: 1,
                },
                visibility: Visibility::Public,
                is_exported: true,
                is_async: false,
                is_test: false,
                decorators: vec![],
                signature: Some(format!("fn {name}()")),
            },
        )
    }

    #[test]
    fn insert_symbol_makes_it_searchable() {
        let store = test_store();
        let (file, sym) = file_and_symbol("UserService", "src/user.rs::UserService");
        store.upsert_file(&file).unwrap();
        store.upsert_symbol(&sym).unwrap();
        let results = store.search("UserService", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "UserService");
    }

    #[test]
    fn delete_symbol_removes_from_search() {
        let store = test_store();
        let (file, sym) = file_and_symbol("UserService", "src/user.rs::UserService");
        store.upsert_file(&file).unwrap();
        store.upsert_symbol(&sym).unwrap();
        store.remove_file("src/user.rs".as_ref()).unwrap();
        let results = store.search("UserService", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn update_symbol_updates_search() {
        let store = test_store();
        let file = FileNode {
            path: "src/a.rs".into(),
            language: Language::Rust,
            hash: "h".into(),
        };
        store.upsert_file(&file).unwrap();

        // Insert with a unique name that only appears in the name field
        let mut sym = SymbolNode {
            name: "AlphaName".into(),
            qualified_name: "src/a.rs::mysym".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/a.rs".into(),
                line_start: 1, line_end: 10,
                col_start: 0, col_end: 1,
            },
            visibility: Visibility::Public,
            is_exported: true, is_async: false, is_test: false,
            decorators: vec![], signature: None,
        };
        store.upsert_symbol(&sym).unwrap();
        assert!(!store.search("AlphaName", 10).unwrap().is_empty());

        // Update name — FTS5 trigger should remove old entry and add new one
        sym.name = "BetaName".into();
        store.upsert_symbol(&sym).unwrap();
        assert!(store.search("AlphaName", 10).unwrap().is_empty());
        assert!(!store.search("BetaName", 10).unwrap().is_empty());
    }

    #[test]
    fn search_ranks_exact_match_higher() {
        let store = test_store();
        let (f1, s1) = file_and_symbol("User", "src/a.rs::User");
        let (f2, s2) = file_and_symbol("UserService", "src/b.rs::UserService");
        store.upsert_file(&f1).unwrap();
        store.upsert_symbol(&s1).unwrap();
        store.upsert_file(&f2).unwrap();
        store.upsert_symbol(&s2).unwrap();
        let results = store.search("User", 10).unwrap();
        assert!(results.len() >= 1);
        assert_eq!(results[0].name, "User");
    }

    #[test]
    fn search_empty_query_returns_empty() {
        let store = test_store();
        let results = store.search("", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_respects_limit() {
        let store = test_store();
        for i in 0..5 {
            let name = format!("func_{i}");
            let qn = format!("src/f{i}.rs::{name}");
            let (f, s) = file_and_symbol(&name, &qn);
            store.upsert_file(&f).unwrap();
            store.upsert_symbol(&s).unwrap();
        }
        let results = store.search("func", 3).unwrap();
        assert!(results.len() <= 3);
    }

    #[test]
    fn index_symbol_is_noop() {
        let store = test_store();
        let (_, sym) = file_and_symbol("Test", "src/t.rs::Test");
        store.index_symbol(&sym).unwrap();
    }

    #[test]
    fn rebuild_is_noop() {
        let store = test_store();
        store.rebuild().unwrap();
    }
}
