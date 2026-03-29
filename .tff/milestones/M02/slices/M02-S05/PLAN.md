# M02-S05: Dead Code Detection — Implementation Plan

> For agentic workers: execute task-by-task with TDD.

**Goal:** Detect unused symbols (functions, types, constants with zero incoming usage-edges) and report them with configurable exclusion layers, multiple output formats, and config.toml persistence.

**Architecture:** Hexagonal — analysis module (pure function, no side effects) → use case (GraphStore integration) → CLI command (output formatting + config merge). Domain has zero CLI/infra dependencies.

**Tech Stack:** Rust, `globset` (multi-pattern glob matching), `clap` (CLI), `serde` (serialization), existing domain types from `model.rs`.

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `crates/domain/src/model.rs` | Add `DeadCodeConfig`, `DeadCodeAnalysis`, `DeadSymbol`, `DeadCodeSummary`, `ExclusionReason` |
| Modify | `crates/domain/Cargo.toml` | Add `globset = "0.4"` dependency |
| Create | `crates/domain/src/analysis/dead_code.rs` | Pure `detect_dead_code()` function + unit tests |
| Modify | `crates/domain/src/analysis/mod.rs` | Register `dead_code` module |
| Create | `crates/domain/src/use_cases/dead_code.rs` | `DeadCodeUseCase<S: GraphStore>` + unit tests |
| Modify | `crates/domain/src/use_cases/mod.rs` | Register `dead_code` module |
| Modify | `crates/cli/src/config.rs` | Add `DeadCodeCliConfig` + `[dead-code]` TOML section support |
| Create | `crates/cli/src/commands/dead_code.rs` | CLI handler `run_dead_code()` with config merge |
| Modify | `crates/cli/src/commands/mod.rs` | Add `DeadCodeArgs`, `Commands::DeadCode` variant, parse tests |
| Modify | `crates/cli/src/lib.rs` | Add dispatch arm for `DeadCode` |
| Modify | `crates/cli/src/output.rs` | Implement `Displayable` for `DeadCodeAnalysis` |

---

## Wave 0 (parallel — no dependencies)

### T01: Domain Types

**Files:** Modify `crates/domain/src/model.rs`
**Traces to:** AC1, AC4, AC5, AC8

- [x] Step 1: Add `HashMap` import and dead code types to `model.rs`

Add to imports at top of file:

```rust
use std::collections::HashMap;
```

Add after the `CommunityStats` struct (after line ~679, before the `QualifiedName` section):

```rust
// ---------------------------------------------------------------------------
// Dead code detection types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DeadCodeConfig {
    pub exclude_patterns: Vec<String>,
    pub entry_point_patterns: Vec<String>,
    pub include_tests: bool,
    pub migration_patterns: Vec<String>,
    pub kind_filter: Option<Vec<SymbolKind>>,
}

impl Default for DeadCodeConfig {
    fn default() -> Self {
        Self {
            exclude_patterns: Vec::new(),
            entry_point_patterns: Vec::new(),
            include_tests: false,
            migration_patterns: vec![
                "**/migrations/**".into(),
                "**/migrate/**".into(),
                "**/alembic/**".into(),
                "**/diesel/migrations/**".into(),
            ],
            kind_filter: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeAnalysis {
    pub dead_symbols: Vec<DeadSymbol>,
    pub summary: DeadCodeSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadSymbol {
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line: usize,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeSummary {
    pub total_symbols: usize,
    pub dead_count: usize,
    pub dead_percentage: f64,
    pub excluded_count: usize,
    pub dead_by_kind: HashMap<SymbolKind, usize>,
    pub dead_by_file: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExclusionReason {
    EntryPoint,
    Exported,
    TestFunction,
    MigrationFile,
    UserPattern(String),
}
```

- [x] Step 2: Run `cargo check -p domain`, verify PASS (types compile)
- [x] Step 3: Commit `docs(S05/T01): add dead code domain types`

---

### T02: Add globset Dependency

**Files:** Modify `crates/domain/Cargo.toml`
**Traces to:** AC3 (pattern matching support)

- [x] Step 1: Add `globset` to `[dependencies]` in `crates/domain/Cargo.toml`

```toml
globset = "0.4"
```

- [x] Step 2: Run `cargo check -p domain`, verify PASS
- [x] Step 3: Commit `chore(S05/T02): add globset dependency`

---

### T03: CLI Config

