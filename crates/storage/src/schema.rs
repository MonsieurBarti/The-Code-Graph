use domain::error::{CodeGraphError, Result};
use rusqlite::Connection;

#[cfg(test)]
pub(crate) const SCHEMA_V1: &str = "
CREATE TABLE metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE files (
    path TEXT PRIMARY KEY,
    language TEXT NOT NULL,
    hash TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE non_parsed_files (
    path TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    hash TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE symbols (
    qualified_name TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    file_path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    col_start INTEGER NOT NULL,
    col_end INTEGER NOT NULL,
    visibility TEXT NOT NULL DEFAULT 'private',
    is_exported INTEGER NOT NULL DEFAULT 0,
    is_async INTEGER NOT NULL DEFAULT 0,
    is_test INTEGER NOT NULL DEFAULT 0,
    decorators TEXT,
    signature TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    source_qualified TEXT NOT NULL,
    target_qualified TEXT NOT NULL,
    metadata TEXT,
    UNIQUE(kind, source_qualified, target_qualified)
);

CREATE VIRTUAL TABLE symbols_fts USING fts5(
    name, qualified_name, file_path, signature,
    content='symbols', content_rowid='rowid'
);

CREATE TRIGGER symbols_ai AFTER INSERT ON symbols BEGIN
    INSERT INTO symbols_fts(rowid, name, qualified_name, file_path, signature)
    VALUES (new.rowid, new.name, new.qualified_name, new.file_path, new.signature);
END;

CREATE TRIGGER symbols_ad AFTER DELETE ON symbols BEGIN
    INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, file_path, signature)
    VALUES ('delete', old.rowid, old.name, old.qualified_name, old.file_path, old.signature);
END;

CREATE TRIGGER symbols_au AFTER UPDATE ON symbols BEGIN
    INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, file_path, signature)
    VALUES ('delete', old.rowid, old.name, old.qualified_name, old.file_path, old.signature);
    INSERT INTO symbols_fts(rowid, name, qualified_name, file_path, signature)
    VALUES (new.rowid, new.name, new.qualified_name, new.file_path, new.signature);
END;

CREATE INDEX idx_symbols_file ON symbols(file_path);
CREATE INDEX idx_symbols_kind ON symbols(kind);
CREATE INDEX idx_symbols_name ON symbols(name);
CREATE INDEX idx_edges_source ON edges(source_qualified);
CREATE INDEX idx_edges_target ON edges(target_qualified);
CREATE INDEX idx_edges_kind ON edges(kind);
";

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

pub(crate) const SCHEMA_V2: &str = "
CREATE TABLE metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE files (
    path TEXT PRIMARY KEY,
    language TEXT NOT NULL,
    hash TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE non_parsed_files (
    path TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    hash TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE symbols (
    qualified_name TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    file_path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    col_start INTEGER NOT NULL,
    col_end INTEGER NOT NULL,
    visibility TEXT NOT NULL DEFAULT 'private',
    is_exported INTEGER NOT NULL DEFAULT 0,
    is_async INTEGER NOT NULL DEFAULT 0,
    is_test INTEGER NOT NULL DEFAULT 0,
    decorators TEXT,
    signature TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    source_qualified TEXT NOT NULL,
    target_qualified TEXT NOT NULL,
    metadata TEXT,
    UNIQUE(kind, source_qualified, target_qualified)
);

CREATE VIRTUAL TABLE symbols_fts USING fts5(
    name, qualified_name, file_path, signature,
    content='symbols', content_rowid='rowid'
);

CREATE TRIGGER symbols_ai AFTER INSERT ON symbols BEGIN
    INSERT INTO symbols_fts(rowid, name, qualified_name, file_path, signature)
    VALUES (new.rowid, new.name, new.qualified_name, new.file_path, new.signature);
END;

CREATE TRIGGER symbols_ad AFTER DELETE ON symbols BEGIN
    INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, file_path, signature)
    VALUES ('delete', old.rowid, old.name, old.qualified_name, old.file_path, old.signature);
END;

CREATE TRIGGER symbols_au AFTER UPDATE ON symbols BEGIN
    INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, file_path, signature)
    VALUES ('delete', old.rowid, old.name, old.qualified_name, old.file_path, old.signature);
    INSERT INTO symbols_fts(rowid, name, qualified_name, file_path, signature)
    VALUES (new.rowid, new.name, new.qualified_name, new.file_path, new.signature);
END;

CREATE INDEX idx_symbols_file ON symbols(file_path);
CREATE INDEX idx_symbols_kind ON symbols(kind);
CREATE INDEX idx_symbols_name ON symbols(name);
CREATE INDEX idx_edges_source ON edges(source_qualified);
CREATE INDEX idx_edges_target ON edges(target_qualified);
CREATE INDEX idx_edges_kind ON edges(kind);

CREATE TABLE embeddings (
    qualified_name TEXT PRIMARY KEY REFERENCES symbols(qualified_name) ON DELETE CASCADE,
    vector BLOB NOT NULL,
    text_hash TEXT NOT NULL,
    provider TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_embeddings_provider ON embeddings(provider);
";

pub(crate) fn ensure_schema(conn: &Connection) -> Result<()> {
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(map_rusqlite_error)?;
    match version {
        0 => {
            conn.execute_batch(SCHEMA_V2).map_err(map_rusqlite_error)?;
            conn.pragma_update(None, "user_version", 2)
                .map_err(map_rusqlite_error)?;
        }
        1 => {
            conn.execute_batch(MIGRATION_V1_TO_V2)
                .map_err(map_rusqlite_error)?;
            conn.pragma_update(None, "user_version", 2)
                .map_err(map_rusqlite_error)?;
        }
        2 => {} // current
        v => {
            return Err(CodeGraphError::Storage(format!(
                "unsupported schema version: {v}"
            )));
        }
    }
    Ok(())
}

fn map_rusqlite_error(e: rusqlite::Error) -> CodeGraphError {
    CodeGraphError::Storage(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn has_table(conn: &Connection, table: &str) -> bool {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                rusqlite::params![table],
                |r| r.get(0),
            )
            .unwrap_or(0);
        count > 0
    }

    #[test]
    fn schema_v0_to_v2_creates_embeddings_table() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        assert!(
            has_table(&conn, "embeddings"),
            "embeddings table must exist after v0→v2"
        );
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }

    #[test]
    fn schema_v1_to_v2_migration_creates_embeddings_table() {
        let conn = Connection::open_in_memory().unwrap();
        // Bootstrap a v1 schema manually
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        // Now run ensure_schema which should migrate v1→v2
        ensure_schema(&conn).unwrap();
        assert!(
            has_table(&conn, "embeddings"),
            "embeddings table must exist after v1→v2"
        );
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }
}
