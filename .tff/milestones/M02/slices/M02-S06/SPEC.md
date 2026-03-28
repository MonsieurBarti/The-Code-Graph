# Spec — M02-S06: Clone Detection

## Problem Statement

Codebases accumulate duplicated logic over time — copy-pasted functions, similar utility code across modules, structurally identical patterns in different files. This increases maintenance burden, bug surface, and wastes AI agent tokens when indexing redundant symbols.

## Goals

1. Detect structurally similar code (Type 1-2) using graph fingerprints + token similarity
2. Group clones into clusters for refactoring guidance
3. Provide codebase-level duplication metrics (health scoring)
4. Expose clone equivalence classes for AI agent context deduplication
5. Handle large repos efficiently via LSH bucketing (avoid O(n²))
6. Support cross-language structural matching with same-language refinement

## Non-Goals (This Slice)

- Semantic clone detection (Type 3-4) — deferred to post-M02-S04 (Embeddings + Hybrid Search)
- Auto-refactoring or code generation suggestions
- IDE integration or real-time detection during editing

## Approach: Hybrid — Graph Fingerprints + Token Similarity

Two-phase detection pipeline with configurable thresholds.

### Phase 1 — Candidate Generation (Graph Fingerprints)

For each symbol, compute a `StructuralFingerprint`:
- `symbol_kind` (Function, Method, Class, etc.)
- `callee_count` (out-degree to high-confidence edges)
- `caller_count` (in-degree from high-confidence edges)
- `edge_kind_set` (bitset of edge types connected)
- `body_line_count` (computed: `location.line_end - location.line_start + 1`)
- `child_count` (number of Contains edges)

**Note on `param_count`:** `SymbolNode.signature` is `Option<String>` and is `None` for most languages (Python, Go, TS). Rather than parsing signatures, `param_count` is **excluded** from the fingerprint. The remaining 6 features provide sufficient discriminative power. If signature parsing improves in future parser versions, `param_count` can be added as a 7th feature.

**Bucketing strategy — Quantize-and-Concatenate:**
1. Quantize each numeric feature into discrete bins: `callee_count` → {0, 1-2, 3-5, 6+}, `caller_count` → same bins, `body_line_count` → {1-5, 6-15, 16-50, 51+}, `child_count` → {0, 1-3, 4+}
2. Concatenate: `(symbol_kind, callee_bin, caller_bin, line_bin, child_bin)` → bucket key
3. Symbols with identical bucket keys are candidates for pairwise comparison
4. Within each bucket, compute a finer-grained fingerprint distance using the raw counts to rank pairs

This is simpler and more predictable than MinHash/SimHash for a low-dimensional discrete feature vector. The bin widths are tunable via `CloneConfig`.

### Phase 2 — Refinement (Token Jaccard Similarity)

For each candidate pair from Phase 1:
1. Read source text via `FileSystem` port (see Architecture below)
2. Tokenize: split on whitespace + punctuation, strip comments
3. Normalize for Type 2 detection: replace identifier tokens with positional placeholders (`_1`, `_2`, ...) preserving binding patterns (same identifier → same placeholder). Keywords, operators, and literals are kept as-is.
4. Compute Jaccard similarity on normalized token multisets
5. Pairs above configurable threshold (default: 0.7) become confirmed clones
6. Cross-language pairs: structural matching only (skip token refinement)

**Why positional placeholders over single `_`:** Single-placeholder normalization would make `fn add(a, b) { a + b }` identical to `fn mul(a, b) { a * b }` since operators are separate tokens but variable bindings collapse. Positional placeholders preserve the pattern that `a` maps to `_1` everywhere, detecting renamed-variable clones without false-matching different logic.

### Clustering

- Build undirected graph of confirmed clone pairs
- Connected components = clone clusters
- Each cluster gets: member list, average similarity, representative member

## Architecture

### Output Types

| Type | Description |
|------|-------------|
| `StructuralFingerprint` | Symbol's structural profile for bucketing |
| `CloneMatch` | Pair of symbols + similarity score + clone type |
| `CloneCluster` | Group of related clones + stats |
| `CloneAnalysis` | All clusters + summary metrics (duplication %, clone density) |
| `CloneConfig` | Thresholds, min lines, max candidates per bucket |

### Port Dependencies

`CloneUseCase<S: GraphStore, F: FileSystem>` — requires both ports:
- `GraphStore` for symbols, edges, and stats (Phase 1 + clustering)
- `FileSystem` for reading source files (Phase 2 token refinement)

This follows the existing pattern: `IndexUseCase` already takes `(S, P, F)` with `FileSystem`.

