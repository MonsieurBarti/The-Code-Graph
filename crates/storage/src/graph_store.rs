use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use domain::error::Result;
use domain::model::*;
use domain::ports::GraphStore;

use crate::mapping::*;
use crate::SqliteStore;

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

impl GraphStore for SqliteStore {
    fn upsert_file(&self, file: &FileNode) -> Result<()> {
        let conn = self.conn()?;
        conn.prepare_cached(
            "INSERT OR REPLACE INTO files (path, language, hash, updated_at) VALUES (?1, ?2, ?3, ?4)",
        )
        .map_err(map_rusqlite_error)?
        .execute(rusqlite::params![
            file.path.to_str().unwrap_or_default(),
            language_to_str(&file.language),
            &file.hash,
            now_epoch(),
        ])
        .map_err(map_rusqlite_error)?;
        Ok(())
    }

    fn upsert_symbol(&self, symbol: &SymbolNode) -> Result<()> {
        let conn = self.conn()?;
        let decorators_json =
            serde_json::to_string(&symbol.decorators).map_err(|e| {
                domain::error::CodeGraphError::Storage(e.to_string())
            })?;
        conn.prepare_cached(
            "INSERT OR REPLACE INTO symbols (
                qualified_name, name, kind, file_path,
                line_start, line_end, col_start, col_end,
                visibility, is_exported, is_async, is_test,
                decorators, signature, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )
        .map_err(map_rusqlite_error)?
        .execute(rusqlite::params![
            &symbol.qualified_name,
            &symbol.name,
            symbol_kind_to_str(&symbol.kind),
            symbol.location.file.to_str().unwrap_or_default(),
            symbol.location.line_start as i64,
            symbol.location.line_end as i64,
            symbol.location.col_start as i64,
            symbol.location.col_end as i64,
            visibility_to_str(&symbol.visibility),
            symbol.is_exported as i32,
            symbol.is_async as i32,
            symbol.is_test as i32,
            &decorators_json,
            &symbol.signature,
            now_epoch(),
        ])
        .map_err(map_rusqlite_error)?;
        Ok(())
    }

