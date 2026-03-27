# Research — M01-S02: SQLite Storage Layer

## R1: Dependency Versions & Feature Flags

### rusqlite
- **Version**: `0.37.0` with `features = ["bundled"]`
- **FTS5 confirmed**: The `bundled` feature unconditionally sets `-DSQLITE_ENABLE_FTS5` in the SQLite compile flags. No additional feature flags needed.
- **Transaction API**: `conn.transaction()` returns `Transaction<'_>` which derefs to `Connection`. Batch insert pattern:
  ```rust
  let tx = conn.transaction()?;
  {
      let mut stmt = tx.prepare_cached("INSERT INTO ...")?;
      for item in items { stmt.execute(params![...])?; }
  }
  tx.commit()?;
  ```
- **`execute_batch`**: Runs multiple semicolon-separated SQL statements. Ideal for schema creation.
- **PRAGMAs**: `conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")`

### r2d2_sqlite
- **Version**: `0.31.0` (compatible with rusqlite `^0.37`)
- **Pool creation with PRAGMA init**:
  ```rust
  let manager = SqliteConnectionManager::file("path.db")
      .with_init(|c| c.execute_batch(
          "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;"
      ));
  let pool = r2d2::Pool::new(manager)?;
  ```
- `with_init` runs on every new connection checkout — correct for per-connection PRAGMAs like `foreign_keys`.
- `journal_mode=WAL` is persistent (set once on DB), but safe to re-execute.

### r2d2
- **Version**: `0.8`
- Pool is `Clone + Send + Sync` — satisfies the port trait bounds.

### Cargo.toml for storage crate
```toml
[dependencies]
domain = { path = "../domain" }
rusqlite = { version = "0.37", features = ["bundled"] }
r2d2_sqlite = "0.31"
r2d2 = "0.8"
```

No `serde_json` needed in storage — enum-to-string mapping done via `Debug`/match.

---

## R2: Schema Migration Strategy

### Decision: Roll our own (no `rusqlite_migration` crate)

For v0.1 with a single schema version, a custom migration is ~15 lines:

```rust
fn ensure_schema(conn: &Connection) -> Result<()> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    match version {
        0 => {
            conn.execute_batch(SCHEMA_V1)?;
            conn.pragma_update(None, "user_version", 1)?;
        }
        1 => {} // current version, nothing to do
        v => return Err(/* unsupported version error */),
    }
    Ok(())
}
```

Uses SQLite's built-in `PRAGMA user_version` (integer, persisted in DB header). No metadata table needed for version tracking — the metadata table can be used for other key-value pairs (e.g., last index timestamp).

If v0.2 needs more migrations, we can adopt `rusqlite_migration` then. Avoiding the dependency now keeps the crate lean.

---

## R3: Enum-to-String Mapping (Domain ↔ SQLite)

Domain enums (`EdgeKind`, `SymbolKind`, `Language`, `Visibility`, `NonParsedKind`, `Confidence`) need TEXT representation in SQLite.

### Approach: `Debug` format for serialization, explicit match for deserialization

```rust
// Serialize: format!("{:?}", EdgeKind::Calls) → "Calls"
// Deserialize: match on string in storage crate

fn edge_kind_to_str(k: &EdgeKind) -> &'static str {
    match k {
        EdgeKind::Calls => "Calls",
        // ... all 16 variants
    }
}

fn edge_kind_from_str(s: &str) -> Result<EdgeKind> {
    match s {
        "Calls" => Ok(EdgeKind::Calls),
        // ... all 16 variants
        _ => Err(CodeGraphError::Storage(format!("unknown edge kind: {s}")))
    }
}
```

**Why not `strum`**: Would add a dependency to the domain crate (which has zero-dep policy).
**Why not `serde_json`**: Overhead of JSON serialization for simple unit variants. `serde_json::to_string(&EdgeKind::Calls)` produces `"\"Calls\""` (with quotes).

The storage crate owns these mapping functions. If a variant is added to domain, the storage mapping gets a compile error (non-exhaustive match) — this is a feature, not a bug.

---

## R4: Port Trait Modifications

### Current trait gaps identified

The current `GraphStore` trait (13 methods) lacks:

1. **Batch store** — No way to store a file + symbols + edges atomically. Individual `upsert_*` calls without transaction wrapping = slow and non-atomic.
2. **Edge cleanup** — No `remove_edges_for_file`. The edges table has no FK to symbols, so removing symbols doesn't cascade to edges. Stale edges will accumulate.

### Proposed additions

```rust
// Batch operation: store file with all its symbols and edges in one atomic operation.
// Adapter wraps in a single transaction for performance and atomicity.
fn store_file_data(
    &self,
    file: &FileNode,
    symbols: &[SymbolNode],
    edges: &[Edge],
) -> Result<()>;

// Remove all data for a file: the file row, its symbols, and all edges
// where source or target belongs to that file.
fn remove_file_data(&self, path: &Path) -> Result<()>;
```

