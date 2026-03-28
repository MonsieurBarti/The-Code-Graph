# M01-S01: Workspace Scaffold & Domain Model

## Problem

The Code Graph needs a foundational Rust workspace and domain crate that defines all business types, port traits, and pure analysis logic. This is the base layer that all other crates depend on — it must be correct, complete, and have zero unnecessary dependencies.

## Approach

Create a Cargo workspace with a single `domain` crate (other crates created by their own slices). The domain crate implements the full domain model from the design spec: types, error hierarchy, port traits, use-case structs, and traversal/analysis logic. Use trait-generic use cases (monomorphized at compile time) for maximum performance.

## Scope

### In Scope
- Cargo workspace root `Cargo.toml` with `members = ["crates/domain"]`
- Domain crate with all types from design spec Sections 3.1-3.9
- All 16 edge types with confidence tier classification
- 4 outbound port traits: GraphStore, SearchIndex, GitProvider, FileSystem
- 3 use-case structs: IndexUseCase, QueryUseCase, ImpactUseCase (trait-generic)
- InMemoryGraph with BFS/DFS traversal and confidence-filtered traversal
- Blast radius analysis, change detection, impact analysis
- CodeGraphError hierarchy with thiserror
- All public types derive Serialize + Deserialize

### Not In Scope
- Non-domain crates (created by S02-S09)
- Trait implementations (created by adapter crates)
- CLI, storage, parser, watch functionality
- Any external dependencies beyond serde (derive) and thiserror

**Note:** The design spec lists "only serde derive" for the domain crate. We add `thiserror` as an intentional deviation — it is a proc-macro crate with zero runtime footprint, used solely for ergonomic error derives.

## Design

### Crate Structure

```
the-code-graph/
  Cargo.toml                    # workspace root
  crates/
    domain/
      Cargo.toml                # deps: serde (derive), thiserror
      src/
        lib.rs                  # re-exports
        error.rs                # CodeGraphError
        model.rs                # all domain types
        ports.rs                # outbound port traits
        traversal.rs            # InMemoryGraph, BFS, DFS
        use_cases/
          mod.rs
          index.rs              # IndexUseCase<S, F, G>
          query.rs              # QueryUseCase<S, I>
          impact.rs             # ImpactUseCase<S>
        analysis/
          mod.rs
          blast_radius.rs       # BFS w/ confidence tiers
          change_detection.rs   # DiffHunk -> overlapping symbols
          impact.rs             # transitive impact
```

### Domain Types (model.rs)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language { TypeScript, JavaScript, Rust, Python, Go }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind { File, Symbol, NonParsed }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Function, Class, Interface, Struct, Trait, Enum, TypeAlias,
    Method, Property, Const, Macro, Variable, Component, Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NonParsedKind { Doc, Config, CI, Asset, Other }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Visibility { Public, Private, Crate }

// Variant order is load-bearing for derived Ord: Structural < Low < Medium < High.
// `bfs_filtered(min_confidence = Structural)` includes all edges.
// `bfs_filtered(min_confidence = High)` includes only High-confidence edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Confidence { Structural, Low, Medium, High }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Contains, ChildOf, Calls, ImportsFrom, Extends, Implements,
    TestedBy, DependsOn, BarrelReExportAll, ConditionalImport,
    SideEffectImport, DotImport, HasDecorator, Embeds,
    TypeReference, ReExport,
}

