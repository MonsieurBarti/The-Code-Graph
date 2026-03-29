# Research — M03-S07: Real-World Validation

## Current Eval Architecture

### Crate Structure
```
crates/eval/src/
  lib.rs        — entry point, Suite enum (Search|Impact|All), SuiteConfig, run_suite()
  runner.rs     — hardcoded search & impact suite execution (485 lines)
  dataset.rs    — manifest parsing, repo caching via shallow clone (451 lines)
  metrics.rs    — MRR, precision@k, F1, blast metrics (227 lines)
  report.rs     — SuiteResult structs & formatting (327 lines)
  adapters.rs   — EvalFileSystem, EvalParseProvider, NoOpGitProvider (188 lines)
```

**Key finding**: No Suite trait exists. Suites are dispatched via pattern match on an enum in `run_suite()`. Adding 6 new suites requires introducing a trait abstraction.

### Dependencies
- `domain` (ports, use cases, analysis), `parser`, `storage` (SqliteStore)
- `tempfile`, `ignore`, `rayon`, `sha2`, `serde/serde_json`

### Current Quality Gates
- Search: MRR >= 0.30 (hard gate), P@5 informational, P@10 informational
- Impact: Precision >= 0.40 (hard gate), Recall informational, F1 informational

## Existing Eval Datasets

### Repos (same 5 across both suites)
| Repo | Language | Tag |
|---|---|---|
| expressjs/express | JavaScript | v4.22.1 (search) / v4.21.2 (impact) |
| trpc/trpc | TypeScript | v11.0.0 |
| BurntSushi/ripgrep | Rust | 14.1.1 |
| tiangolo/fastapi | Python | 0.115.0 |
| golang/go (stdlib subset) | Go | go1.23.0 |

### Ground Truth Format
**Search** (`eval/suites/search/queries/{language}.json`): 6 files, 50+ queries
```json
{ "query": "SearcherBuilder", "expected": ["crates/searcher/...::SearcherBuilder"], "category": "exact" }
```
Categories: exact, semantic, partial

**Impact** (`eval/suites/impact/queries/{language}.json`): 20+ scenarios
```json
{ "target": "...::Stats", "depth": 3, "confidence": "high", "expected_affected": [...] }
```

### Cache
- Location: `$XDG_CACHE_HOME/code-graph-eval/<repo>/<revision>/`
- Strategy: shallow clone (--depth 1) + `.revision` marker
- Validation: marker file hash check

## CLI Integration

**Command**: `crates/cli/src/commands/eval.rs` (152 lines)
- `tcg eval [--suite search|impact|all] [--no-cache]`
- Parses suite string → enum, creates SuiteConfig, calls `eval::run_suite()`
- Output format controlled by global `--json`/`--table` flags
- Non-zero exit on quality gate failure

**Registration**: `crates/cli/src/commands/mod.rs` — `Commands::Eval(EvalArgs)`

## v0.1 Core Features Under Validation

### Key Types (`domain/src/model.rs`, 1266 lines)
- `Node`: File | Symbol | NonParsed
- `SymbolNode`: qualified_name, name, kind, location, visibility, decorators, signature
- `Edge`: kind (Calls/Imports/Extends/Implements/etc.), source, target, confidence
- `SymbolKind`: Function, Class, Interface, Struct, Trait, Enum, Method, etc.

### Indexing (`domain/src/use_cases/index.rs`, 485 lines)
- **Full**: list files → parallel parse → parallel resolve imports → store atomically
- **Incremental**: git diff → hash check → reparse changed + one-hop dependents
- Supported languages: TypeScript/JavaScript, Rust, Python, Go (tree-sitter)

### GraphStore API (`domain/src/ports.rs`, 182 lines)
Key methods: `upsert_file/symbol/edge`, `get_edges_from/to`, `all_symbols/edges`, `find_by_name`, `stats`, `store_file_data`, `edges_streaming`
- Implementations: `SqliteStore` (production), `InMemoryGraphStore` (testing)

### Search (`storage/src/search_index.rs`)
- FTS5 virtual table `symbols_fts` (name, qualified_name, file_path, signature)
- BM25 ranking, sync triggers on INSERT/DELETE/UPDATE

### Query APIs (`domain/src/use_cases/query.rs`, 380 lines)
- `find(pattern)` — exact → prefix symbol name search
- `refs(qn)` — all edges to symbol
- `callers(qn)` / `callees(qn)` — Calls edges to/from
- `search(query, limit)` — FTS5
- `hybrid_search(query, limit, mode, config)` — RRF fusion (FTS + semantic)

### Impact (`domain/src/use_cases/impact.rs`, 197 lines)
- `blast_radius(targets, depth, confidence)` — transitive closure with confidence threshold
- `diff_impact(hunks, depth, confidence)` — changed symbols + blast radius

### Storage (`storage/src/`)
SQLite WAL mode, schema v2: files, symbols, edges, embeddings, symbols_fts
- Pragmas: WAL, foreign_keys, busy_timeout=5000
- Pool: r2d2

## v0.2 Analysis Features Under Validation

