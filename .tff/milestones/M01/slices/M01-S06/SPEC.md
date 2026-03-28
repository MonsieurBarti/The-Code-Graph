# Spec — M01-S06: Query Commands

## Problem

After S05, `code-graph index` works but every other command returns "not yet implemented." Users and AI agents cannot interrogate the graph. S06 wires the remaining 8 query/analysis commands so the tool delivers its core value: structural context from indexed codebases.

## Approach

### Domain Changes

- Add `find_by_name(pattern: &str) -> Result<Vec<SymbolNode>>` to `GraphStore` trait + `SqliteStore` impl. Uses SQL: exact match on `name` first, then `name LIKE ?%` prefix fallback.
- Rewrite `QueryUseCase::find` to call `store.find_by_name(pattern)` and return `Result<Vec<SymbolNode>>` (was `get_symbol` returning `Option<SymbolNode>`).
- Add `min_confidence: Confidence` parameter to `ImpactUseCase::diff_impact` (currently hardcoded to `Confidence::Structural`). Propagate to `compute_blast_radius`.
- Change `GitProvider::diff_hunks` signature: `fn diff_hunks(&self, from: &str, to: Option<&str>) -> Result<Vec<DiffHunk>>`. `None` means working tree.

### Git Adapter

- Implement `diff_hunks()` in `ShellGitProvider`: parse `git diff --unified=0` output into `DiffHunk` structs.
- `to: None` maps to `git diff --unified=0 <from>` (working tree). `to: Some(ref)` maps to `git diff --unified=0 <from> <ref>`.

### CLI Commands

- Extract `open_graph() -> Result<(SqliteStore, PathBuf)>` shared helper (project root detection + store opening). All 8 handlers use it — no handler contains inline project-root detection or store-opening logic.
- One handler file per command: `find.rs`, `refs.rs`, `callers.rs`, `callees.rs`, `search.rs`, `stats.rs`, `impact.rs`, `diff.rs`.
- Fix `ImpactArgs`: rename `qualified_name` to `target`, add `--depth` (default: 3, per design doc Section 7.3), add `--confidence` flag (values: `high`, `medium`, `low`, `all`; default: `all` = `Confidence::Structural`).
- Fix `DiffArgs`: make `from` optional (default: `HEAD`), `to` optional (default: working tree). Add `--depth` (default: 3) and `--confidence` (default: `all`).
- Impact target disambiguation heuristic: contains `::` = qualified symbol name, contains `/` or ends with known source extension (`.ts`, `.tsx`, `.js`, `.jsx`, `.rs`, `.py`, `.go`) = file path, otherwise treat as symbol name.
- Find enrichment: CLI handler fetches edges per returned symbol (callers via `get_edges_to` filtered to `Calls`, callees via `get_edges_from` filtered to `Calls`, tested_by via `get_edges_to` filtered to `TestedBy`). This stays in the CLI handler — not the domain use case — because it's a presentation concern.

### Output Formatting

Define `FindResult` struct in the `cli` crate:

```rust
struct FindResult {
    symbol: SymbolNode,
    callers: Vec<String>,   // qualified names
    callees: Vec<String>,   // qualified names
    tested_by: Vec<String>, // qualified names
}
```

Implement `Displayable` for:
- `Vec<FindResult>` — enriched output per symbol
- `Vec<Reference>` — flat reference list (for refs, callers, callees)
- `Vec<SearchResult>` — search results with scores
- `GraphStats` — file/symbol/edge counts
- `ImpactReport` — blast radius report with affected nodes by confidence tier
- `DiffImpactReport` — changed symbols + blast radius

Compact format follows design spec Section 7.2:
- `find`: `Name kind file:lines [flags]\n  -> calls: ...\n  -> tested_by: ...\n  <- callers: ...`
- `refs/callers/callees`: one line per reference: `source_qualified_name (EdgeKind)`
- `search`: one line per result: `qualified_name kind file:lines score=N.NN`
- `stats`: `Files: N | Symbols: N | Edges: N`
- `impact`: header line + one line per affected node grouped by confidence tier
- `diff`: changed symbols section + impact section

## Acceptance Criteria

- **AC1**: `code-graph find <name>` returns symbols matching by exact name, falling back to prefix match if zero exact matches. Each result is annotated with lists of callers, callees, and tested_by edges. Output defaults to compact format per design spec Section 7.2.
- **AC2**: `code-graph refs <qualified_name>` returns all incoming edges to the symbol, one per line with source and edge kind.
- **AC3**: `code-graph callers <qualified_name>` returns incoming `Calls` edges only.
- **AC4**: `code-graph callees <qualified_name>` returns outgoing `Calls` edges only.
- **AC5**: `code-graph search <query>` returns symbols matching the query, ordered by relevance score descending, with each result showing qualified name, kind, file path, and numeric relevance score.
- **AC6**: `code-graph stats` returns file, symbol, and edge counts.
- **AC7**: `code-graph impact <target> [--depth N] [--confidence LEVEL]` disambiguates target as qualified symbol (contains `::`), file path (contains `/` or ends with known source extension), or symbol name (fallback). Returns blast radius listing transitively affected symbols up to `--depth N` (default 3), filtered to `--confidence LEVEL` or above (default: all tiers). Report includes qualified names, depths, and confidence tiers.
- **AC8**: `code-graph diff [from] [to]` parses `git diff --unified=0` output into hunks, identifies symbols whose line ranges overlap changed hunks, then computes blast radius of those symbols. `from` defaults to `HEAD`; `to` defaults to working tree. Supports `--depth` and `--confidence` flags.
- **AC9**: All 8 commands support `--json` (valid parseable JSON) and `--table` (human-readable tabular) output modes in addition to compact (default).

## Non-Goals

- No lazy staleness check on query (deferred to S09).
- No incremental indexing (deferred to S09).
- No `watch`, `setup`, or `eval` commands (deferred to later slices).
- No `context_file` parameter on `search` (design doc Section 3.4 mentions it; deferred to a future enhancement).
- No new crate dependencies.

## Testing Strategy

### Unit Tests

- `diff_hunks` parser: fixtures for add, modify, delete, rename, multi-hunk diffs.
- `find_by_name`: exact match, prefix match, no match, case sensitivity.
- Impact target disambiguation: file path vs symbol heuristic edge cases.
- `Displayable` impls: snapshot-test compact/table/JSON output for each result type.

### Integration Tests

- Per command: create temp git repo with TS/Rust fixtures, index, run command, assert output correctness.
- `--json` flag produces valid parseable JSON for every command.
- Error cases: command on unindexed project, find with no matches, refs for nonexistent symbol.