impl EdgeKind {
    pub fn confidence(&self) -> Confidence {
        // High: Calls, Extends, Implements, Embeds
        // Medium: ImportsFrom, BarrelReExportAll, ReExport, TypeReference, DotImport
        // Low: DependsOn, ConditionalImport, SideEffectImport
        // Structural: Contains, ChildOf, HasDecorator, TestedBy
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub file: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub col_start: usize,
    pub col_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub path: PathBuf,
    pub language: Language,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolNode {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub location: Location,
    pub visibility: Visibility,
    pub is_exported: bool,
    pub is_async: bool,
    pub is_test: bool,
    pub decorators: Vec<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonParsedNode {
    pub path: PathBuf,
    pub file_kind: NonParsedKind,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    File(FileNode),
    Symbol(SymbolNode),
    NonParsed(NonParsedNode),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub kind: EdgeKind,
    pub source: String,
    pub target: String,
    pub metadata: Option<String>,
}

// --- Supporting types used in port traits and use cases ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction { Forward, Backward }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImpactTarget {
    File(PathBuf),
    Symbol(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalResult {
    pub node: String,          // qualified_name
    pub depth: usize,
    pub path: Vec<String>,     // traversal path
    pub edge_kind: EdgeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub qualified_name: String,
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: PathBuf,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub source: String,        // qualified_name of referrer
    pub edge_kind: EdgeKind,
    pub location: Option<Location>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub symbols_extracted: usize,
    pub edges_created: usize,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub files: usize,
    pub symbols: usize,
    pub edges: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub file: PathBuf,
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffectedNode {
    pub qualified_name: String,
    pub depth: usize,
    pub confidence: Confidence,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactReport {
    pub targets: Vec<ImpactTarget>,
    pub affected: Vec<AffectedNode>,
    pub depth: usize,
    pub min_confidence: Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffImpactReport {
    pub changed_symbols: Vec<SymbolNode>,
    pub impact: ImpactReport,
}

// Node identifier convention:
// - FileNode: path.to_string_lossy() as identifier
// - SymbolNode: qualified_name field
// - NonParsedNode: path.to_string_lossy() as identifier
impl Node {
    pub fn id(&self) -> &str { /* returns the appropriate identifier */ }
}

// Qualified name validation (spec Section 3.5):
// Grammar: file_path "::" symbol_path, where symbol_path = segment ("." segment)*
// A newtype or validation function enforces this at construction time.
// Split on FIRST "::" occurrence. File paths containing "::" are unsupported.
pub struct QualifiedName(String);
impl QualifiedName {
    pub fn parse(s: &str) -> Result<Self>;  // validates grammar, rejects: empty, missing "::", empty file_path, empty symbol_path
    pub fn file_path(&self) -> &str;        // part before first "::"
    pub fn symbol_path(&self) -> &str;      // part after first "::"
    pub fn as_str(&self) -> &str;
}
```

### Error Hierarchy (error.rs)

```rust
// CodeGraphError does NOT derive Serialize/Deserialize because it contains
// std::io::Error (which does not implement serde traits). Errors are for
// propagation and display, not serialization. AC9 excludes this type.
#[derive(Debug, thiserror::Error)]
pub enum CodeGraphError {
    #[error("parse error in {file}: {message}")]
    Parse { file: PathBuf, message: String },
    #[error("resolution error: {0}")]
    Resolution(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("git error: {0}")]
    Git(String),
    #[error("file system error: {path}: {source}")]
    FileSystem { path: PathBuf, source: std::io::Error },
    #[error("no project found (no .git directory)")]
    NoProject,
    #[error("refused to index blocklisted root: {0}")]
    BlocklistedRoot(PathBuf),
    #[error("index not built — run `code-graph index` first")]
    IndexNotBuilt,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CodeGraphError>;
```

### Port Traits (ports.rs)

All traits require `Send + Sync` for thread-safe concurrent access:

- `GraphStore` — 10 methods for CRUD on files, symbols, edges
- `SearchIndex` — 3 methods for FTS operations
- `GitProvider` — 3 methods wrapping git CLI
- `FileSystem` — 3 methods for file I/O

All return `Result<T, CodeGraphError>`.

### Use Cases

Trait-generic, monomorphized at compile time:

- `IndexUseCase<S: GraphStore, F: FileSystem, G: GitProvider>` — `full_index`, `incremental_index`
- `QueryUseCase<S: GraphStore, I: SearchIndex>` — `find`, `refs`, `callers`, `callees`, `search`, `stats`
  (Two generic params: `S` for graph queries, `I` for FTS. Both are implemented by the storage crate on a single type, but keeping them as separate trait bounds preserves flexibility.)
- `ImpactUseCase<S: GraphStore>` — `blast_radius`, `diff_impact`

### Traversal (traversal.rs)

`InMemoryGraph` built from `Vec<Edge>` with outgoing + incoming adjacency maps:
- `bfs(start, direction, max_depth)` — breadth-first with depth limit
- `dfs(start, direction)` — depth-first with cycle detection
- `bfs_filtered(start, direction, max_depth, min_confidence)` — BFS excluding edges below confidence threshold

### Analysis

- **blast_radius.rs** — BFS through edges from targets, configurable depth + confidence, returns `ImpactReport`
- **change_detection.rs** — map `DiffHunk` line ranges to overlapping `SymbolNode` locations. Uses **post-diff (new) line numbers** against current symbol locations (which reflect the file state after the diff). For pure deletions (`new_count = 0`), symbols overlapping the `old_start..old_start+old_count` range in the previous file state are marked as affected.
- **impact.rs** — combines change detection + blast radius for `DiffImpactReport`

## Acceptance Criteria

- AC1: `cargo build` succeeds with workspace root + domain crate
- AC2: All 16 EdgeKind variants defined with correct confidence tier mapping (unit test asserts each variant's tier)
- AC3: All 4 port traits compile with Send + Sync bounds
- AC4: InMemoryGraph BFS returns correct nodes at each depth level (unit test with hand-built graph)
- AC5: `bfs_filtered` with `min_confidence = High` excludes edges classified as Medium, Low, or None
- AC6: DFS traversal detects cycles without infinite loops (unit test with cyclic graph)
- AC7: change_detection correctly maps DiffHunks to overlapping SymbolNodes (line range intersection)
- AC8: Each CodeGraphError variant's Display output contains its context fields (e.g., file path, message)
- AC9: All public types (except CodeGraphError) derive Serialize + Deserialize (round-trip test)
- AC10: Domain crate has zero dependencies beyond serde and thiserror (Cargo.toml inspection)
- AC11: `cargo test -p domain` passes (all tests green)
- AC12: Every public function in traversal.rs and analysis/ has at least one unit test exercising its primary path
- AC13: `QualifiedName::parse` validates the grammar `file_path "::" symbol_path` and rejects malformed inputs
- AC14: `ImpactTarget` enum exists with `File(PathBuf)` and `Symbol(String)` variants
- AC15: All supporting types (SearchResult, Reference, IndexStats, GraphStats, ImpactReport, DiffImpactReport, DiffHunk, TraversalResult, Direction, AffectedNode) are defined and derive Serialize + Deserialize
- AC16: `ImpactUseCase::blast_radius` with mock GraphStore returns an ImpactReport containing the transitive closure of affected nodes (integration test)
- AC17: `ImpactUseCase::diff_impact` with mock GraphStore maps DiffHunk inputs to affected symbols and returns a DiffImpactReport (integration test)
- AC18: BFS/DFS on empty graph returns empty Vec, does not panic
- AC19: Self-referential edge (source == target) handled gracefully (node appears once in results)
- AC20: DiffHunk with `new_count = 0` (pure deletion) still identifies affected symbols
- AC21: `diff_impact` with non-overlapping hunks returns empty report, not an error

## Design Notes

- **Port trait signatures are provisional** and may be refined in S02-S04 as real adapters reveal needs.
- **v0.1 accepts full edge loading** via `InMemoryGraph::from_edges`. Streaming/lazy loading is a v0.2 optimization.
- **Use-case mock strategy:** A shared `InMemoryGraphStore` test double lives in domain's `#[cfg(test)]` module and is reusable across slices. It implements both `GraphStore` and `SearchIndex`.
- **Any additional domain dependency** beyond serde and thiserror requires explicit justification in the slice spec.

## Non-Goals

- Implementing any port trait (done by adapter crates in later slices)
- Parser logic, SQL queries, file watching, CLI commands
- Performance optimization (premature at this stage)
- Any runtime dependencies beyond serde + thiserror
