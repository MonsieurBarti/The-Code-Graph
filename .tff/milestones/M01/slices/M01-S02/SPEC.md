# M01-S02: SQLite Storage Layer

## Problem

The domain crate defines port traits (`GraphStore`, `SearchIndex`) but has no real adapter. Without a persistence layer, the system cannot store or query the code graph. The storage crate is the first adapter crate — it implements the domain's outbound ports against SQLite with FTS5 full-text search.

## Approach

Create a `crates/storage` crate that implements `GraphStore` and `SearchIndex` using `rusqlite` (bundled SQLite) with `r2d2` connection pooling. The crate owns all SQL, schema creation, enum mapping, and connection management. It depends only on `domain` and its SQLite/pooling dependencies.

Two port trait methods are added to `GraphStore` in the domain crate to support atomic batch operations needed for indexing: `store_file_data` (store file + symbols + edges in one transaction) and `remove_file_data` (remove file + symbols + edges atomically, including edge cleanup).

## Scope

### In Scope
- Create `crates/storage` with Cargo.toml and add to workspace
- Full SQLite schema from design spec Section 5.2 (tables, indexes, FTS5 virtual table, triggers)
- Connection management via r2d2 pool with WAL mode, foreign_keys, busy_timeout pragmas
- Implement all `GraphStore` methods (existing 13 + 2 new batch methods)
- Implement all `SearchIndex` methods (basic FTS5 search with default BM25 ranking)
- Schema versioning via `PRAGMA user_version`
- Add `store_file_data` and `remove_file_data` to `GraphStore` trait in domain
- Update `InMemoryGraphStore` test double with new method implementations
- Domain-to-SQL enum mapping functions in the storage crate
- Unit tests for every trait method
- Integration tests for FTS5 trigger sync, cascading deletes, batch atomicity

### Not In Scope
- Advanced search quality (custom BM25 weights, trigram similarity, query-aware boosting) — deferred to Query/Eval slices
- Non-parsed file CRUD methods on `GraphStore` — table created, methods added when a consumer needs them
- Multi-process WAL concurrency testing
- Connection pool tuning / benchmarks
- Any CLI, parser, or watch integration
- `SearchIndex::rebuild` — implemented as a stub that returns `Ok(())`; real rebuild deferred to when the index command needs it

## Design

### Crate Structure

```
crates/storage/
  Cargo.toml
  src/
    lib.rs              # SqliteStore, constructors, re-exports
    schema.rs           # SQL DDL string, ensure_schema()
    mapping.rs          # domain enum <-> TEXT conversion functions
    graph_store.rs      # GraphStore impl for SqliteStore
    search_index.rs     # SearchIndex impl for SqliteStore
```

### Dependencies

```toml
[dependencies]
domain = { path = "../domain" }
rusqlite = { version = "0.37", features = ["bundled"] }
r2d2_sqlite = "0.31"
r2d2 = "0.8"
serde_json = "1"       # for Vec<String> decorators <-> TEXT mapping
```

### SqliteStore Type

```rust
pub struct SqliteStore {
    pool: r2d2::Pool<SqliteConnectionManager>,
}

impl SqliteStore {
    /// Open or create a database at the given path.
    /// Runs PRAGMA setup on each connection and ensures schema is at latest version.
    pub fn open(path: impl AsRef<Path>) -> domain::error::Result<Self>;

    /// In-memory database for tests. Single-connection pool.
    pub fn open_in_memory() -> domain::error::Result<Self>;
}

// SqliteStore is Send + Sync via r2d2::Pool.
```

Connection initialization via `SqliteConnectionManager::with_init`:
```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

### Schema (schema.rs)

Full schema from design spec Section 5.2:
- `files` table (path PK, language, hash, updated_at)
- `non_parsed_files` table (path PK, kind, hash, updated_at)
- `symbols` table (qualified_name PK, FK to files with CASCADE, all symbol fields, updated_at)
- `edges` table (id AUTOINCREMENT, kind, source_qualified, target_qualified, metadata, UNIQUE constraint)
- `symbols_fts` FTS5 virtual table (content='symbols', content_rowid='rowid')
- 3 FTS5 sync triggers (AFTER INSERT, AFTER DELETE, AFTER UPDATE)
- 6 indexes (symbols: file, kind, name; edges: source, target, kind)

Schema versioning:
```rust
fn ensure_schema(conn: &Connection) -> Result<()> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    match version {
        0 => {
            conn.execute_batch(SCHEMA_V1)?;
            conn.pragma_update(None, "user_version", 1)?;
        }
        1 => {} // current
        v => return Err(CodeGraphError::Storage(
            format!("unsupported schema version: {v}")
        )),
    }
    Ok(())
}
```

Schema initialization runs once when the `SqliteStore` is constructed (on a dedicated connection before pooling begins), not on every connection checkout.

### Port Trait Modifications (domain crate)

Two methods added to `GraphStore`:

```rust
/// Store a file and all its symbols and edges atomically.
/// The adapter wraps this in a single transaction.
/// Replaces existing data for the file if present.
fn store_file_data(
    &self,
    file: &FileNode,
    symbols: &[SymbolNode],
    edges: &[Edge],
) -> Result<()>;