**Files:** Modify `crates/cli/src/config.rs`
**Traces to:** AC7

- [x] Step 1: Write failing test in `crates/cli/src/config.rs`

Add to the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn dead_code_config_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".code-graph");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
[dead-code]
exclude_patterns = ["**/generated/**", "**/proto/**"]
migration_patterns = ["**/migrations/**"]
entry_point_patterns = ["*_handler", "*_endpoint"]
"#,
        )
        .unwrap();
        let config = load_config(tmp.path()).unwrap();
        let dc = config.dead_code.unwrap();
        assert_eq!(
            dc.exclude_patterns.unwrap(),
            vec!["**/generated/**", "**/proto/**"]
        );
        assert_eq!(dc.migration_patterns.unwrap(), vec!["**/migrations/**"]);
        assert_eq!(
            dc.entry_point_patterns.unwrap(),
            vec!["*_handler", "*_endpoint"]
        );
    }
```

- [x] Step 2: Run `cargo test -p cli -- dead_code_config_parses`, verify FAIL (field `dead_code` doesn't exist)

- [x] Step 3: Add `DeadCodeCliConfig` struct and field

After `CommunitiesConfig` struct, add:

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DeadCodeCliConfig {
    pub exclude_patterns: Option<Vec<String>>,
    pub entry_point_patterns: Option<Vec<String>>,
    pub migration_patterns: Option<Vec<String>>,
}
```

Add field to `CodeGraphConfig`:

```rust
    #[serde(rename = "dead-code")]
    pub dead_code: Option<DeadCodeCliConfig>,
```

- [x] Step 4: Run `cargo test -p cli -- dead_code_config_parses`, verify PASS
- [x] Step 5: Commit `feat(S05/T03): add dead-code CLI config section`

---

## Wave 1 (depends on Wave 0)

### T04: Analysis Module — `detect_dead_code()`

**Files:** Create `crates/domain/src/analysis/dead_code.rs`, Modify `crates/domain/src/analysis/mod.rs`
**Traces to:** AC1, AC2, AC3, AC4, AC5

- [x] Step 1: Register module in `crates/domain/src/analysis/mod.rs`

Add line (alphabetical order):

```rust
pub mod dead_code;
```

- [x] Step 2: Create `crates/domain/src/analysis/dead_code.rs` with stub + tests

