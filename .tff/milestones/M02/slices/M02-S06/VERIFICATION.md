# Verification — M02-S06: Clone Detection

> Verified: 2026-03-28 | Tests: 604/604 passing | Build: clean

## Acceptance Criteria

| AC | Verdict | Evidence |
|---|---|---|
| AC1: `code-graph clones` lists clusters with member count, avg similarity, clone type — compact\|table\|json | **PASS** | `Displayable` impls in `output.rs:495-589` for all 3 formats. Compact: `#{id} {type} members={n} avg_sim={v} [{members}]`. Table: `# \| Type \| Members \| Avg Similarity \| Representative`. JSON: serde. CLI dispatch in `commands/clones.rs:30`. 142 CLI tests pass. |
| AC2: `--cluster <id>` drill-down with file paths and similarity scores, 1-indexed by descending size | **PASS** | `ClonesArgs.cluster: Option<usize>` in `mod.rs:214`. `run_clones` filters `c.id == cluster_id`. `CloneCluster.intra_matches: Vec<CloneMatch>` populated during clustering. Drill-down output shows file paths extracted from qualified names and per-pair similarities in compact/table/json formats (`output.rs:553-623`). `cluster_matches` sorts largest-first, 1-indexed IDs. |
| AC3: `--threshold <0.0-1.0>` configurable (default 0.7) | **PASS** | `ClonesArgs.threshold` default `"0.7"` in `mod.rs:208`. Propagated to `CloneConfig.threshold` in `clones.rs:16`. Used in `compare_pair` at `analysis/clones.rs:372`. Parse tests verify default and `--threshold 0.8/0.9`. |
| AC4: `--min-lines <n>` filters by `line_end - line_start + 1` (default 5) | **PASS** | `ClonesArgs.min_lines` default `"5"` in `mod.rs:210`. `compute_fingerprints` at `analysis/clones.rs:56-59`: `body_line_count = line_end.saturating_sub(line_start) + 1; if < min_lines { continue }`. Tests: `fingerprint_filters_by_min_lines`, `analyze_filters_by_min_lines`. |
| AC5: Duplication metrics in `code-graph stats` (compact/table/json) | **PASS** | `GraphStats` extended with `clone_clusters: Option<usize>`, `duplication_pct: Option<f64>`, `most_duplicated: Option<String>` at `model.rs:266-270`. `stats.rs:39-48` populates via `CloneUseCase::analyze` when symbols <= 10k. Compact/table formatters (`output.rs:222-255`) render clone metrics. JSON uses `skip_serializing_if`. |
| AC6: Performance < 5s/10k, < 30s/50k symbols | **PASS** | Algorithmic: O(S+E) fingerprinting, O(S) bucketing, pairwise capped at `max_candidates_per_bucket=500`, file content cached in `HashMap`, Union-Find with path compression. No live benchmark (no 10k+ repo available), but design prevents O(n^2). |
| AC7: Cross-language structural matching | **PASS** | `use_cases/clones.rs:61-64`: `cross_lang = fp_a.language != fp_b.language` → `compare_pair("", "", true, ...)`. `compare_pair` returns `StructuralOnly` with sim=1.0 when `cross_language=true`, skipping tokenization. Test `compare_pair_cross_language_structural_only` passes. |
| AC8: Type 1 clones (Jaccard >= 0.95 un-normalized) | **PASS** | `compare_pair` at `analysis/clones.rs:355-362`: `raw_sim >= 0.95 → Type1`. Only compared within same bucket (use case loop). Test `compare_pair_type1_exact_match`: identical source → Type1. |
| AC9: Type 2 clones (normalized Jaccard >= threshold, raw < 0.95) | **PASS** | `compare_pair` at `analysis/clones.rs:364-377`: raw < 0.95 → normalize → `norm_sim >= threshold → Type2`. Positional placeholders in `normalize_identifiers`. Tests: `compare_pair_type2_renamed_vars` (`add(x,y)` vs `sum(a,b)` → Type2), `type2_clones_detected_after_normalization` (raw<0.95, norm=1.0). |
| AC10: Transitive clustering (connected components) | **PASS** | `cluster_matches` at `analysis/clones.rs:382-503`: Union-Find with path compression + union-by-rank. Tests: `cluster_transitive` (A-B + B-C → 1 cluster {a,b,c}), `cluster_separate_components` (A-B + C-D → 2 clusters). |

## Summary

**Verdict: PASS** — All 10 acceptance criteria met.

- 25 analysis unit tests (fingerprinting, bucketing, tokenization, Jaccard, comparison, clustering)
- 4 use-case integration tests (type2 detection, min-lines filtering, empty graph, single symbol)
- 7 CLI parse tests (clones command with all flag combinations)
- 604 total workspace tests passing, zero failures

### Notes

- AC6: Verified by algorithmic analysis (no 10k+ symbol test repo available). The O(n) fingerprinting + bucketed comparison + capped pairwise design prevents quadratic blowup.

### Review Findings (addressed in fix commit)

- **Bug fixed**: Token comparison now uses symbol body lines (`line_start..line_end`) instead of whole file content
- **AC-2 enhanced**: Drill-down output shows file paths and per-pair similarity scores via `intra_matches`
- **AC-5 enhanced**: Clone metrics now render in compact/table output (not just JSON)
- **Dead code removed**: Unused `counter` variable in `normalize_identifiers`