### Execution Flows (`analysis/flow.rs` 865 lines, `use_cases/flow.rs` 285 lines)
- Entry point detection: main(), tests, HTTP handlers, CLI, public root exports
- Depth-limited traversal (max 20), budget-bounded (100k visits)
- `CriticalityScore`: betweenness (normalized 0-1), flow_count, is_entry_point
- `ExecutionFlow`: entry point, path, depth, truncated flag

### Risk Scoring (`analysis/risk.rs` 528 lines, `use_cases/risk.rs` 186 lines)
- `RiskFactors`: criticality (0.30), coupling (0.25), test_gap (0.25), sensitivity (0.20)
- `RiskScore`: qualified_name, composite (0-1), factors breakdown
- Sensitivity: keyword detection (auth, password, crypto, sql, eval, unsafe)

### Community Detection (`analysis/community.rs` 1060 lines, `use_cases/community.rs` 103 lines)
- Louvain modularity optimization on undirected weighted graph
- `Community`: id, name, members, modularity_contribution
- Metrics: modularity, internal/boundary edges, size distribution

### Dead Code (`analysis/dead_code.rs` 408 lines, `use_cases/dead_code.rs` 132 lines)
- Reverse reachability from entry points
- Factors: visibility, test coverage, export status, decorators
- Output: qualified_name, reason, file, kind, confidence

### Clone Detection (`analysis/clones.rs` 1093 lines, `use_cases/clones.rs` 264 lines)
- Structural fingerprinting (kind, callee/caller count, edge bitmask, line/child count)
- Bucketed candidate reduction (O(n) from O(n^2))
- Types: Type1 (identical), Type2 (renamed), StructuralOnly
- Similarity threshold: 0.7 default, scores in [0.0, 1.0]

### Embeddings (`analysis/search.rs` 277 lines, `use_cases/embed.rs` 275 lines)
- ONNX model (all-MiniLM-L6-v2), batch 64, incremental via text hash
- Vector store in SQLite embeddings table (BLOB)
- RRF fusion (k=60) with FTS, kind_boost option

## Key Integration Points

### Eval ↔ Core
- Eval creates `SqliteStore` in tempdir per repo
- Uses `EvalParseProvider` (wraps parser registry) for indexing
- Calls use case functions directly for queries
- `NoOpGitProvider` stubs git operations (no incremental in eval currently)

### New Suites → Existing APIs
| New Suite | Primary API | Crate |
|---|---|---|
| Core | `index()`, `find()`, `stats()` | domain/use_cases |
| Flows | `detect_entry_points()`, `analyze_flows()`, `compute_criticality()` | domain/use_cases/flow |
| Risk | `compute_risk_scores()` | domain/use_cases/risk |
| Analysis (communities) | `detect_communities()` | domain/use_cases/community |
| Analysis (dead code) | `detect_dead_code()` | domain/use_cases/dead_code |
| Analysis (clones) | `detect_clones()` | domain/use_cases/clones |
| Invariants | All of the above (property checks only) | — |
| Bench | All of the above (timing only) | — |

### Test Infrastructure
- `InMemoryGraphStore` + `InMemoryVectorStore` + `InMemoryEmbeddingProvider` in `domain/src/test_support.rs`
- 71 `#[cfg(test)]` modules across crates (unit tests, no property-based)
- No shared integration test fixtures beyond eval cache

## Architectural Gaps

### Gap 1: No Suite Trait
Runner dispatches via enum match. Need trait with `name()`, `run_metrics()`, `run_invariants()` methods. Each new suite implements the trait; runner iterates registered suites.

### Gap 2: No Invariant Framework
Spec calls for structural property checks (scores in range, valid IDs, acyclic paths). No existing infrastructure for property assertions beyond ad-hoc unit tests.

### Gap 3: No Benchmark Harness
No timing infrastructure. Need `Instant`-based measurement with statistical aggregation (p50/p95 over N runs).

### Gap 4: Ground Truth Format Divergence
Search uses `queries/{lang}.json`, impact uses `queries/{lang}.json` with different schema. New suites need `ground-truth/{repo-slug}.json` (per spec). Existing suites keep their format for backward compat.

### Gap 5: Incremental Testing Limitation
`NoOpGitProvider` means eval can't test true incremental indexing with file modifications. Spec acknowledges this — tests idempotency and no-op stability instead.

### Gap 6: CLI Suite Enum Extension
`EvalArgs.suite` is a String parsed to enum. Need to extend enum + parser for 6 new values.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Ground truth curation is labor-intensive | High | Medium | Prioritize search+core first; defer analysis suites |
| Bugs found during validation block progress | High | High | Fix-as-you-go with time-box; defer complex fixes |
| Suite trait refactor breaks existing suites | Medium | High | Keep existing runner paths, add trait alongside |
| Eval repo cache staleness | Low | Low | Use pinned revisions (already done) |
| Performance suite noise | Medium | Low | Multiple runs with p50/p95 aggregation |

## Recommendations for Planning

1. **Phase the work**: (a) Suite trait + refactor runner, (b) core + search enhancement, (c) flows + risk, (d) analysis, (e) invariants + bench
2. **Start with core suite**: Validates the foundation everything else depends on
3. **Invariants run cross-cutting**: Each suite registers its own invariants; meta-suite collects all
4. **Bench suite last**: No ground truth needed, captures baselines after all fixes
5. **Budget for bug fixes**: History suggests real-world validation always surfaces issues