/// Remove all data associated with a file: the file row, its symbols,
/// and all edges where source or target is a symbol in that file.
fn remove_file_data(&self, path: &Path) -> Result<()>;
```

`InMemoryGraphStore` in `test_support.rs` updated to implement both methods with simple in-memory logic.

### Enum Mapping (mapping.rs)

Each domain enum stored as TEXT gets a pair of functions:

```rust
pub fn language_to_str(l: &Language) -> &'static str;
pub fn language_from_str(s: &str) -> Result<Language>;

pub fn symbol_kind_to_str(k: &SymbolKind) -> &'static str;
pub fn symbol_kind_from_str(s: &str) -> Result<SymbolKind>;

// ... same for EdgeKind, Visibility, NonParsedKind
```

Uses explicit `match` in both directions. Non-exhaustive match on `*_to_str` produces a compile error when a new variant is added to domain — intentional safety net.

### Decorators Mapping

`Vec<String>` stored as JSON array TEXT via `serde_json`:
- Write: `serde_json::to_string(&decorators)?`
- Read: `serde_json::from_str::<Vec<String>>(text)?`

Empty vec stored as `"[]"`, not NULL. The `decorators` column is nullable in the schema for forward-compat, but this slice always writes a value.

### updated_at Column

The `updated_at INTEGER` columns in `files` and `symbols` store Unix epoch seconds. This is a storage concern not present in domain types. The storage adapter generates it at write time via `SystemTime::now().duration_since(UNIX_EPOCH)`.

### store_file_data SQL Strategy

```sql
BEGIN IMMEDIATE;
  INSERT OR REPLACE INTO files (path, language, hash, updated_at) VALUES (...);
  -- for each symbol:
  INSERT OR REPLACE INTO symbols (...) VALUES (...);
  -- for each edge:
  INSERT OR REPLACE INTO edges (kind, source_qualified, target_qualified, metadata) VALUES (...);
COMMIT;
```

Uses `IMMEDIATE` transaction to acquire write lock upfront and avoid deadlocks under concurrent access.

### remove_file_data SQL Strategy

```sql
BEGIN IMMEDIATE;
  DELETE FROM edges
    WHERE source_qualified IN (SELECT qualified_name FROM symbols WHERE file_path = ?1)
       OR target_qualified IN (SELECT qualified_name FROM symbols WHERE file_path = ?1);
  -- CASCADE handles symbols when file is deleted:
  DELETE FROM files WHERE path = ?1;