```rust
use crate::analysis::flow::detect_entry_points;
use crate::model::{
    DeadCodeAnalysis, DeadCodeConfig, DeadCodeSummary, DeadSymbol, Edge, EdgeKind,
    EntryPointKind, FlowConfig, SymbolKind, SymbolNode,
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::{HashMap, HashSet};

/// Edge kinds that represent actual symbol usage.
/// Excludes structural edges (Contains, ChildOf, HasDecorator) and TestedBy.
const USAGE_EDGES: &[EdgeKind] = &[
    EdgeKind::Calls,
    EdgeKind::Extends,
    EdgeKind::Implements,
    EdgeKind::Embeds,
    EdgeKind::ImportsFrom,
    EdgeKind::ReExport,
    EdgeKind::BarrelReExportAll,
    EdgeKind::TypeReference,
    EdgeKind::DotImport,
    EdgeKind::DependsOn,
    EdgeKind::ConditionalImport,
    EdgeKind::SideEffectImport,
];

/// Build a GlobSet from a list of patterns. Returns None if patterns is empty.
fn build_glob_set(patterns: &[String]) -> Option<GlobSet> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

/// Detect dead code: symbols with zero incoming usage-edges, after applying
/// exclusion layers (entry points, exports, tests, migrations, user patterns).
///
/// Complexity: O(E + S) — single pass over edges, single pass over symbols.
pub fn detect_dead_code(
    symbols: &[SymbolNode],
    edges: &[Edge],
    config: &DeadCodeConfig,
) -> DeadCodeAnalysis {
    // TODO: implement
    DeadCodeAnalysis {
        dead_symbols: Vec::new(),
        summary: DeadCodeSummary {
            total_symbols: 0,
            dead_count: 0,
            dead_percentage: 0.0,
            excluded_count: 0,
            dead_by_kind: HashMap::new(),
            dead_by_file: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, EdgeKind, Location, SymbolKind, SymbolNode, Visibility};

    fn make_symbol(name: &str, file: &str) -> SymbolNode {
        SymbolNode {
            name: name.split("::").last().unwrap_or(name).into(),
            qualified_name: name.into(),
            kind: SymbolKind::Function,
            location: Location {
                file: file.into(),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }
    }

    fn make_edge(source: &str, target: &str, kind: EdgeKind) -> Edge {
        Edge {
            kind,
            source: source.into(),
            target: target.into(),
            metadata: None,
        }
    }

    #[test]
    fn unused_symbol_detected() {
        let symbols = vec![make_symbol("src/lib.rs::unused_fn", "src/lib.rs")];
        let edges: Vec<Edge> = vec![];
        let result = detect_dead_code(&symbols, &edges, &DeadCodeConfig::default());
        assert_eq!(result.dead_symbols.len(), 1);
        assert_eq!(
            result.dead_symbols[0].qualified_name,
            "src/lib.rs::unused_fn"
        );
    }

    #[test]
    fn used_symbol_alive() {
        let symbols = vec![make_symbol("src/lib.rs::used_fn", "src/lib.rs")];
        let edges = vec![make_edge(
            "src/main.rs::main",
            "src/lib.rs::used_fn",
            EdgeKind::Calls,
        )];
        let result = detect_dead_code(&symbols, &edges, &DeadCodeConfig::default());
        assert_eq!(result.dead_symbols.len(), 0);
    }

    #[test]
    fn structural_edges_do_not_count_as_usage() {
        let symbols = vec![make_symbol("src/lib.rs::inner_fn", "src/lib.rs")];
        let edges = vec![make_edge(
            "src/lib.rs::Module",
            "src/lib.rs::inner_fn",
            EdgeKind::Contains,
        )];
        let result = detect_dead_code(&symbols, &edges, &DeadCodeConfig::default());
        assert_eq!(
            result.dead_symbols.len(),
            1,
            "Contains edge should not make symbol alive"
        );
    }

    #[test]
    fn tested_by_does_not_count_as_usage() {
        let symbols = vec![make_symbol("src/lib.rs::fn_only_tested", "src/lib.rs")];
        let edges = vec![make_edge(
            "tests/test.rs::test_fn",
            "src/lib.rs::fn_only_tested",
            EdgeKind::TestedBy,
        )];
        let result = detect_dead_code(&symbols, &edges, &DeadCodeConfig::default());
        assert_eq!(
            result.dead_symbols.len(),
            1,
            "TestedBy should not make symbol alive"
        );
    }

    #[test]
    fn exported_symbol_excluded() {
        let mut sym = make_symbol("src/lib.rs::public_api", "src/lib.rs");
        sym.is_exported = true;
        let result = detect_dead_code(&[sym], &[], &DeadCodeConfig::default());
        assert_eq!(result.dead_symbols.len(), 0);
        assert_eq!(result.summary.excluded_count, 1);
    }

    #[test]
    fn test_function_excluded_by_default() {
        let mut sym = make_symbol("src/lib.rs::test_helper", "src/lib.rs");
        sym.is_test = true;
        let result = detect_dead_code(&[sym], &[], &DeadCodeConfig::default());
        assert_eq!(result.dead_symbols.len(), 0);
        assert_eq!(result.summary.excluded_count, 1);
    }

    #[test]
    fn include_tests_flags_dead_tests() {
        let mut sym = make_symbol("src/lib.rs::test_helper", "src/lib.rs");
        sym.is_test = true;
        let config = DeadCodeConfig {
            include_tests: true,
            ..DeadCodeConfig::default()
        };
        let result = detect_dead_code(&[sym], &[], &config);
        assert_eq!(
            result.dead_symbols.len(),
            1,
            "test fn should be flagged when include_tests=true"
        );
    }

    #[test]
    fn migration_file_excluded() {
        let sym = make_symbol("migrations/001.rs::up", "migrations/001.rs");
        let result = detect_dead_code(&[sym], &[], &DeadCodeConfig::default());
        assert_eq!(result.dead_symbols.len(), 0);
        assert_eq!(result.summary.excluded_count, 1);
    }

    #[test]
    fn user_pattern_excludes_by_qualified_name() {
        let sym = make_symbol(
            "src/generated/types.rs::AutoStruct",
            "src/generated/types.rs",
        );
        let config = DeadCodeConfig {
            exclude_patterns: vec!["**/generated/**".into()],
            ..DeadCodeConfig::default()
        };
        let result = detect_dead_code(&[sym], &[], &config);
        assert_eq!(result.dead_symbols.len(), 0);
        assert_eq!(result.summary.excluded_count, 1);
    }

    #[test]
    fn kind_filter_restricts_results() {
        let mut sym_fn = make_symbol("src/lib.rs::dead_fn", "src/lib.rs");
        sym_fn.kind = SymbolKind::Function;
        let mut sym_struct = make_symbol("src/lib.rs::DeadStruct", "src/lib.rs");
        sym_struct.kind = SymbolKind::Struct;
        let config = DeadCodeConfig {
            kind_filter: Some(vec![SymbolKind::Function]),
            ..DeadCodeConfig::default()
        };
        let result = detect_dead_code(&[sym_fn, sym_struct], &[], &config);
        assert_eq!(result.dead_symbols.len(), 1);
        assert_eq!(result.dead_symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn entry_point_test_kind_not_excluded_as_entry_point() {
        // Test entry points are filtered OUT from the entry-point exclusion set
        // so that layer 3 (include_tests) controls test handling
        let mut sym = make_symbol("src/lib.rs::test_main", "src/lib.rs");
        sym.is_test = true;
        sym.kind = SymbolKind::Test;
        // With include_tests=true, test entry points should be flagged as dead
        let config = DeadCodeConfig {
            include_tests: true,
            ..DeadCodeConfig::default()
        };
        let result = detect_dead_code(&[sym], &[], &config);
        assert_eq!(
            result.dead_symbols.len(),
            1,
            "Test entry points should not be excluded via entry point layer"
        );
    }

    #[test]
    fn entry_point_patterns_add_exclusions() {
        let sym = make_symbol("src/api.rs::handle_request", "src/api.rs");
        let config = DeadCodeConfig {
            entry_point_patterns: vec!["**::handle_*".into()],
            ..DeadCodeConfig::default()
        };
        let result = detect_dead_code(&[sym], &[], &config);
        assert_eq!(result.dead_symbols.len(), 0);
        assert_eq!(result.summary.excluded_count, 1);
    }

    #[test]
    fn summary_statistics_correct() {
        let syms = vec![
            make_symbol("src/a.rs::dead1", "src/a.rs"),
            make_symbol("src/a.rs::dead2", "src/a.rs"),
            make_symbol("src/b.rs::dead3", "src/b.rs"),
        ];
        let result = detect_dead_code(&syms, &[], &DeadCodeConfig::default());
        assert_eq!(result.summary.total_symbols, 3);
        assert_eq!(result.summary.dead_count, 3);
        assert!((result.summary.dead_percentage - 100.0).abs() < f64::EPSILON);
        assert_eq!(result.summary.dead_by_kind[&SymbolKind::Function], 3);
        // dead_by_file sorted by count desc
        assert_eq!(result.summary.dead_by_file[0], ("src/a.rs".to_string(), 2));
        assert_eq!(result.summary.dead_by_file[1], ("src/b.rs".to_string(), 1));
    }

    #[test]
    fn empty_graph_returns_zero_percentage() {
        let result = detect_dead_code(&[], &[], &DeadCodeConfig::default());
        assert_eq!(result.summary.total_symbols, 0);
        assert_eq!(result.summary.dead_count, 0);
        assert!((result.summary.dead_percentage - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn exclusion_layer_order_first_match_wins() {
        // Symbol is both exported AND a test — should be excluded as exported
        // (layer 2 before layer 3)
        let mut sym = make_symbol("src/lib.rs::exported_test", "src/lib.rs");
        sym.is_exported = true;
        sym.is_test = true;
        let config = DeadCodeConfig {
            include_tests: true,
            ..DeadCodeConfig::default()
        };
        let result = detect_dead_code(&[sym], &[], &config);
        assert_eq!(
            result.dead_symbols.len(),
            0,
            "exported symbol excluded regardless of include_tests"
        );
        assert_eq!(result.summary.excluded_count, 1);
    }
}
```

