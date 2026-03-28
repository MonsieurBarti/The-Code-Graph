pub mod graph_store;
pub mod mapping;
pub mod schema;
pub mod search_index;

use domain::error::{CodeGraphError, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SqliteStore {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let manager = SqliteConnectionManager::file(path.as_ref()).with_init(|c| {
            c.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
            )
        });
        let pool = Pool::builder()
            .build(manager)
            .map_err(|e| CodeGraphError::Storage(e.to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| CodeGraphError::Storage(e.to_string()))?;
        schema::ensure_schema(&conn)?;
        Ok(Self { pool })
    }

    pub fn open_in_memory() -> Result<Self> {
        let manager = SqliteConnectionManager::memory().with_init(|c| {
            c.execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
            )
        });
        let pool = Pool::builder()
            .max_size(1)
            .build(manager)
            .map_err(|e| CodeGraphError::Storage(e.to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| CodeGraphError::Storage(e.to_string()))?;
        schema::ensure_schema(&conn)?;
        Ok(Self { pool })
    }

    pub(crate) fn conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.pool
            .get()
            .map_err(|e| CodeGraphError::Storage(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::model::*;
    use domain::ports::{GraphStore, SearchIndex};

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn sqlite_store_is_send_sync() {
        assert_send_sync::<SqliteStore>();
    }

    #[test]
    fn open_in_memory_creates_schema() {
        let store = SqliteStore::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }

    #[test]
    fn open_in_memory_creates_all_tables() {
        let store = SqliteStore::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"non_parsed_files".to_string()));
        assert!(tables.contains(&"symbols".to_string()));
        assert!(tables.contains(&"edges".to_string()));
        assert!(tables.contains(&"metadata".to_string()));
    }

    #[test]
    fn pragmas_are_set() {
        let store = SqliteStore::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let fk: i32 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn unsupported_schema_version_errors() {
        let store = SqliteStore::open_in_memory().unwrap();
        {
            let conn = store.conn().unwrap();
            conn.pragma_update(None, "user_version", 99).unwrap();
        }
        let conn = store.conn().unwrap();
        let result = schema::ensure_schema(&conn);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("unsupported schema version"));
    }

    // --- Integration tests (T07) ---

    #[test]
    fn fts5_trigger_sync_on_store_file_data() {
        let store = SqliteStore::open_in_memory().unwrap();
        let file = FileNode {
            path: "a.rs".into(),
            language: Language::Rust,
            hash: "h".into(),
        };
        let sym = SymbolNode {
            name: "BatchSymbol".into(),
            qualified_name: "a.rs::BatchSymbol".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "a.rs".into(),
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
        };
        store.store_file_data(&file, &[sym], &[]).unwrap();
        let results = store.search("BatchSymbol", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn cascade_delete_removes_from_fts() {
        let store = SqliteStore::open_in_memory().unwrap();
        let file = FileNode {
            path: "a.rs".into(),
            language: Language::Rust,
            hash: "h".into(),
        };
        let sym = SymbolNode {
            name: "Doomed".into(),
            qualified_name: "a.rs::Doomed".into(),
            kind: SymbolKind::Class,
            location: Location {
                file: "a.rs".into(),
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
        };
        store.store_file_data(&file, &[sym], &[]).unwrap();
        store.remove_file("a.rs".as_ref()).unwrap();
        assert!(store.search("Doomed", 10).unwrap().is_empty());
    }

    #[test]
    fn concurrent_reads_do_not_deadlock() {
        use std::thread;
        let store = SqliteStore::open_in_memory().unwrap();
        let file = FileNode {
            path: "a.rs".into(),
            language: Language::Rust,
            hash: "h".into(),
        };
        store.upsert_file(&file).unwrap();

        let s1 = store.clone();
        let s2 = store.clone();
        let t1 = thread::spawn(move || s1.all_files().unwrap());
        let t2 = thread::spawn(move || s2.stats().unwrap());
        t1.join().unwrap();
        t2.join().unwrap();
    }

    #[test]
    fn store_file_data_atomicity() {
        let store = SqliteStore::open_in_memory().unwrap();
        let file = FileNode {
            path: "a.rs".into(),
            language: Language::Rust,
            hash: "h".into(),
        };
        let sym = SymbolNode {
            name: "X".into(),
            qualified_name: "a.rs::X".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "a.rs".into(),
                line_start: 1,
                line_end: 2,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        };
        store.store_file_data(&file, &[sym], &[]).unwrap();
        assert!(store.get_file("a.rs".as_ref()).unwrap().is_some());
        assert!(store.get_symbol("a.rs::X").unwrap().is_some());
    }
}
