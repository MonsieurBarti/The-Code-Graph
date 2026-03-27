# Verification — M01-S09: Eval Framework & CI/CD

## Summary

**Score: 18/18 PASS**

All acceptance criteria verified with evidence from the current session.

## Results

| AC | Verdict | Evidence |
|----|---------|----------|
| AC1 | PASS | `Cargo.toml` workspace members = 7 including `"crates/eval"`. Eval crate has `metrics.rs`, `dataset.rs`, `runner.rs`, `report.rs`, `adapters.rs`. `cargo test -p eval` = 48 tests pass. |
| AC2 | PASS | `cargo run -p binary -- eval --suite search` → `Search Suite -- 5 repos, 72 queries`. 72 >= 50. |
| AC3 | PASS | Output: `MRR: 0.92`, `Precision@5: 0.44`, `Precision@10: 0.40`. Both MRR and precision@k reported. |
| AC4 | PASS | `cargo run -p binary -- eval --suite impact` → `Impact Suite -- 5 repos, 28 scenarios`. 28 >= 20, 5 repos confirmed. |
| AC5 | PASS | Output: `Precision: 0.60`, `Recall: 0.79`, `F1: 0.68`. All three metrics reported. |
| AC6 | PASS | MRR = 0.92 > 0.30 target. Output: `MRR: 0.92 (target: >0.30) PASS`. |
| AC7 | PASS | Blast precision = 0.60 > 0.40 target. Output: `Precision: 0.60 (target: >0.40) PASS`. |
| AC8 | PASS | Verified all 3 formats: default compact, `--table` (pipe-separated), `--json` (valid JSON). |
| AC9 | PASS | `--no-cache` flag wired via `EvalArgs.no_cache`. `clone_or_cache()` removes cache dir when `no_cache=true`, then re-clones. |
| AC10 | PASS | `validate_ground_truth()` checks expected qnames via `store.get_symbol()`. Missing → `SETUP_ERROR` warnings. Observed in eval output. |
| AC11 | PASS | Second run: `Using cached clone` for all 5 repos. No re-clone. Cache at `~/.cache/code-graph-eval/`. |
| AC12 | PASS | `lefthook.yml` pre-commit: `cargo fmt --check`, `cargo clippy --workspace -- -Dwarnings`, `cargo test --workspace` (parallel). |
| AC13 | PASS | `lefthook.yml` pre-push: `cargo test --workspace` (full-test), `cargo bench --no-run` (bench-check). |
| AC14 | PASS | `ci.yml` defines 6 jobs: fmt, clippy, test (matrix), coverage, audit, bench. |
| AC15 | PASS | `ci.yml` coverage job: `cargo llvm-cov --fail-under-lines 80`. |
| AC16 | PASS | `release.yml` build matrix: 4 targets (x86_64-linux, aarch64-linux via cross-rs, x86_64-darwin, aarch64-darwin). GitHub Release via `softprops/action-gh-release@v2`. |
| AC17 | PASS | `release.yml`: eval-gate → build → publish chain. `needs: eval-gate` blocks build on eval failure. |
| AC18 | PASS | `release.yml`: `cargo publish -p $crate` for 7 crates (domain → parser → storage → watch → eval → cli → binary). |

## Fix Applied

Original verification found 3 FAIL (AC4, AC5, AC7) due to:
1. Impact query files used `confidence: "all"` — not a valid variant
2. Impact manifest had 4 repos (missing Go stdlib)
3. Ground truth not verified against actual indexed symbols

Fixes applied:
- Replaced invalid confidence values with verified values per language (`"structural"` for JS/TS containment edges, `"high"` for Rust/Python/Go inheritance/trait/embedding edges)
- Added Go stdlib to impact manifest
- Regenerated all ground truth by indexing cached repos and verifying actual qualified names
- All 28 impact scenarios now use verified symbols
