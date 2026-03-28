use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, Result as SqlResult, params};
use serde::{de::DeserializeOwned, Serialize};

pub struct StorageEngine {
    conn: Arc<Mutex<Connection>>,
}

impl StorageEngine {
    pub fn open(path: &Path) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA cache_size=-32000;")?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn open_in_memory() -> SqlResult<Self> {
        let conn = Connection::open_in_memory()?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn execute_ddl(&self, sql: &str) -> SqlResult<()> {
        self.conn.lock().execute_batch(sql)
    }

    pub fn insert_json<T: Serialize>(&self, table: &str, key: &str, value: &T) -> SqlResult<()> {
        let json = serde_json::to_string(value).unwrap();
        self.conn.lock().execute(
            &format!("INSERT OR REPLACE INTO {} (key, value) VALUES (?1, ?2)", table),
            params![key, json],
        )?;
        Ok(())
    }

    pub fn get_json<T: DeserializeOwned>(&self, table: &str, key: &str) -> SqlResult<Option<T>> {
        let conn = self.conn.lock();
        let result: Option<String> = conn.query_row(
            &format!("SELECT value FROM {} WHERE key = ?1", table),
            params![key],
            |row| row.get(0),
        ).optional()?;
        Ok(result.and_then(|s| serde_json::from_str(&s).ok()))
    }

    pub fn delete(&self, table: &str, key: &str) -> SqlResult<bool> {
        let n = self.conn.lock().execute(
            &format!("DELETE FROM {} WHERE key = ?1", table),
            params![key],
        )?;
        Ok(n > 0)
    }

    pub fn count(&self, table: &str) -> SqlResult<i64> {
        self.conn.lock().query_row(
            &format!("SELECT COUNT(*) FROM {}", table),
            [],
            |row| row.get(0),
        )
    }
}