### File Structure

| File | Responsibility |
|------|----------------|
| `crates/domain/src/analysis/clones.rs` | Fingerprinting, bucketing, Jaccard similarity, clustering |
| `crates/domain/src/use_cases/clones.rs` | `CloneUseCase<S, F>` orchestrating store + filesystem queries + algorithms |
| `crates/cli/src/commands/clones.rs` | CLI handler for `code-graph clones` |
| `crates/domain/src/model.rs` | New types (listed above) + extend `GraphStats` with optional clone fields |

### CLI Interface

```
code-graph clones [OPTIONS]
  --threshold <0.0-1.0>   Similarity threshold (default: 0.7)
  --min-lines <n>          Minimum symbol body lines (default: 5)
  --cluster <id>           Show detailed members of a specific cluster
  --format <fmt>           Output format: compact|table|json
```

Duplication metrics also added to `code-graph stats` output.

## Acceptance Criteria

1. **`code-graph clones`** lists all clone clusters with member count, average similarity, and clone type — supports `--format compact|table|json`
2. **`code-graph clones --cluster <id>`** shows detailed members of a specific clone cluster with file paths and similarity scores. Cluster IDs are 1-indexed sequential integers assigned in descending order of cluster size (largest cluster = 1).
3. **`code-graph clones --threshold <0.0-1.0>`** allows configurable similarity threshold (default 0.7)
4. **`code-graph clones --min-lines <n>`** filters symbols whose `body_line_count` (computed as `line_end - line_start + 1` from `Location`) is less than `n` (default: 5)
5. **Duplication metrics** included in `code-graph stats` output: total clone clusters, duplication percentage (= symbols appearing in any clone cluster / total symbols × 100), most-duplicated symbol (= symbol with most clone pairs). Extends `GraphStats` with optional `clone_clusters`, `duplication_pct`, `most_duplicated` fields.
6. **Performance**: completes in < 5s for repos with 10k symbols, < 30s for 50k symbols, measured on GitHub Actions `ubuntu-latest` runners (or equivalent: 2-core, 7GB RAM)
7. **Cross-language structural matching**: symbols from different languages can appear in the same cluster based on graph fingerprints (token refinement skipped for cross-language pairs)
8. **Type 1 clones** (identical bucket key AND token Jaccard ≥ 0.95 on un-normalized tokens) detected and labeled as Type 1
9. **Type 2 clones** (identical bucket key AND token Jaccard ≥ 0.7 on positionally-normalized tokens, but < 0.95 on un-normalized tokens) detected and labeled as Type 2
10. **Clone clusters are transitive**: if A≈B and B≈C, all three appear in one cluster (connected components)

## Error Handling & Edge Cases

- **Empty graph**: Return empty `CloneAnalysis` with zero clusters, 0% duplication
- **Single symbol**: No pairs possible — same as empty
- **All symbols identical**: One cluster containing all members
- **Very large buckets** (> 1000 symbols in one LSH bucket): Cap pairwise comparisons at `max_candidates_per_bucket` (default: 500), sample randomly, log a warning that results may be incomplete
- **Missing source files** (for token refinement): Fall back to Phase 1 structural similarity only, mark clone type as "structural-only"
- **Cross-language pairs**: Skip token refinement phase, use fingerprint similarity only, clearly labeled as cross-language match
- **Symbols without signatures** (e.g., constants, type aliases): Use reduced fingerprint (kind + edge profile only), lower default similarity threshold

## Testing Strategy

### Unit Tests (analysis/clones.rs)
- Fingerprint generation from known symbol/edge data
- LSH bucketing correctness (similar fingerprints → same bucket)
- Jaccard similarity calculation (known token sets → expected scores)
- Identifier normalization for Type 2 detection
- Connected component clustering (pairs → expected clusters)
- Edge cases: empty input, single symbol, all identical, missing signatures

### Integration Tests (use_cases/clones.rs)
- `CloneUseCase` against `InMemoryGraphStore` with synthetic clone pairs
- Threshold filtering (clones below threshold excluded)
- Min-lines filtering (small symbols excluded)
- Cross-language pair handling (structural-only matching)
- Stats integration (duplication % in GraphStats)

### CLI Tests (commands/clones.rs)
- Output format validation (compact, table, JSON)
- `--cluster <id>` drill-down
- `--threshold` and `--min-lines` flags

## Future Extensions (Post-M02-S04)

- Type 3-4 semantic clone detection via embedding cosine similarity (replaces token Jaccard in Phase 2)
- Clone evolution tracking across git history
- Suggested refactoring targets based on clone cluster size and coupling