    fn upsert_edge(&self, edge: &Edge) -> Result<()> {
        let conn = self.conn()?;
        conn.prepare_cached(
            "INSERT OR REPLACE INTO edges (kind, source_qualified, target_qualified, metadata)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .map_err(map_rusqlite_error)?
        .execute(rusqlite::params![
            edge_kind_to_str(&edge.kind),
            &edge.source,
            &edge.target,
            &edge.metadata,
        ])
        .map_err(map_rusqlite_error)?;
        Ok(())
    }

    fn get_file(&self, path: &Path) -> Result<Option<FileNode>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached("SELECT path, language, hash FROM files WHERE path = ?1")
            .map_err(map_rusqlite_error)?;
        let result = stmt.query_row(
            rusqlite::params![path.to_str().unwrap_or_default()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        );
        match result {
            Ok((p, lang, hash)) => Ok(Some(FileNode {
                path: p.into(),
                language: language_from_str(&lang)?,
                hash,
            })),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(map_rusqlite_error(e)),
        }
    }

    fn get_symbol(&self, qualified_name: &str) -> Result<Option<SymbolNode>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT qualified_name, name, kind, file_path,
                        line_start, line_end, col_start, col_end,
                        visibility, is_exported, is_async, is_test,
                        decorators, signature
                 FROM symbols WHERE qualified_name = ?1",
            )
            .map_err(map_rusqlite_error)?;
        let result = stmt.query_row(rusqlite::params![qualified_name], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i32>(9)?,
                row.get::<_, i32>(10)?,
                row.get::<_, i32>(11)?,
                row.get::<_, Option<String>>(12)?,
                row.get::<_, Option<String>>(13)?,
            ))
        });
        match result {
            Ok((qn, name, kind, file, ls, le, cs, ce, vis, exp, asy, tst, dec, sig)) => {
                let decorators: Vec<String> = match dec {
                    Some(ref s) => serde_json::from_str(s).map_err(|e| {
                        domain::error::CodeGraphError::Storage(e.to_string())
                    })?,
                    None => vec![],
                };
                Ok(Some(SymbolNode {
                    qualified_name: qn,
                    name,
                    kind: symbol_kind_from_str(&kind)?,
                    location: Location {
                        file: file.into(),
                        line_start: ls as usize,
                        line_end: le as usize,
                        col_start: cs as usize,
                        col_end: ce as usize,
                    },
                    visibility: visibility_from_str(&vis)?,
                    is_exported: exp != 0,
                    is_async: asy != 0,
                    is_test: tst != 0,
                    decorators,
                    signature: sig,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(map_rusqlite_error(e)),
        }
    }

    fn get_edges_from(&self, source: &str) -> Result<Vec<Edge>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT kind, source_qualified, target_qualified, metadata
                 FROM edges WHERE source_qualified = ?1",
            )
            .map_err(map_rusqlite_error)?;
        let rows = stmt
            .query_map(rusqlite::params![source], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(map_rusqlite_error)?;
        let mut edges = Vec::new();
        for row in rows {
            let (kind, src, tgt, meta) = row.map_err(map_rusqlite_error)?;
            edges.push(Edge {
                kind: edge_kind_from_str(&kind)?,
                source: src,
                target: tgt,
                metadata: meta,
            });
        }
        Ok(edges)
    }

    fn get_edges_to(&self, target: &str) -> Result<Vec<Edge>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT kind, source_qualified, target_qualified, metadata
                 FROM edges WHERE target_qualified = ?1",
            )
            .map_err(map_rusqlite_error)?;
        let rows = stmt
            .query_map(rusqlite::params![target], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(map_rusqlite_error)?;
        let mut edges = Vec::new();
        for row in rows {
            let (kind, src, tgt, meta) = row.map_err(map_rusqlite_error)?;
            edges.push(Edge {
                kind: edge_kind_from_str(&kind)?,
                source: src,
                target: tgt,
                metadata: meta,
            });
        }
        Ok(edges)
    }

    fn all_files(&self) -> Result<Vec<FileNode>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached("SELECT path, language, hash FROM files")
            .map_err(map_rusqlite_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(map_rusqlite_error)?;
        let mut files = Vec::new();
        for row in rows {
            let (path, lang, hash) = row.map_err(map_rusqlite_error)?;
            files.push(FileNode {
                path: path.into(),
                language: language_from_str(&lang)?,
                hash,
            });
        }
        Ok(files)
    }

    fn all_symbols(&self) -> Result<Vec<SymbolNode>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT qualified_name, name, kind, file_path,
                        line_start, line_end, col_start, col_end,
                        visibility, is_exported, is_async, is_test,
                        decorators, signature
                 FROM symbols",
            )
            .map_err(map_rusqlite_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i32>(9)?,
                    row.get::<_, i32>(10)?,
                    row.get::<_, i32>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                ))
            })
            .map_err(map_rusqlite_error)?;
        let mut symbols = Vec::new();
        for row in rows {
            let (qn, name, kind, file, ls, le, cs, ce, vis, exp, asy, tst, dec, sig) =
                row.map_err(map_rusqlite_error)?;
            let decorators: Vec<String> = match dec {
                Some(ref s) => serde_json::from_str(s).map_err(|e| {
                    domain::error::CodeGraphError::Storage(e.to_string())
                })?,
                None => vec![],
            };
            symbols.push(SymbolNode {
                qualified_name: qn,
                name,
                kind: symbol_kind_from_str(&kind)?,
                location: Location {
                    file: file.into(),
                    line_start: ls as usize,
                    line_end: le as usize,
                    col_start: cs as usize,
                    col_end: ce as usize,
                },
                visibility: visibility_from_str(&vis)?,
                is_exported: exp != 0,
                is_async: asy != 0,
                is_test: tst != 0,
                decorators,
                signature: sig,
            });
        }
        Ok(symbols)
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT kind, source_qualified, target_qualified, metadata FROM edges",
            )
            .map_err(map_rusqlite_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(map_rusqlite_error)?;
        let mut edges = Vec::new();
        for row in rows {
            let (kind, src, tgt, meta) = row.map_err(map_rusqlite_error)?;
            edges.push(Edge {
                kind: edge_kind_from_str(&kind)?,
                source: src,
                target: tgt,
                metadata: meta,
            });
        }
        Ok(edges)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let conn = self.conn()?;
        conn.prepare_cached("DELETE FROM files WHERE path = ?1")
            .map_err(map_rusqlite_error)?
            .execute(rusqlite::params![path.to_str().unwrap_or_default()])
            .map_err(map_rusqlite_error)?;
        Ok(())
    }

    fn remove_symbols_in_file(&self, path: &Path) -> Result<()> {
        let conn = self.conn()?;
        conn.prepare_cached("DELETE FROM symbols WHERE file_path = ?1")
            .map_err(map_rusqlite_error)?
            .execute(rusqlite::params![path.to_str().unwrap_or_default()])
            .map_err(map_rusqlite_error)?;
        Ok(())
    }

    fn stats(&self) -> Result<GraphStats> {
        let conn = self.conn()?;
        let files: usize = conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .map_err(map_rusqlite_error)?;
        let symbols: usize = conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
            .map_err(map_rusqlite_error)?;
        let edges: usize = conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .map_err(map_rusqlite_error)?;
        Ok(GraphStats { files, symbols, edges })
    }

    fn store_file_data(
        &self,
        _file: &FileNode,
        _symbols: &[SymbolNode],
        _edges: &[Edge],
    ) -> Result<()> {
        todo!("implemented in T05")
    }

    fn remove_file_data(&self, _path: &Path) -> Result<()> {
        todo!("implemented in T05")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ports::GraphStore;

    fn test_store() -> SqliteStore {
        SqliteStore::open_in_memory().unwrap()
    }

    fn sample_file() -> FileNode {
        FileNode {
            path: "src/main.rs".into(),
            language: Language::Rust,
            hash: "abc123".into(),
        }
    }

    fn sample_symbol() -> SymbolNode {
        SymbolNode {
            name: "foo".into(),
            qualified_name: "src/main.rs::foo".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/main.rs".into(),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 1,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec!["inline".into()],
            signature: Some("fn foo() -> bool".into()),
        }
    }

    fn sample_edge() -> Edge {
        Edge {
            kind: EdgeKind::Calls,
            source: "src/main.rs::foo".into(),
            target: "src/lib.rs::bar".into(),
            metadata: None,
        }
    }

    #[test]
    fn upsert_file_insert_then_update() {
        let store = test_store();
        let mut file = sample_file();
        store.upsert_file(&file).unwrap();
        let got = store.get_file(&file.path).unwrap().unwrap();
        assert_eq!(got.hash, "abc123");

        file.hash = "def456".into();
        store.upsert_file(&file).unwrap();
        let got = store.get_file(&file.path).unwrap().unwrap();
        assert_eq!(got.hash, "def456");
    }

    #[test]
    fn get_file_missing_returns_none() {
        let store = test_store();
        assert!(store.get_file("nonexistent".as_ref()).unwrap().is_none());
    }

    #[test]
    fn upsert_symbol_insert_then_update() {
        let store = test_store();
        store.upsert_file(&sample_file()).unwrap();
        let mut sym = sample_symbol();
        store.upsert_symbol(&sym).unwrap();
        let got = store.get_symbol(&sym.qualified_name).unwrap().unwrap();
        assert_eq!(got.name, "foo");

        sym.name = "foo_renamed".into();
        store.upsert_symbol(&sym).unwrap();
        let got = store.get_symbol(&sym.qualified_name).unwrap().unwrap();
        assert_eq!(got.name, "foo_renamed");
    }

    #[test]
    fn get_symbol_missing_returns_none() {
        let store = test_store();
        assert!(store.get_symbol("nonexistent").unwrap().is_none());
    }

    #[test]
    fn upsert_edge_idempotent() {
        let store = test_store();
        let edge = sample_edge();
        store.upsert_edge(&edge).unwrap();
        store.upsert_edge(&edge).unwrap();
        let edges = store.get_edges_from(&edge.source).unwrap();
        assert_eq!(edges.len(), 1);
    }

    #[test]
    fn get_edges_from_and_to() {
        let store = test_store();
        store.upsert_edge(&sample_edge()).unwrap();
        let from = store.get_edges_from("src/main.rs::foo").unwrap();
        assert_eq!(from.len(), 1);
        let to = store.get_edges_to("src/lib.rs::bar").unwrap();
        assert_eq!(to.len(), 1);
        assert!(store.get_edges_from("none").unwrap().is_empty());
        assert!(store.get_edges_to("none").unwrap().is_empty());
    }

    #[test]
    fn all_files_symbols_edges() {
        let store = test_store();
        store.upsert_file(&sample_file()).unwrap();
        store.upsert_symbol(&sample_symbol()).unwrap();
        store.upsert_edge(&sample_edge()).unwrap();
        assert_eq!(store.all_files().unwrap().len(), 1);
        assert_eq!(store.all_symbols().unwrap().len(), 1);
        assert_eq!(store.all_edges().unwrap().len(), 1);
    }

    #[test]
    fn remove_file_cascades_to_symbols() {
        let store = test_store();
        store.upsert_file(&sample_file()).unwrap();
        store.upsert_symbol(&sample_symbol()).unwrap();
        store.remove_file("src/main.rs".as_ref()).unwrap();
        assert!(store.get_file("src/main.rs".as_ref()).unwrap().is_none());
        assert!(store.get_symbol("src/main.rs::foo").unwrap().is_none());
    }

    #[test]
    fn remove_symbols_in_file_keeps_file() {
        let store = test_store();
        store.upsert_file(&sample_file()).unwrap();
        store.upsert_symbol(&sample_symbol()).unwrap();
        store.remove_symbols_in_file("src/main.rs".as_ref()).unwrap();
        assert!(store.get_file("src/main.rs".as_ref()).unwrap().is_some());
        assert!(store.get_symbol("src/main.rs::foo").unwrap().is_none());
    }

    #[test]
    fn stats_returns_correct_counts() {
        let store = test_store();
        store.upsert_file(&sample_file()).unwrap();
        store.upsert_symbol(&sample_symbol()).unwrap();
        store.upsert_edge(&sample_edge()).unwrap();
        let s = store.stats().unwrap();
        assert_eq!(s.files, 1);
        assert_eq!(s.symbols, 1);
        assert_eq!(s.edges, 1);
    }

    #[test]
    fn symbol_roundtrip_preserves_all_fields() {
        let store = test_store();
        store.upsert_file(&sample_file()).unwrap();
        let sym = sample_symbol();
        store.upsert_symbol(&sym).unwrap();
        let got = store.get_symbol(&sym.qualified_name).unwrap().unwrap();
        assert_eq!(got.name, sym.name);
        assert_eq!(got.kind, sym.kind);
        assert_eq!(got.visibility, sym.visibility);
        assert_eq!(got.is_exported, sym.is_exported);
        assert_eq!(got.is_async, sym.is_async);
        assert_eq!(got.is_test, sym.is_test);
        assert_eq!(got.decorators, sym.decorators);
        assert_eq!(got.signature, sym.signature);
        assert_eq!(got.location.line_start, sym.location.line_start);
        assert_eq!(got.location.line_end, sym.location.line_end);
    }
}
