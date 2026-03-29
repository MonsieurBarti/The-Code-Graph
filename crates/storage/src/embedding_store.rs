use std::time::SystemTime;

use domain::error::Result;
use domain::model::EmbeddingEntry;
use domain::ports::VectorStore;

use crate::mapping::map_rusqlite_error;
use crate::SqliteStore;

const PROVIDER: &str = "all-MiniLM-L6-v2";

fn now_rfc3339() -> String {
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    // Simple ISO 8601 timestamp without pulling in chrono
    let (days, rem) = (secs / 86400, secs % 86400);
    let (hours, rem) = (rem / 3600, rem % 3600);
    let (mins, s) = (rem / 60, rem % 60);
    // Days since 1970-01-01, convert to y-m-d via a basic algorithm
    let mut y = 1970i64;
    let mut d = days as i64;
    loop {
        let year_days = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if d < year_days {
            break;
        }
        d -= year_days;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0;
    for md in month_days {
        if d < md {
            break;
        }
        d -= md;
        m += 1;
    }
    format!(
        "{y:04}-{:02}-{:02}T{hours:02}:{mins:02}:{s:02}Z",
        m + 1,
        d + 1
    )
}

fn pack_f32(vec: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(vec.len() * 4);
    for &v in vec {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf
}

fn unpack_f32(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for i in 0..a.len() {
        let ai = a[i] as f64;
        let bi = b[i] as f64;
        dot += ai * bi;
        norm_a += ai * ai;
        norm_b += bi * bi;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

impl VectorStore for SqliteStore {
    fn store_embeddings(&self, entries: &[EmbeddingEntry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let conn = self.conn()?;
        let created_at = now_rfc3339();
        let mut stmt = conn
            .prepare_cached(
                "INSERT OR REPLACE INTO embeddings (qualified_name, vector, text_hash, provider, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(map_rusqlite_error)?;
        for entry in entries {
            let blob = pack_f32(&entry.vector);
            stmt.execute(rusqlite::params![
                &entry.qualified_name,
                blob,
                &entry.text_hash,
                PROVIDER,
                &created_at,
            ])
            .map_err(map_rusqlite_error)?;
        }
        Ok(())
    }

    fn search_nearest(&self, query_vec: &[f32], limit: usize) -> Result<Vec<(String, f64)>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached("SELECT qualified_name, vector FROM embeddings")
            .map_err(map_rusqlite_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(map_rusqlite_error)?;

        let mut scored: Vec<(String, f64)> = Vec::new();
        for row in rows {
            let (qn, blob) = row.map_err(map_rusqlite_error)?;
            let vec = unpack_f32(&blob);
            let sim = cosine_similarity(query_vec, &vec);
            scored.push((qn, sim));
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    fn has_embeddings(&self) -> bool {
        self.conn()
            .ok()
            .and_then(|conn| {
                conn.query_row("SELECT EXISTS(SELECT 1 FROM embeddings)", [], |r| {
                    r.get::<_, i32>(0)
                })
                .ok()
            })
            .map(|v| v != 0)
            .unwrap_or(false)
    }

    fn count(&self) -> Result<usize> {
        let conn = self.conn()?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
            .map_err(map_rusqlite_error)?;
        Ok(n as usize)
    }

    fn remove_embeddings(&self, qualified_names: &[&str]) -> Result<()> {
        if qualified_names.is_empty() {
            return Ok(());
        }
        let conn = self.conn()?;
        // SAFETY: placeholders are numeric indices (?1, ?2, ...) derived from the slice
        // length — no user data is interpolated into SQL. Values are bound via params_from_iter.
        let placeholders: String = (1..=qualified_names.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("DELETE FROM embeddings WHERE qualified_name IN ({placeholders})");
        let mut stmt = conn.prepare(&sql).map_err(map_rusqlite_error)?;
        stmt.execute(rusqlite::params_from_iter(qualified_names.iter()))
            .map_err(map_rusqlite_error)?;
        Ok(())
    }

    fn get_stored_hashes(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached("SELECT qualified_name, text_hash FROM embeddings")
            .map_err(map_rusqlite_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(map_rusqlite_error)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(map_rusqlite_error)?);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::model::{FileNode, Language, Location, SymbolKind, SymbolNode, Visibility};
    use domain::ports::{GraphStore, VectorStore};

    fn setup() -> SqliteStore {
        SqliteStore::open_in_memory().unwrap()
    }

    fn make_entry(qn: &str, vec: Vec<f32>) -> EmbeddingEntry {
        EmbeddingEntry {
            qualified_name: qn.to_string(),
            vector: vec,
            text_hash: format!("hash_{qn}"),
        }
    }

    /// Insert a file + symbol so the FK constraint on embeddings.qualified_name is satisfied.
    fn insert_symbol(store: &SqliteStore, file_path: &str, qn: &str) {
        let file = FileNode {
            path: file_path.into(),
            language: Language::Rust,
            hash: "h".into(),
        };
        store.upsert_file(&file).unwrap();
        let sym = SymbolNode {
            name: qn.split("::").last().unwrap_or(qn).to_string(),
            qualified_name: qn.to_string(),
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
            signature: None,
        };
        store.upsert_symbol(&sym).unwrap();
    }

    #[test]
    fn has_embeddings_false_when_empty() {
        let store = setup();
        assert!(!store.has_embeddings());
    }

    #[test]
    fn has_embeddings_true_after_store() {
        let store = setup();
        insert_symbol(&store, "src/a.rs", "src/a.rs::foo");
        store
            .store_embeddings(&[make_entry("src/a.rs::foo", vec![1.0, 0.0])])
            .unwrap();
        assert!(store.has_embeddings());
    }

    #[test]
    fn count_returns_correct_number() {
        let store = setup();
        assert_eq!(store.count().unwrap(), 0);
        insert_symbol(&store, "src/a.rs", "src/a.rs::foo");
        insert_symbol(&store, "src/b.rs", "src/b.rs::bar");
        store
            .store_embeddings(&[
                make_entry("src/a.rs::foo", vec![1.0, 0.0]),
                make_entry("src/b.rs::bar", vec![0.0, 1.0]),
            ])
            .unwrap();
        assert_eq!(store.count().unwrap(), 2);
    }

    #[test]
    fn store_and_retrieve_embeddings() {
        let store = setup();
        insert_symbol(&store, "src/a.rs", "src/a.rs::foo");
        insert_symbol(&store, "src/b.rs", "src/b.rs::bar");

        store
            .store_embeddings(&[
                make_entry("src/a.rs::foo", vec![1.0, 0.0, 0.0]),
                make_entry("src/b.rs::bar", vec![0.0, 1.0, 0.0]),
            ])
            .unwrap();

        // Query close to "foo"
        let results = store.search_nearest(&[1.0, 0.0, 0.0], 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "src/a.rs::foo");
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn cosine_similarity_ranking() {
        let store = setup();
        insert_symbol(&store, "src/a.rs", "src/a.rs::close");
        insert_symbol(&store, "src/b.rs", "src/b.rs::far");

        // "close" is near the query; "far" is orthogonal
        store
            .store_embeddings(&[
                make_entry("src/a.rs::close", vec![0.9, 0.1]),
                make_entry("src/b.rs::far", vec![0.0, 1.0]),
            ])
            .unwrap();

        let results = store.search_nearest(&[1.0, 0.0], 2).unwrap();
        assert_eq!(results[0].0, "src/a.rs::close");
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn remove_embeddings_deletes_entries() {
        let store = setup();
        insert_symbol(&store, "src/a.rs", "src/a.rs::foo");
        insert_symbol(&store, "src/b.rs", "src/b.rs::bar");

        store
            .store_embeddings(&[
                make_entry("src/a.rs::foo", vec![1.0, 0.0]),
                make_entry("src/b.rs::bar", vec![0.0, 1.0]),
            ])
            .unwrap();
        assert_eq!(store.count().unwrap(), 2);

        store.remove_embeddings(&["src/a.rs::foo"]).unwrap();
        assert_eq!(store.count().unwrap(), 1);

        let results = store.search_nearest(&[1.0, 0.0], 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "src/b.rs::bar");
    }

    #[test]
    fn store_embeddings_upserts() {
        let store = setup();
        insert_symbol(&store, "src/a.rs", "src/a.rs::foo");

        // First insert
        store
            .store_embeddings(&[make_entry("src/a.rs::foo", vec![1.0, 0.0])])
            .unwrap();
        assert_eq!(store.count().unwrap(), 1);

        // Second insert with same qualified_name — should replace, not add
        store
            .store_embeddings(&[make_entry("src/a.rs::foo", vec![0.0, 1.0])])
            .unwrap();
        assert_eq!(store.count().unwrap(), 1);

        // The stored vector should now be [0.0, 1.0]
        let results = store.search_nearest(&[0.0, 1.0], 1).unwrap();
        assert_eq!(results.len(), 1);
        assert!((results[0].1 - 1.0).abs() < 1e-6);
    }
}
