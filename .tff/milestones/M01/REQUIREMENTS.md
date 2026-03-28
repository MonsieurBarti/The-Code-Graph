# Requirements — v0.1 Core

## Goal
Deliver a fully functional Rust CLI tool that indexes codebases into a queryable dependency graph with tree-sitter parsing, SQLite storage, incremental updates, and blast radius analysis.

## Requirements

### R1: Workspace Architecture
- Six-crate Cargo workspace: domain, parser, storage, watch, cli, binary
- Hexagonal boundaries enforced by compiler (Cargo.toml dependency rules)
- Unified error hierarchy (`CodeGraphError`) propagated across crate boundaries
- Structured logging via `tracing` crate with configurable verbosity

### R2: Domain Model
- Three node kinds: File, Symbol, NonParsed
- 16 edge types (Contains, Calls, ImportsFrom, Extends, Implements, etc.)
- Outbound port traits: GraphStore, SearchIndex, GitProvider, FileSystem
- Inbound use cases: IndexUseCase, QueryUseCase, ImpactUseCase
- Qualified name format: `file_path::symbol_path` with formal grammar
- Visibility mapping per language (Public, Private, Crate)
- Confidence tiers for edge classification (High, Medium, Low, None)
- InMemoryGraph with BFS/DFS traversal

### R3: Tree-Sitter Parsing
- Language support: TypeScript/TSX, JavaScript/JSX, Rust, Python, Go
- Thread-local parsers with `thread_local!` + `RefCell<Parser>` for rayon
- Two-phase: parse then resolve (RawImport → resolved edges)
- Graceful parse failure (skip file, retain previous state, log warning)
- Extract: symbols, edges, imports, exports per file

### R4: Import Resolution
- TypeScript/JS: oxc_resolver for file resolution + custom barrel chain traversal
- Rust: crate-root module tree walk, Cargo workspace awareness
- Python: package resolution, `__init__.py` handling
- Go: `go.mod` module resolution
- Cross-file call resolution: scoped → qualified → single-candidate fallback → ambiguous = no edge

### R5: SQLite Storage
- Database at `.code-graph/graph.db` with WAL mode + 5s busy timeout
- Tables: metadata, files, non_parsed_files, symbols, edges
- FTS5 virtual table with auto-sync triggers
- BM25 with custom column weights, query-aware boosting, trigram similarity fallback
- Schema migrations via metadata table (versioned, idempotent, forward-only)
- `ON DELETE CASCADE` from symbols → files

### R6: Incremental Updates & Watch
- SHA-256 hash-based change detection
- Incremental pipeline: detect change → hash check → re-parse → re-parse dependents → update store
- Three-layer freshness: daemon (always fresh), hooks (on agent actions), lazy staleness check
- Lazy staleness via `git status --porcelain` (O(changed files) not O(all files))
- Watch daemon with `notify` crate, 100ms debounce, PID/socket files, log rotation
- Project detection: `.git` only, blocklisted roots (/, /home, /Users, etc.)

### R7: CLI Commands
- Commands: index, find, refs, impact, diff, callers, callees, search, stats, watch, setup, eval
- Three output formats: compact (AI-optimized default), table, JSON
- Blast radius with configurable depth and confidence level
- Claude Code hooks: SessionStart, PostToolUse, PreCommit
- `setup` command: install/verify/remove hooks, add `.code-graph/` to `.gitignore`

### R8: Eval Framework
- `code-graph eval` with `--suite search` and `--suite impact`
- Search quality: MRR > 0.50 (vs code-review-graph's 0.35)
- Blast radius precision > 0.55 at high confidence (vs code-review-graph's 0.38)
- Eval dataset: 3+ open-source repos, manually curated query sets (50+ queries)

### R9: CI/CD & Quality
- Lefthook: pre-commit (fmt, clippy, test), pre-push (full test + bench)
- GitHub Actions: PR checks (fmt, clippy, test, coverage, audit), release builds
- `cargo-llvm-cov` with 80% minimum coverage
- Release: linux x86_64/aarch64, macOS x86_64/aarch64, crates.io publish

### R10: Configuration
- Optional `.code-graph/config.toml` per project
- `.code-graphignore` file (gitignore syntax)
- Language-specific resolver settings auto-detected, not configured
