use crate::analysis::flow::detect_entry_points;
use crate::model::{
    DeadCodeAnalysis, DeadCodeConfig, DeadCodeSummary, DeadSymbol, Edge, EdgeKind, EntryPointKind,
    FlowConfig, SymbolKind, SymbolNode,
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
        let mut sym = make_symbol("src/lib.rs::test_main", "src/lib.rs");
        sym.is_test = true;
        sym.kind = SymbolKind::Test;
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