COMMIT;
```

Edge cleanup happens first (before CASCADE removes the symbols we need to join on).

### FTS5 Search (search_index.rs)

`SearchIndex::search` implementation:
```sql
SELECT s.qualified_name, s.name, s.kind, s.file_path, rank
FROM symbols_fts
JOIN symbols s ON symbols_fts.rowid = s.rowid
WHERE symbols_fts MATCH ?1
ORDER BY rank
LIMIT ?2
```

FTS5's built-in `rank` uses BM25 with default column weights. Custom weights deferred.

`SearchIndex::index_symbol` is a no-op — FTS5 triggers handle sync automatically on INSERT/UPDATE/DELETE. The method exists to satisfy the trait.

`SearchIndex::rebuild` is a no-op stub for now. When needed, it will drop and recreate the FTS5 content from the symbols table.

## Acceptance Criteria

### Schema & Infrastructure
- AC1: `cargo build --workspace` succeeds with storage crate in workspace members
- AC2: Opening a fresh `SqliteStore` creates all 5 tables, 1 FTS5 virtual table, 3 triggers, and 6 indexes
- AC3: Every pooled connection has WAL journal mode, foreign_keys=ON, busy_timeout=5000
- AC4: `PRAGMA user_version` reads 1 after schema creation
- AC5: Opening a DB with `user_version > 1` returns `CodeGraphError::Storage` with "unsupported schema version"

### GraphStore — Individual Operations
- AC6: `upsert_file` inserts a new file; upserting with same path but different hash updates it
- AC7: `upsert_symbol` inserts a new symbol; upserting same qualified_name updates all fields
- AC8: `upsert_edge` inserts; upserting same (kind, source, target) tuple is idempotent (no duplicate, no error)
- AC9: `get_file` returns `Some` for existing path, `None` for missing
- AC10: `get_symbol` returns `Some` for existing qualified_name, `None` for missing
- AC11: `get_edges_from` returns all edges with matching source; empty vec if none
- AC12: `get_edges_to` returns all edges with matching target; empty vec if none
- AC13: `all_files`, `all_symbols`, `all_edges` return complete collections matching inserted data
- AC14: `remove_file` deletes file row; CASCADE deletes its symbols
- AC15: `remove_symbols_in_file` deletes symbols for a file path without removing the file row
- AC16: `stats` returns correct counts for files, symbols, edges

### GraphStore — Batch Operations
- AC17: `store_file_data` stores file + N symbols + M edges; all retrievable afterward
- AC18: `store_file_data` is atomic — if any insert fails, none are persisted
- AC19: `remove_file_data` removes file + its symbols + edges referencing those symbols
- AC20: `store_file_data` then `remove_file_data` for the same path leaves zero orphan edges

### FTS5 Search
- AC21: Inserting a symbol (via `upsert_symbol` or `store_file_data`) makes it findable via `search`
- AC22: Deleting a symbol (via `remove_file` CASCADE or `remove_symbols_in_file`) removes it from search results
- AC23: Updating a symbol's name via `upsert_symbol` updates search — old name not found, new name found
- AC24: `search` returns results ordered by relevance (FTS5 rank); exact name match ranks higher than partial

### Thread Safety
- AC25: `SqliteStore` satisfies `Send + Sync` (compile-time assertion)
- AC26: Two threads performing concurrent reads do not error or deadlock

### Error Handling
- AC27: All `rusqlite::Error` values are converted to `CodeGraphError::Storage(message)`

### Domain Trait Changes
- AC28: `GraphStore` trait has `store_file_data` and `remove_file_data` methods
- AC29: `InMemoryGraphStore` implements both new methods
- AC30: `cargo test -p domain` passes (all existing tests green after trait changes)

### Quality
- AC31: `cargo test -p storage` passes with all tests green
- AC32: `cargo clippy -p storage -- -Dwarnings` passes
- AC33: Storage crate depends only on: domain, rusqlite, r2d2_sqlite, r2d2, serde_json

## Design Notes

- **Schema follows design spec Section 5.2 verbatim** — no deviations from the SQL DDL. The `metadata` table is created but not used for schema versioning (we use `PRAGMA user_version` instead); it is available for future key-value storage needs.
- **`SearchIndex::index_symbol` is a no-op** because FTS5 triggers handle sync. The method satisfies the trait contract. If a future slice needs explicit FTS indexing (e.g., for non-triggered bulk loads), the implementation can be added.
- **`SearchIndex::rebuild` is a stub** returning `Ok(())`. Real implementation deferred to when `code-graph index --full` or a repair command needs to rebuild the FTS5 index from scratch.
- **`updated_at` is a storage-only concern** — domain types don't carry timestamps. The storage adapter generates timestamps at write time.
- **Enum mapping lives in the storage crate** to avoid adding dependencies to domain. A new domain enum variant will cause a compile error in storage's match arms — this is the desired safety behavior.
- **Port trait changes are backward-compatible** — existing methods unchanged, two methods added. Per S01 design note: "Port trait signatures are provisional and may be refined in S02-S04."

## Non-Goals

- Implementing `GitProvider` or `FileSystem` traits (done by cli crate)
- Advanced search quality features (custom BM25 weights, trigram fallback, query-aware boosting, context-file boosting)
- Non-parsed file CRUD methods (table exists for schema readiness, methods deferred)
- Connection pool tuning, benchmarking, or multi-process concurrency tests
- Any CLI, parser, or watch functionality
- Performance optimization (premature at this stage)