**`store_file_data` SQL strategy**:
```sql
BEGIN;
INSERT OR REPLACE INTO files ...;
-- for each symbol:
INSERT OR REPLACE INTO symbols ...;
-- for each edge:
INSERT OR REPLACE INTO edges ...;
COMMIT;
```

**`remove_file_data` SQL strategy**:
```sql
BEGIN;
DELETE FROM edges WHERE source_qualified IN
    (SELECT qualified_name FROM symbols WHERE file_path = ?1)
    OR target_qualified IN
    (SELECT qualified_name FROM symbols WHERE file_path = ?1);
DELETE FROM symbols WHERE file_path = ?1;  -- or rely on CASCADE
DELETE FROM files WHERE path = ?1;
COMMIT;
```

### Backward compatibility

- Keep existing individual `upsert_*` methods (useful for fine-grained operations like adding a single edge during resolution)
- Keep `remove_file` and `remove_symbols_in_file` (useful for targeted cleanup)
- New methods are additive — `InMemoryGraphStore` test double updated with trivial implementations

---

## R5: FTS5 Trigger Behavior with TEXT Primary Key

### Confirmed: Triggers work with implicit rowid

SQLite tables with `TEXT PRIMARY KEY` still have an implicit `rowid` (they are NOT WITHOUT ROWID tables). The FTS5 content sync triggers referencing `new.rowid` / `old.rowid` will work correctly.

The design spec's trigger definitions are valid as-is:
```sql
CREATE TRIGGER symbols_ai AFTER INSERT ON symbols BEGIN
    INSERT INTO symbols_fts(rowid, name, qualified_name, file_path, signature)
    VALUES (new.rowid, new.name, new.qualified_name, new.file_path, new.signature);
END;
```

### FTS5 search implementation

Basic `SearchIndex::search` implementation:
```sql
SELECT s.qualified_name, s.name, s.kind, s.file_path,
       rank AS score
FROM symbols_fts
JOIN symbols s ON symbols_fts.rowid = s.rowid
WHERE symbols_fts MATCH ?1
ORDER BY rank
LIMIT ?2
```

FTS5's built-in `rank` column uses BM25 by default. Custom column weights (spec: name > signature > qualified_name > file_path) deferred to search quality slice.

---

## R6: Thread Safety Model

### r2d2 pool handles everything

- `r2d2::Pool<SqliteConnectionManager>` is `Clone + Send + Sync`
- Each `pool.get()` returns a `PooledConnection` which is `Send` (not `Sync`)
- With WAL mode: multiple readers can operate concurrently
- Writes are serialized by SQLite's internal locking + our 5s busy timeout
- For rayon parallel parsing: each thread gets its own connection from the pool

### Storage struct design

```rust
pub struct SqliteStore {
    pool: r2d2::Pool<SqliteConnectionManager>,
}

// SqliteStore is Send + Sync because Pool is Send + Sync.
// Satisfies GraphStore: Send + Sync and SearchIndex: Send + Sync.
```

---

## R7: Testing Strategy

### In-memory SQLite for fast tests

```rust
// Test helper: create an in-memory store with schema applied
fn test_store() -> SqliteStore {
    SqliteStore::open_in_memory().unwrap()
}
```

r2d2_sqlite supports `SqliteConnectionManager::memory()` but each connection gets a **different** in-memory DB. For tests with a single connection pool, use a named in-memory DB: `SqliteConnectionManager::file("file::memory:?cache=shared")` or configure pool with `max_size(1)`.

Simpler approach for tests: bypass r2d2, use a raw `Connection::open_in_memory()` directly. The `SqliteStore` can accept either a pool or a direct connection via an enum internally, or we add a `SqliteStore::open_in_memory()` constructor for tests.

**Recommended**: `SqliteStore::open_in_memory()` that creates a single-connection pool with `max_size(1)` pointing to a shared in-memory DB. This tests the real pool path without needing separate test infra.

---

## Summary of Decisions

| Question | Decision | Rationale |
|----------|----------|-----------|
| rusqlite version | 0.37 bundled | Confirmed FTS5 included |
| r2d2_sqlite version | 0.31 | Compatible with rusqlite 0.37 |
| Migration crate | Roll our own | Single version, ~15 lines |
| Schema versioning | PRAGMA user_version | Built-in, zero overhead |
| Enum mapping | Debug + match in storage | No new deps, compile-time safety |
| Port trait changes | Add store_file_data + remove_file_data | Atomic operations, edge cleanup |
| FTS5 triggers | Use design spec as-is | Confirmed rowid works with TEXT PK |
| Advanced search | Defer to later slice | Basic BM25 sufficient for S02 |
| Test strategy | In-memory pool, max_size(1) | Fast, tests real pool path |
