# Verification — M02-S05: Dead Code Detection

## Test Evidence

| Suite | Result | Command |
|-------|--------|---------|
| Domain dead_code tests | 17 passed | `cargo test -p domain -- dead_code` |
| CLI dead_code tests | 3 passed | `cargo test -p cli -- dead_code` |
| Full test suite | 370 passed | `cargo test -p domain -p cli` |
| Build | Success | `cargo build -p cli` |
| CLI binary | Compiles, help renders | `code-graph dead-code --help` |

## Acceptance Criteria

| AC | Description | Verdict | Evidence |
|----|-------------|---------|----------|
| AC1 | Dead symbols with zero usage-edges, excluding entry points/exports/tests/migrations | **PASS** | `detect_dead_code()` builds alive set from `USAGE_EDGES`, applies 5 exclusion layers in order. Tests: `unused_symbol_detected`, `used_symbol_alive`, `exported_symbol_excluded`, `test_function_excluded_by_default`, `migration_file_excluded`, `structural_edges_do_not_count_as_usage`, `tested_by_does_not_count_as_usage` |
| AC2 | Entry point detection reuses `detect_entry_points()` from flow analysis | **PASS** | `dead_code.rs` imports and calls `detect_entry_points(symbols, edges, &FlowConfig::default())` from `crate::analysis::flow`. Not duplicated. Tests: `entry_point_test_kind_not_excluded_as_entry_point`, `entry_point_patterns_add_exclusions` |
| AC3 | `--exclude-pattern <glob>` excludes by qualified name or file path | **PASS** | Layer 5 in `detect_dead_code()` matches `user_glob` against both `sym.qualified_name` and `sym.location.file`. CLI wires `args.exclude_pattern` into `dead_config.exclude_patterns`. Test: `user_pattern_excludes_by_qualified_name` |
| AC4 | `--include-tests` flag includes test functions as dead code candidates | **PASS** | Layer 3 skips test exclusion when `config.include_tests == true`. CLI flag `--include-tests` wired in handler. Tests: `test_function_excluded_by_default`, `include_tests_flags_dead_tests`, `use_case_with_include_tests` |
| AC5 | `--kind <kind>` filters results to specific symbol kinds | **PASS** | `kind_filter` applied as display-layer `retain()` after dead symbols computed. CLI parses string kinds to `SymbolKind` variants. Test: `kind_filter_restricts_results` |
| AC6 | Output supports compact, `--table`, `--json` formats via Displayable | **PASS** | `impl Displayable for DeadCodeAnalysis` in `output.rs` provides `fmt_compact`, `fmt_table`, `fmt_json`. Handler calls `print(&analysis, output_format)`. CLI flags `--json`/`--table` declared in global args. |
| AC7 | `[dead-code]` section in config.toml for persistent patterns | **PASS** | `DeadCodeCliConfig` with `exclude_patterns`, `entry_point_patterns`, `migration_patterns` fields. `#[serde(rename = "dead-code")]` maps TOML section. Config merge: defaults -> TOML -> CLI flags (union, not override). Test: `dead_code_config_parses` |
| AC8 | Summary statistics: total, dead count, percentage, by-kind, by-file | **PASS** | `DeadCodeSummary` has all fields. `dead_by_file` sorted desc by count. Div-by-zero guard for empty graph. Tests: `summary_statistics_correct`, `empty_graph_returns_zero_percentage` |

## Architecture Compliance

- **Hexagonal pattern:** Analysis module (pure function) -> Use case (GraphStore integration) -> CLI command (output + config merge). Domain has zero CLI/infra dependencies.
- **Complexity:** O(E + S) as specified — single pass over edges, single pass over symbols.
- **Exclusion layer order:** Entry points -> Exported -> Tests -> Migrations -> User patterns. First match wins. Test: `exclusion_layer_order_first_match_wins`.

## Verdict: PASS

All 8 acceptance criteria met. 20 targeted tests + 370 full suite tests pass. Implementation follows hexagonal architecture and spec algorithm.