- [x] Step 3: Run `cargo test -p domain -- dead_code`, verify FAIL (13 tests fail — stub returns empty)

- [x] Step 4: Implement `detect_dead_code()` — replace the `TODO: implement` stub body with:

```rust
    // 1. Build alive set from usage edges
    let usage_set: HashSet<&EdgeKind> = USAGE_EDGES.iter().collect();
    let alive: HashSet<&str> = edges
        .iter()
        .filter(|e| usage_set.contains(&e.kind))
        .map(|e| e.target.as_str())
        .collect();

    // 2. Detect entry points, filter OUT Test entries (layer 3 handles tests)
    let entry_points = detect_entry_points(symbols, edges, &FlowConfig::default());
    let mut entry_point_names: HashSet<&str> = entry_points
        .iter()
        .filter(|ep| ep.kind != EntryPointKind::Test)
        .map(|ep| ep.qualified_name.as_str())
        .collect();

    // 3. Resolve additional entry_point_patterns from config
    let ep_glob = build_glob_set(&config.entry_point_patterns);
    if let Some(ref gs) = ep_glob {
        for sym in symbols {
            if gs.is_match(&sym.qualified_name) {
                entry_point_names.insert(&sym.qualified_name);
            }
        }
    }

    // 4. Build migration file glob set
    let migration_glob = build_glob_set(&config.migration_patterns);

    // 5. Build user exclusion glob set
    let user_glob = build_glob_set(&config.exclude_patterns);

    // 6. Classify each symbol through exclusion layers
    let mut dead_symbols = Vec::new();
    let mut excluded_count = 0usize;
    let total_symbols = symbols.len();

    for sym in symbols {
        // Alive check — target of at least one usage edge
        if alive.contains(sym.qualified_name.as_str()) {
            continue;
        }

        // Layer 1: Entry points (Main, HttpHandler, CliCommand, PublicRoot — NOT Test)
        if entry_point_names.contains(sym.qualified_name.as_str()) {
            excluded_count += 1;
            continue;
        }

        // Layer 2: Exported symbols
        if sym.is_exported {
            excluded_count += 1;
            continue;
        }

        // Layer 3: Test functions (unless include_tests is set)
        if sym.is_test && !config.include_tests {
            excluded_count += 1;
            continue;
        }

        // Layer 4: Migration files
        if let Some(ref gs) = migration_glob {
            let file_str = sym.location.file.to_string_lossy();
            if gs.is_match(file_str.as_ref()) {
                excluded_count += 1;
                continue;
            }
        }

        // Layer 5: User-configured patterns (match on qualified name or file path)
        if let Some(ref gs) = user_glob {
            let file_str = sym.location.file.to_string_lossy();
            if gs.is_match(&sym.qualified_name) || gs.is_match(file_str.as_ref()) {
                excluded_count += 1;
                continue;
            }
        }

        // Symbol is dead
        dead_symbols.push(DeadSymbol {
            qualified_name: sym.qualified_name.clone(),
            kind: sym.kind,
            file_path: sym.location.file.to_string_lossy().to_string(),
            line: sym.location.line_start,
            visibility: sym.visibility,
        });
    }

    // 7. Apply kind_filter (display-layer only — does not affect excluded_count)
    if let Some(ref kinds) = config.kind_filter {
        let kind_set: HashSet<&SymbolKind> = kinds.iter().collect();
        dead_symbols.retain(|s| kind_set.contains(&s.kind));
    }

    // 8. Build summary
    let dead_count = dead_symbols.len();
    let dead_percentage = if total_symbols > 0 {
        dead_count as f64 / total_symbols as f64 * 100.0
    } else {
        0.0
    };

    let mut dead_by_kind: HashMap<SymbolKind, usize> = HashMap::new();
    let mut dead_by_file_map: HashMap<String, usize> = HashMap::new();
    for ds in &dead_symbols {
        *dead_by_kind.entry(ds.kind).or_default() += 1;
        *dead_by_file_map.entry(ds.file_path.clone()).or_default() += 1;
    }
    let mut dead_by_file: Vec<(String, usize)> = dead_by_file_map.into_iter().collect();
    dead_by_file.sort_by(|a, b| b.1.cmp(&a.1));

    DeadCodeAnalysis {
        dead_symbols,
        summary: DeadCodeSummary {
            total_symbols,
            dead_count,
            dead_percentage,
            excluded_count,
            dead_by_kind,
            dead_by_file,
        },
    }
```

