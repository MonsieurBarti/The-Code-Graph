# M02-S01: Execution Flows — Verification

**Verdict: PASS** (12/12 criteria met)

| AC | Verdict | Evidence |
|---|---|---|
| AC1: `code-graph flows` lists detected execution flows from auto-detected entry points | **PASS** | `cargo run -- flows` exits 0, outputs `Entry points: 51 detected (50 public-root, 1 main)` and `Flows: 66 total, showing 20` with 20 flow lines showing entry->path traversals. |
| AC2: `code-graph flows --symbol <name>` filters flows passing through a specific symbol | **PASS** | `cargo run -- flows --symbol "crates/domain/src/model.rs::QualifiedName"` returns 5 flows all containing that symbol. Uses backward BFS + forward DFS filtering in `FlowUseCase::flows_through`. Unit test `flows_through_filters_correctly` passes. |
| AC3: `code-graph flows --rank` outputs symbols ranked by betweenness centrality | **PASS** | `cargo run -- flows --rank` exits 0, outputs ranked list with `betweenness=` scores, `flows=` counts, and `entry=yes/no` labels. `FlowUseCase::criticality()` sorts by betweenness descending. Unit test `criticality_returns_sorted_scores` verifies ordering. |
| AC4: Entry points auto-detected from: main, test functions, HTTP handler decorators, public functions with no callers | **PASS** | `detect_entry_points` handles all 5 kinds: Main, Test, HttpHandler, CliCommand, PublicRoot. 13 unit tests pass covering all variants including edge cases (tokio::main, test_ prefix, non-callable kinds excluded from PublicRoot). |
| AC5: Entry point overrides configurable via `.code-graph/config.toml` | **PASS** | `FlowsConfig` has `extra_entry_points` and `excluded_entry_points`. Config test `flows_config_parses` verifies TOML parsing. `detect_entry_points` respects both: tests `excluded_entry_points_filtered` and `extra_entry_points_added` pass. |
| AC6: Criticality scores normalized to [0.0, 1.0] via Brandes' betweenness centrality | **PASS** | `brandes_betweenness` normalizes by `(n-1)(n-2)` directed graph factor. Test `brandes_normalization_directed` verifies center of 5-node linear graph = 1/3 (4/12). All 7 brandes tests pass. All scores in [0, 1]. |
| AC7: Flow traversal uses High confidence edges | **PASS** | `is_high_confidence()` filters to Calls, Extends, Implements, Embeds. Used in brandes, enumerate_flows, and flows_through backward BFS. Tests verify ImportsFrom edges excluded from all three paths. |
| AC8: Flow enumeration bounded by depth (default 20) and count (default 1000) | **PASS** | `FlowConfig::default()` sets max_depth=20, max_flows=1000. Tests `enumerate_flows_depth_limit` (max_depth=5) and `enumerate_flows_global_cap` (max_flows=10) verify bounds enforced. visit_budget=100,000 provides additional safety. |
| AC9: Cycle detection prevents infinite loops (per-path visited set) | **PASS** | DFS uses per-path `HashSet<String>`. Neighbors filtered by `!visited.contains(n)`. Test `enumerate_flows_cycle_detection` creates A->B->A cycle, asserts no duplicates in any flow path. |
| AC10: `code-graph stats` shows entry point count and average criticality | **PASS** | `cargo run -- stats` outputs `Entry points: 51 | Avg criticality: 0.000`. JSON confirms `"entry_point_count": 51, "avg_criticality": 0.0`. On-demand computation in stats.rs. |
| AC11: All three output formats supported: compact, --table, --json | **PASS** | All 6 combinations tested: flows compact/table/json, rank compact/table/json. 40 output format unit tests pass. |
| AC12: Works on the existing test fixtures / dogfood codebase | **PASS** | `cargo run -- index` indexed 121 files, 1055 symbols, 1435 edges. All commands run successfully. 574 tests pass across all crates. |

## Test Evidence

- `cargo test`: 574 tests, 0 failures
- `cargo run -- flows`: 51 entry points, 66 flows
- `cargo run -- flows --rank`: ranked list with betweenness scores
- `cargo run -- stats`: entry_point_count=51, avg_criticality=0.0
- All output formats (compact/table/json) produce valid output