- [x] Step 5: Run `cargo test -p domain -- dead_code`, verify PASS (all 13 tests green)
- [x] Step 6: Commit `feat(S05/T04): implement dead code detection algorithm`

---

### T05: CLI Command Args + Dispatch Wiring

**Files:** Modify `crates/cli/src/commands/mod.rs`, Modify `crates/cli/src/lib.rs`, Create `crates/cli/src/commands/dead_code.rs`
**Traces to:** AC4, AC5, AC6

- [x] Step 1: Write parse tests in `crates/cli/src/commands/mod.rs`

Add to `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn parse_dead_code_command() {
        let cli = Cli::parse_from(["code-graph", "dead-code"]);
        assert!(matches!(cli.command, Commands::DeadCode(_)));
    }

    #[test]
    fn parse_dead_code_with_flags() {
        let cli = Cli::parse_from([
            "code-graph",
            "dead-code",
            "--include-tests",
            "--exclude-pattern",
            "**/generated/**",
            "--kind",
            "Function",
            "--limit",
            "50",
        ]);
        if let Commands::DeadCode(args) = cli.command {
            assert!(args.include_tests);
            assert_eq!(args.exclude_pattern, vec!["**/generated/**"]);
            assert_eq!(args.kind, vec!["Function"]);
            assert_eq!(args.limit, Some(50));
        } else {
            panic!("expected DeadCode command");
        }
    }
```

Add entries to the `all_subcommands_parse` test's `commands` array:

```rust
            vec!["code-graph", "dead-code"],
            vec!["code-graph", "dead-code", "--include-tests"],
            vec!["code-graph", "dead-code", "--exclude-pattern", "**/gen/**"],
            vec!["code-graph", "dead-code", "--kind", "Function", "--limit", "10"],
```

- [x] Step 2: Run `cargo test -p cli -- parse_dead_code`, verify FAIL (variant doesn't exist)

- [x] Step 3: Add module, args struct, enum variant, dispatch, and stub handler

In `crates/cli/src/commands/mod.rs`, add to module list (alphabetical):

```rust
pub mod dead_code;
```

Add to `Commands` enum (after `Diff`):

```rust
    /// Detect unused symbols in the codebase
    #[command(name = "dead-code")]
    DeadCode(DeadCodeArgs),
```

Add args struct after `SetupArgs`:

```rust
#[derive(clap::Args)]
pub struct DeadCodeArgs {
    /// Additional exclusion patterns (repeatable)
    #[arg(long = "exclude-pattern")]
    pub exclude_pattern: Vec<String>,
    /// Include test functions as dead code candidates
    #[arg(long)]
    pub include_tests: bool,
    /// Filter to specific symbol kinds (repeatable)
    #[arg(long)]
    pub kind: Vec<String>,
    /// Maximum results to display
    #[arg(long)]
    pub limit: Option<usize>,
}
```

Create stub `crates/cli/src/commands/dead_code.rs`:

```rust
use domain::error::Result;

use crate::commands::DeadCodeArgs;
use crate::output::OutputFormat;

pub fn run_dead_code(args: &DeadCodeArgs, output_format: OutputFormat) -> Result<()> {
    todo!("dead code command not yet implemented")
}
```

Add dispatch arm in `crates/cli/src/lib.rs` (alphabetical, after Diff):

```rust
        Commands::DeadCode(args) => commands::dead_code::run_dead_code(args, output_format),
```

- [x] Step 4: Run `cargo test -p cli -- parse_dead_code`, verify PASS
- [x] Step 5: Commit `feat(S05/T05): add dead-code CLI args and dispatch wiring`

---

## Wave 2 (depends on Wave 1)

### T06: Use Case — `DeadCodeUseCase`

**Files:** Create `crates/domain/src/use_cases/dead_code.rs`, Modify `crates/domain/src/use_cases/mod.rs`
**Traces to:** AC1, AC2

- [x] Step 1: Register module in `crates/domain/src/use_cases/mod.rs`

Add (alphabetical):

```rust
pub mod dead_code;
```

- [x] Step 2: Create `crates/domain/src/use_cases/dead_code.rs` with implementation + tests

```rust
use crate::analysis::dead_code::detect_dead_code;
use crate::error::Result;
use crate::model::*;
use crate::ports::GraphStore;

pub struct DeadCodeUseCase<S> {
    store: S,
}

impl<S: GraphStore> DeadCodeUseCase<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Run dead code analysis: load all symbols + edges, detect dead code.
    pub fn analyze(&self, config: &DeadCodeConfig) -> Result<DeadCodeAnalysis> {
        let symbols = self.store.all_symbols()?;
        let edges = self.store.all_edges()?;
        Ok(detect_dead_code(&symbols, &edges, config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, EdgeKind, Location, SymbolKind, SymbolNode, Visibility};
    use crate::test_support::InMemoryGraphStore;

    fn build_store() -> InMemoryGraphStore {
        let mut store = InMemoryGraphStore::new();

        // Unused function — should be dead
        store.insert_symbol(SymbolNode {
            name: "orphan_fn".into(),
            qualified_name: "src/old.rs::orphan_fn".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/old.rs".into(),
                line_start: 10,
                line_end: 20,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Private,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });

        // Used function — should be alive
        store.insert_symbol(SymbolNode {
            name: "active_fn".into(),
            qualified_name: "src/core.rs::active_fn".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/core.rs".into(),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });

        // Edge: something calls active_fn
        store.insert_edge(Edge {
            kind: EdgeKind::Calls,
            source: "src/main.rs::main".into(),
            target: "src/core.rs::active_fn".into(),
            metadata: None,
        });

        store
    }

    #[test]
    fn use_case_detects_dead_code() {
        let store = build_store();
        let uc = DeadCodeUseCase::new(store);
        let result = uc.analyze(&DeadCodeConfig::default()).unwrap();

        assert_eq!(result.summary.total_symbols, 2);
        assert_eq!(result.dead_symbols.len(), 1);
        assert_eq!(
            result.dead_symbols[0].qualified_name,
            "src/old.rs::orphan_fn"
        );
    }

    #[test]
    fn use_case_with_include_tests() {
        let mut store = build_store();
        store.insert_symbol(SymbolNode {
            name: "old_test".into(),
            qualified_name: "tests/old.rs::old_test".into(),
            kind: SymbolKind::Test,
            location: Location {
                file: "tests/old.rs".into(),
                line_start: 1,
                line_end: 5,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Private,
            is_exported: false,
            is_async: false,
            is_test: true,
            decorators: vec![],
            signature: None,
        });

        // Default: test excluded
        let uc = DeadCodeUseCase::new(store.clone());
        let result = uc.analyze(&DeadCodeConfig::default()).unwrap();
        assert_eq!(result.dead_symbols.len(), 1); // only orphan_fn

        // With include_tests: test also flagged
        let config = DeadCodeConfig {
            include_tests: true,
            ..DeadCodeConfig::default()
        };
        let result = uc.analyze(&config).unwrap();
        assert_eq!(result.dead_symbols.len(), 2);
    }
}
```

- [x] Step 3: Run `cargo test -p domain -- use_cases::dead_code`, verify PASS
- [x] Step 4: Commit `feat(S05/T06): add dead code use case`

---

### T07: Output Formatting — `Displayable` for `DeadCodeAnalysis`

**Files:** Modify `crates/cli/src/output.rs`
**Traces to:** AC6, AC8

- [x] Step 1: Add `DeadCodeAnalysis` to the import at top of `crates/cli/src/output.rs`

Add to the existing `use domain::model::{...}` import:

```rust
    DeadCodeAnalysis,
```

- [x] Step 2: Implement `Displayable` for `DeadCodeAnalysis`

Add before the final closing (at end of main impl section):

```rust
// ---------------------------------------------------------------------------
// Displayable: DeadCodeAnalysis
// ---------------------------------------------------------------------------

impl Displayable for DeadCodeAnalysis {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(
            w,
            "Dead code: {} symbols (of {} total, {:.1}%)\n",
            self.summary.dead_count,
            self.summary.total_symbols,
            self.summary.dead_percentage,
        )?;
        for ds in &self.dead_symbols {
            let short_name = ds
                .qualified_name
                .split("::")
                .last()
                .unwrap_or(&ds.qualified_name);
            writeln!(
                w,
                "  {}:{}    {}    {:?}",
                ds.file_path, ds.line, short_name, ds.kind,
            )?;
        }
        Ok(())
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "File | Line | Symbol | Kind | Visibility")?;
        writeln!(w, "-----+------+--------+------+-----------")?;
        for ds in &self.dead_symbols {
            writeln!(
                w,
                "{} | {} | {} | {:?} | {:?}",
                ds.file_path, ds.line, ds.qualified_name, ds.kind, ds.visibility,
            )?;
        }
        writeln!(
            w,
            "\nTotal: {} dead of {} ({:.1}%), {} excluded",
            self.summary.dead_count,
            self.summary.total_symbols,
            self.summary.dead_percentage,
            self.summary.excluded_count,
        )
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}
```

- [x] Step 3: Run `cargo check -p cli`, verify PASS
- [x] Step 4: Commit `feat(S05/T07): implement dead code output formatting`

---

## Wave 3 (depends on Wave 2)

### T08: CLI Handler — `run_dead_code()`

**Files:** Implement `crates/cli/src/commands/dead_code.rs` (replace stub)
**Traces to:** AC1, AC3, AC4, AC5, AC6, AC7

- [x] Step 1: Replace stub in `crates/cli/src/commands/dead_code.rs` with full implementation

```rust
use domain::error::Result;
use domain::model::{DeadCodeConfig, SymbolKind};
use domain::use_cases::dead_code::DeadCodeUseCase;

use crate::commands::DeadCodeArgs;
use crate::commands::helpers::open_graph;
use crate::config::load_config;
use crate::output::{print, OutputFormat};

pub fn run_dead_code(args: &DeadCodeArgs, output_format: OutputFormat) -> Result<()> {
    let (store, root) = open_graph()?;
    let config = load_config(&root)?;

    // Build DeadCodeConfig: defaults -> config.toml -> CLI flags
    let mut dead_config = DeadCodeConfig::default();

    // Apply config.toml [dead-code] section
    if let Some(dc) = &config.dead_code {
        if let Some(patterns) = &dc.exclude_patterns {
            dead_config.exclude_patterns.extend(patterns.clone());
        }
        if let Some(patterns) = &dc.entry_point_patterns {
            dead_config.entry_point_patterns.extend(patterns.clone());
        }
        if let Some(patterns) = &dc.migration_patterns {
            dead_config.migration_patterns = patterns.clone();
        }
    }

    // Apply CLI flags (unioned with config, not overriding)
    dead_config.exclude_patterns.extend(args.exclude_pattern.clone());
    dead_config.include_tests = args.include_tests;

    // Parse --kind flags into SymbolKind filter
    if !args.kind.is_empty() {
        let kinds: Vec<SymbolKind> = args
            .kind
            .iter()
            .filter_map(|k| match k.to_lowercase().as_str() {
                "function" => Some(SymbolKind::Function),
                "class" => Some(SymbolKind::Class),
                "interface" => Some(SymbolKind::Interface),
                "struct" => Some(SymbolKind::Struct),
                "trait" => Some(SymbolKind::Trait),
                "enum" => Some(SymbolKind::Enum),
                "typealias" | "type_alias" => Some(SymbolKind::TypeAlias),
                "method" => Some(SymbolKind::Method),
                "property" => Some(SymbolKind::Property),
                "const" => Some(SymbolKind::Const),
                "macro" => Some(SymbolKind::Macro),
                "variable" => Some(SymbolKind::Variable),
                "component" => Some(SymbolKind::Component),
                "test" => Some(SymbolKind::Test),
                _ => None,
            })
            .collect();
        if !kinds.is_empty() {
            dead_config.kind_filter = Some(kinds);
        }
    }

    let uc = DeadCodeUseCase::new(store);
    let mut analysis = uc.analyze(&dead_config)?;

    // Apply display-layer --limit
    if let Some(limit) = args.limit {
        analysis.dead_symbols.truncate(limit);
    }

    print(&analysis, output_format);
    Ok(())
}
```

- [x] Step 2: Run `cargo build -p cli`, verify PASS
- [x] Step 3: Run `cargo test -p domain -p cli`, verify PASS (all tests green)
- [x] Step 4: Commit `feat(S05/T08): implement dead-code CLI handler with config merge`

---

## AC Traceability Matrix

| AC | Description | Tasks |
|----|-------------|-------|
| AC1 | Dead symbols with zero usage-edges, excluding entry points/exports/tests/migrations | T01, T04, T06, T08 |
| AC2 | Reuse `detect_entry_points()` from flow analysis | T04, T06 |
| AC3 | `--exclude-pattern <glob>` excludes by qualified name or file path | T04, T08 |
| AC4 | `--include-tests` causes test functions to be dead code candidates | T04, T08 |
| AC5 | `--kind <kind>` filters results to specific symbol kinds | T04, T08 |
| AC6 | Compact (default), `--table`, `--json` output formats | T05, T07, T08 |
| AC7 | `[dead-code]` section in config.toml | T03, T08 |
| AC8 | Summary statistics: total, dead count, percentage, by-kind, by-file | T01, T04, T07 |
