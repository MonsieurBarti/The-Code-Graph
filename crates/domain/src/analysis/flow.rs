use crate::model::*;
use std::collections::{HashMap, HashSet, VecDeque};

// ---------------------------------------------------------------------------
// High-confidence edge filter
// ---------------------------------------------------------------------------

fn is_high_confidence(kind: &EdgeKind) -> bool {
    matches!(
        kind,
        EdgeKind::Calls | EdgeKind::Extends | EdgeKind::Implements | EdgeKind::Embeds
    )
}

// ---------------------------------------------------------------------------
// Entry point detection
// ---------------------------------------------------------------------------

/// Callable SymbolKinds eligible for PublicRoot classification.
const CALLABLE_KINDS: &[SymbolKind] = &[
    SymbolKind::Function,
    SymbolKind::Method,
    SymbolKind::Class,
    SymbolKind::Struct,
];

/// HTTP-handler decorator substrings (case-insensitive match).
const HTTP_DECORATORS: &[&str] = &[
    "get",
    "post",
    "put",
    "delete",
    "patch",
    "app.route",
    "router.get",
    "router.post",
    "api_view",
    "route",
];

/// CLI-command decorator substrings (case-insensitive match).
const CLI_DECORATORS: &[&str] = &["command", "subcommand", "clap", "parser"];

/// Detect entry points from symbols and edges.
pub fn detect_entry_points(
    symbols: &[SymbolNode],
    edges: &[Edge],
    config: &FlowConfig,
) -> Vec<EntryPoint> {
    let excluded: HashSet<&str> = config
        .excluded_entry_points
        .iter()
        .map(|s| s.as_str())
        .collect();
    let extra: HashSet<&str> = config
        .extra_entry_points
        .iter()
        .map(|s| s.as_str())
        .collect();

    // Build set of symbols that have incoming Calls edges (not PublicRoot candidates)
    let has_incoming_calls: HashSet<&str> = edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .map(|e| e.target.as_str())
        .collect();

    // Count outgoing edges per symbol (for PublicRoot ranking)
    let mut outgoing_count: HashMap<&str, usize> = HashMap::new();
    for edge in edges {
        if is_high_confidence(&edge.kind) {
            *outgoing_count.entry(edge.source.as_str()).or_default() += 1;
        }
    }

    let mut results: Vec<EntryPoint> = Vec::new();
    let mut public_roots: Vec<EntryPoint> = Vec::new();

    for sym in symbols {
        let qn = sym.qualified_name.as_str();

        // Skip excluded
        if excluded.contains(qn) {
            continue;
        }

        // Classify by priority: Main > Test > HttpHandler > CliCommand > PublicRoot
        if let Some(ep) = classify_symbol(sym, &has_incoming_calls, &extra) {
            if ep.kind == EntryPointKind::PublicRoot {
                let mut ep_with_score = ep;
                // Store outgoing count in confidence temporarily for sorting
                ep_with_score.confidence = 0.7;
                public_roots.push(ep_with_score);
            } else {
                results.push(ep);
            }
        }
    }

    // Cap PublicRoot at max_public_roots, sorted by outgoing edge count descending
    public_roots.sort_by(|a, b| {
        let a_out = outgoing_count
            .get(a.qualified_name.as_str())
            .copied()
            .unwrap_or(0);
        let b_out = outgoing_count
            .get(b.qualified_name.as_str())
            .copied()
            .unwrap_or(0);
        b_out.cmp(&a_out)
    });
    public_roots.truncate(config.max_public_roots);
    results.extend(public_roots);

    results
}

fn classify_symbol(
    sym: &SymbolNode,
    has_incoming_calls: &HashSet<&str>,
    extra: &HashSet<&str>,
) -> Option<EntryPoint> {
    let qn = &sym.qualified_name;
    let name_lower = sym.name.to_lowercase();
    let is_extra = extra.contains(qn.as_str());

    // Main detection
    if name_lower == "main" {
        return Some(EntryPoint {
            qualified_name: qn.clone(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        });
    }
    if sym.decorators.iter().any(|d| d.contains("tokio::main")) {
        return Some(EntryPoint {
            qualified_name: qn.clone(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        });
    }

    // Test detection
    if sym.is_test || sym.kind == SymbolKind::Test {
        return Some(EntryPoint {
            qualified_name: qn.clone(),
            kind: EntryPointKind::Test,
            confidence: 1.0,
        });
    }
    if name_lower.starts_with("test_") {
        return Some(EntryPoint {
            qualified_name: qn.clone(),
            kind: EntryPointKind::Test,
            confidence: 0.9,
        });
    }

    // HTTP handler detection
    for decorator in &sym.decorators {
        let dec_lower = decorator.to_lowercase();
        if HTTP_DECORATORS.iter().any(|h| dec_lower.contains(h)) {
            return Some(EntryPoint {
                qualified_name: qn.clone(),
                kind: EntryPointKind::HttpHandler,
                confidence: 1.0,
            });
        }
    }

    // CLI command detection
    for decorator in &sym.decorators {
        let dec_lower = decorator.to_lowercase();
        if CLI_DECORATORS.iter().any(|c| dec_lower.contains(c)) {
            return Some(EntryPoint {
                qualified_name: qn.clone(),
                kind: EntryPointKind::CliCommand,
                confidence: 1.0,
            });
        }
    }

    // Extra entry points (forced, regardless of incoming calls)
    if is_extra {
        return Some(EntryPoint {
            qualified_name: qn.clone(),
            kind: EntryPointKind::PublicRoot,
            confidence: 0.8,
        });
    }

    // PublicRoot: exported public symbol with zero incoming Calls, callable kind only
    if sym.is_exported
        && sym.visibility == Visibility::Public
        && CALLABLE_KINDS.contains(&sym.kind)
        && !has_incoming_calls.contains(qn.as_str())
    {
        return Some(EntryPoint {
            qualified_name: qn.clone(),
            kind: EntryPointKind::PublicRoot,
            confidence: 0.7,
        });
    }

    None
}

// ---------------------------------------------------------------------------
// Betweenness centrality (Brandes' algorithm)
// ---------------------------------------------------------------------------

/// Compute betweenness centrality for all nodes using Brandes' algorithm.
/// Only traverses High-confidence edges (Calls, Extends, Implements, Embeds).
/// Returns normalized scores in [0.0, 1.0] using directed graph factor (n-1)(n-2).
pub fn brandes_betweenness(nodes: &HashSet<String>, edges: &[Edge]) -> HashMap<String, f64> {
    let n = nodes.len();
    if n < 2 {
        return nodes.iter().map(|node| (node.clone(), 0.0)).collect();
    }

    // Build adjacency list from High-confidence edges only
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in nodes {
        adj.entry(node.as_str()).or_default();
    }
    for edge in edges {
        if is_high_confidence(&edge.kind)
            && nodes.contains(&edge.source)
            && nodes.contains(&edge.target)
        {
            adj.entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
        }
    }

    let mut centrality: HashMap<String, f64> = nodes.iter().map(|n| (n.clone(), 0.0)).collect();

    // Brandes' algorithm: BFS from each source
    for source in nodes {
        let mut stack: Vec<&str> = Vec::new();
        let mut predecessors: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut sigma: HashMap<&str, f64> = HashMap::new();
        let mut dist: HashMap<&str, i64> = HashMap::new();

        for node in nodes {
            sigma.insert(node.as_str(), 0.0);
            dist.insert(node.as_str(), -1);
        }
        sigma.insert(source.as_str(), 1.0);
        dist.insert(source.as_str(), 0);

        let mut queue: VecDeque<&str> = VecDeque::new();
        queue.push_back(source.as_str());

        // BFS
        while let Some(v) = queue.pop_front() {
            stack.push(v);
            let d_v = dist[v];
            if let Some(neighbors) = adj.get(v) {
                for &w in neighbors {
                    // First visit?
                    if dist[w] < 0 {
                        dist.insert(w, d_v + 1);
                        queue.push_back(w);
                    }
                    // Shortest path via v?
                    if dist[w] == d_v + 1 {
                        *sigma.get_mut(w).unwrap() += sigma[v];
                        predecessors.entry(w).or_default().push(v);
                    }
                }
            }
        }

        // Back-propagation
        let mut delta: HashMap<&str, f64> = nodes.iter().map(|n| (n.as_str(), 0.0)).collect();
        while let Some(w) = stack.pop() {
            if let Some(preds) = predecessors.get(w) {
                for &v in preds {
                    let d = (sigma[v] / sigma[w]) * (1.0 + delta[w]);
                    *delta.get_mut(v).unwrap() += d;
                }
            }
            if w != source.as_str() {
                *centrality.get_mut(w).unwrap() += delta[w];
            }
        }
    }

    // Normalize by (n-1)(n-2) — directed graph factor
    let norm = (n as f64 - 1.0) * (n as f64 - 2.0);
    if norm > 0.0 {
        for val in centrality.values_mut() {
            *val /= norm;
        }
    }

    centrality
}

// ---------------------------------------------------------------------------
// Flow enumeration (bounded DFS)
// ---------------------------------------------------------------------------

/// Enumerate execution flows from entry points via bounded DFS.
/// Only traverses High-confidence edges. Respects depth limit, global flow cap,
/// and per-entry-point visit budget. Uses per-path cycle detection.
pub fn enumerate_flows(
    entry_points: &[EntryPoint],
    edges: &[Edge],
    config: &FlowConfig,
) -> Vec<ExecutionFlow> {
    // Build adjacency list from High-confidence edges only
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in edges {
        if is_high_confidence(&edge.kind) {
            adj.entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
        }
    }

    let mut all_flows: Vec<ExecutionFlow> = Vec::new();

    for ep in entry_points {
        if all_flows.len() >= config.max_flows {
            break;
        }

        let mut ep_flows: Vec<ExecutionFlow> = Vec::new();
        let mut visit_count: usize = 0;
        let mut truncated = false;

        // DFS with per-path visited set (iterative with explicit stack)
        // Stack holds: (current_node, path_so_far, visited_set)
        let mut stack: Vec<(String, Vec<String>, HashSet<String>)> = Vec::new();
        let initial_visited: HashSet<String> = [ep.qualified_name.clone()].into_iter().collect();
        stack.push((
            ep.qualified_name.clone(),
            vec![ep.qualified_name.clone()],
            initial_visited,
        ));

        while let Some((node, path, visited)) = stack.pop() {
            visit_count += 1;
            if visit_count > config.visit_budget {
                truncated = true;
                break;
            }
            if all_flows.len() + ep_flows.len() >= config.max_flows {
                break;
            }

            let neighbors: Vec<&str> = adj
                .get(node.as_str())
                .map(|v| v.as_slice())
                .unwrap_or(&[])
                .iter()
                .filter(|&&n| !visited.contains(n))
                .copied()
                .collect();

            if neighbors.is_empty() || path.len() >= config.max_depth {
                // Terminal: no unvisited neighbors or depth limit reached
                ep_flows.push(ExecutionFlow {
                    entry: ep.qualified_name.clone(),
                    path: path.clone(),
                    depth: path.len(),
                    truncated: false,
                });
            } else {
                for &neighbor in &neighbors {
                    let mut new_path = path.clone();
                    new_path.push(neighbor.to_string());
                    let mut new_visited = visited.clone();
                    new_visited.insert(neighbor.to_string());
                    stack.push((neighbor.to_string(), new_path, new_visited));
                }
            }
        }

        // Mark truncation on all flows from this entry point if budget was exhausted
        if truncated {
            for flow in &mut ep_flows {
                flow.truncated = true;
            }
        }

        let remaining = config.max_flows - all_flows.len();
        ep_flows.truncate(remaining);
        all_flows.extend(ep_flows);
    }

    all_flows
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_symbol(name: &str, qn: &str, kind: SymbolKind) -> SymbolNode {
        SymbolNode {
            name: name.into(),
            qualified_name: qn.into(),
            kind,
            location: Location {
                file: "src/lib.rs".into(),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }
    }

    fn make_edge(kind: EdgeKind, source: &str, target: &str) -> Edge {
        Edge {
            kind,
            source: source.into(),
            target: target.into(),
            metadata: None,
        }
    }

    // -----------------------------------------------------------------------
    // T02: Entry point detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn detect_main_entry_point() {
        let sym = make_symbol("main", "src/main.rs::main", SymbolKind::Function);
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EntryPointKind::Main);
        assert_eq!(entries[0].confidence, 1.0);
    }

    #[test]
    fn detect_tokio_main() {
        let mut sym = make_symbol("main", "src/main.rs::main", SymbolKind::Function);
        sym.decorators = vec!["tokio::main".into()];
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert!(entries.iter().any(|e| e.kind == EntryPointKind::Main));
    }

    #[test]
    fn detect_test_entry_point() {
        let mut sym = make_symbol("test_foo", "src/lib.rs::test_foo", SymbolKind::Function);
        sym.is_test = true;
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EntryPointKind::Test);
    }

    #[test]
    fn detect_http_handler() {
        let mut sym = make_symbol("handle", "src/api.rs::handle", SymbolKind::Function);
        sym.decorators = vec!["Get".into()];
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EntryPointKind::HttpHandler);
    }

    #[test]
    fn detect_cli_command() {
        let mut sym = make_symbol("run_cmd", "src/cli.rs::run_cmd", SymbolKind::Function);
        sym.decorators = vec!["command".into()];
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EntryPointKind::CliCommand);
    }

    #[test]
    fn detect_public_root() {
        let sym = make_symbol("init", "src/lib.rs::init", SymbolKind::Function);
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert!(entries.iter().any(|e| e.kind == EntryPointKind::PublicRoot));
    }

    #[test]
    fn public_root_excluded_when_has_incoming_calls() {
        let sym = make_symbol("helper", "src/lib.rs::helper", SymbolKind::Function);
        let edge = make_edge(EdgeKind::Calls, "src/main.rs::main", "src/lib.rs::helper");
        let entries = detect_entry_points(&[sym], &[edge], &FlowConfig::default());
        assert!(!entries.iter().any(|e| e.kind == EntryPointKind::PublicRoot));
    }

    #[test]
    fn public_root_capped_at_max() {
        let config = FlowConfig {
            max_public_roots: 2,
            ..FlowConfig::default()
        };
        let syms: Vec<_> = (0..10)
            .map(|i| {
                make_symbol(
                    &format!("fn{i}"),
                    &format!("src/lib.rs::fn{i}"),
                    SymbolKind::Function,
                )
            })
            .collect();
        let entries = detect_entry_points(&syms, &[], &config);
        let public_roots: Vec<_> = entries
            .iter()
            .filter(|e| e.kind == EntryPointKind::PublicRoot)
            .collect();
        assert!(public_roots.len() <= 2);
    }

    #[test]
    fn excluded_entry_points_filtered() {
        let sym = make_symbol("main", "src/main.rs::main", SymbolKind::Function);
        let config = FlowConfig {
            excluded_entry_points: vec!["src/main.rs::main".into()],
            ..FlowConfig::default()
        };
        let entries = detect_entry_points(&[sym], &[], &config);
        assert!(entries.is_empty());
    }

    #[test]
    fn extra_entry_points_added() {
        let sym = make_symbol("custom", "src/lib.rs::custom", SymbolKind::Function);
        let edge = make_edge(EdgeKind::Calls, "src/main.rs::main", "src/lib.rs::custom");
        let config = FlowConfig {
            extra_entry_points: vec!["src/lib.rs::custom".into()],
            ..FlowConfig::default()
        };
        let entries = detect_entry_points(&[sym], &[edge], &config);
        assert!(!entries.is_empty());
    }

    #[test]
    fn detect_python_dunder_main() {
        let sym = make_symbol("main", "app.py::main", SymbolKind::Function);
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert!(entries.iter().any(|e| e.kind == EntryPointKind::Main));
    }

    #[test]
    fn detect_test_prefix_python() {
        let mut sym = make_symbol(
            "test_login",
            "test_auth.py::test_login",
            SymbolKind::Function,
        );
        sym.is_test = false;
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert!(entries.iter().any(|e| e.kind == EntryPointKind::Test));
    }

    #[test]
    fn public_root_excludes_non_callable_kinds() {
        for kind in [
            SymbolKind::Const,
            SymbolKind::Enum,
            SymbolKind::TypeAlias,
            SymbolKind::Variable,
            SymbolKind::Property,
            SymbolKind::Interface,
            SymbolKind::Trait,
        ] {
            let sym = make_symbol("MyType", "src/lib.rs::MyType", kind);
            let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
            assert!(
                !entries.iter().any(|e| e.kind == EntryPointKind::PublicRoot),
                "{kind:?} should not be classified as PublicRoot"
            );
        }
    }

    // -----------------------------------------------------------------------
    // T03: Brandes' betweenness centrality tests
    // -----------------------------------------------------------------------

    #[test]
    fn brandes_linear_graph_center_has_highest_centrality() {
        let edges = vec![
            make_edge(EdgeKind::Calls, "a", "b"),
            make_edge(EdgeKind::Calls, "b", "c"),
            make_edge(EdgeKind::Calls, "c", "d"),
            make_edge(EdgeKind::Calls, "d", "e"),
        ];
        let nodes: HashSet<String> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let scores = brandes_betweenness(&nodes, &edges);
        let c_score = scores.get("c").copied().unwrap_or(0.0);
        let a_score = scores.get("a").copied().unwrap_or(0.0);
        let e_score = scores.get("e").copied().unwrap_or(0.0);
        assert!(
            c_score > a_score,
            "center should have higher betweenness than endpoints"
        );
        assert!(c_score > e_score);
        for (_, &v) in &scores {
            assert!(
                v >= 0.0 && v <= 1.0,
                "betweenness must be normalized to [0,1]"
            );
        }
    }

    #[test]
    fn brandes_disconnected_nodes_have_zero_betweenness() {
        let nodes: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let scores = brandes_betweenness(&nodes, &[]);
        assert_eq!(*scores.get("a").unwrap_or(&0.0), 0.0);
        assert_eq!(*scores.get("b").unwrap_or(&0.0), 0.0);
    }

    #[test]
    fn brandes_diamond_graph_intermediaries_have_betweenness() {
        let edges = vec![
            make_edge(EdgeKind::Calls, "a", "b"),
            make_edge(EdgeKind::Calls, "a", "c"),
            make_edge(EdgeKind::Calls, "b", "d"),
            make_edge(EdgeKind::Calls, "c", "d"),
        ];
        let nodes: HashSet<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let scores = brandes_betweenness(&nodes, &edges);
        let b = scores.get("b").copied().unwrap_or(0.0);
        let c = scores.get("c").copied().unwrap_or(0.0);
        assert!(b > 0.0, "intermediary b must have nonzero betweenness");
        assert!(c > 0.0, "intermediary c must have nonzero betweenness");
        assert!(
            (b - c).abs() < 1e-9,
            "symmetric intermediaries should have equal betweenness"
        );
    }

    #[test]
    fn brandes_single_node_no_division_by_zero() {
        let nodes: HashSet<String> = ["a"].iter().map(|s| s.to_string()).collect();
        let scores = brandes_betweenness(&nodes, &[]);
        assert_eq!(*scores.get("a").unwrap_or(&0.0), 0.0);
    }

    #[test]
    fn brandes_empty_graph() {
        let nodes: HashSet<String> = HashSet::new();
        let scores = brandes_betweenness(&nodes, &[]);
        assert!(scores.is_empty());
    }

    #[test]
    fn brandes_only_uses_high_confidence_edges() {
        let edges = vec![
            make_edge(EdgeKind::Calls, "a", "b"),
            make_edge(EdgeKind::ImportsFrom, "a", "c"),
        ];
        let nodes: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let scores = brandes_betweenness(&nodes, &edges);
        assert_eq!(*scores.get("c").unwrap_or(&0.0), 0.0);
    }

    #[test]
    fn brandes_normalization_directed() {
        let edges = vec![
            make_edge(EdgeKind::Calls, "a", "b"),
            make_edge(EdgeKind::Calls, "b", "c"),
            make_edge(EdgeKind::Calls, "c", "d"),
            make_edge(EdgeKind::Calls, "d", "e"),
        ];
        let nodes: HashSet<String> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let scores = brandes_betweenness(&nodes, &edges);
        for &v in scores.values() {
            assert!(v <= 1.0, "normalized score must not exceed 1.0");
        }
        // In directed A→B→C→D→E, C's raw betweenness = 4 (on paths A→D, A→E, B→D, B→E).
        // Normalized: 4 / ((5-1)(5-2)) = 4/12 = 1/3
        let c_score = scores.get("c").copied().unwrap_or(0.0);
        assert!(
            (c_score - 1.0 / 3.0).abs() < 1e-9,
            "center of 5-node directed linear graph should have betweenness 1/3, got {c_score}"
        );
    }

    // -----------------------------------------------------------------------
    // T04: Flow enumeration tests
    // -----------------------------------------------------------------------

    #[test]
    fn enumerate_flows_linear_graph() {
        let edges = vec![
            make_edge(EdgeKind::Calls, "main", "a"),
            make_edge(EdgeKind::Calls, "a", "b"),
        ];
        let entry_points = vec![EntryPoint {
            qualified_name: "main".into(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        }];
        let flows = enumerate_flows(&entry_points, &edges, &FlowConfig::default());
        assert_eq!(flows.len(), 1);
        assert_eq!(flows[0].path, vec!["main", "a", "b"]);
        assert_eq!(flows[0].depth, 3);
        assert!(!flows[0].truncated);
    }

    #[test]
    fn enumerate_flows_cycle_detection() {
        let edges = vec![
            make_edge(EdgeKind::Calls, "main", "a"),
            make_edge(EdgeKind::Calls, "a", "b"),
            make_edge(EdgeKind::Calls, "b", "a"),
        ];
        let entry_points = vec![EntryPoint {
            qualified_name: "main".into(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        }];
        let flows = enumerate_flows(&entry_points, &edges, &FlowConfig::default());
        for flow in &flows {
            let unique: HashSet<&String> = flow.path.iter().collect();
            assert_eq!(unique.len(), flow.path.len(), "no duplicates in flow path");
        }
    }

    #[test]
    fn enumerate_flows_depth_limit() {
        let edges: Vec<Edge> = (0..25)
            .map(|i| make_edge(EdgeKind::Calls, &format!("e{i}"), &format!("e{}", i + 1)))
            .collect();
        let entry_points = vec![EntryPoint {
            qualified_name: "e0".into(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        }];
        let config = FlowConfig {
            max_depth: 5,
            ..FlowConfig::default()
        };
        let flows = enumerate_flows(&entry_points, &edges, &config);
        for flow in &flows {
            assert!(flow.path.len() <= 5, "flow depth must not exceed max_depth");
        }
    }

    #[test]
    fn enumerate_flows_global_cap() {
        let edges: Vec<Edge> = (0..100)
            .map(|i| make_edge(EdgeKind::Calls, "entry", &format!("a{i}")))
            .collect();
        let entry_points = vec![EntryPoint {
            qualified_name: "entry".into(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        }];
        let config = FlowConfig {
            max_flows: 10,
            ..FlowConfig::default()
        };
        let flows = enumerate_flows(&entry_points, &edges, &config);
        assert!(flows.len() <= 10, "global flow cap must be respected");
    }

    #[test]
    fn enumerate_flows_visit_budget() {
        let edges: Vec<Edge> = (0..1000)
            .map(|i| make_edge(EdgeKind::Calls, "entry", &format!("a{i}")))
            .collect();
        let entry_points = vec![EntryPoint {
            qualified_name: "entry".into(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        }];
        let config = FlowConfig {
            visit_budget: 50,
            ..FlowConfig::default()
        };
        let flows = enumerate_flows(&entry_points, &edges, &config);
        assert!(flows.iter().any(|f| f.truncated) || flows.len() < 1000);
    }

    #[test]
    fn enumerate_flows_only_high_confidence_edges() {
        let edges = vec![
            make_edge(EdgeKind::Calls, "main", "a"),
            make_edge(EdgeKind::ImportsFrom, "a", "b"),
        ];
        let entry_points = vec![EntryPoint {
            qualified_name: "main".into(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        }];
        let flows = enumerate_flows(&entry_points, &edges, &FlowConfig::default());
        for flow in &flows {
            assert!(!flow.path.contains(&"b".to_string()));
        }
    }

    #[test]
    fn enumerate_flows_multiple_entry_points_share_global_cap() {
        let mut edges = Vec::new();
        for i in 0..10 {
            edges.push(make_edge(EdgeKind::Calls, "e1", &format!("a{i}")));
            edges.push(make_edge(EdgeKind::Calls, "e2", &format!("b{i}")));
        }
        let entry_points = vec![
            EntryPoint {
                qualified_name: "e1".into(),
                kind: EntryPointKind::Main,
                confidence: 1.0,
            },
            EntryPoint {
                qualified_name: "e2".into(),
                kind: EntryPointKind::Test,
                confidence: 1.0,
            },
        ];
        let config = FlowConfig {
            max_flows: 15,
            ..FlowConfig::default()
        };
        let flows = enumerate_flows(&entry_points, &edges, &config);
        assert!(
            flows.len() <= 15,
            "global cap must be shared across entry points"
        );
        let e1_flows = flows.iter().filter(|f| f.entry == "e1").count();
        let e2_flows = flows.iter().filter(|f| f.entry == "e2").count();
        assert!(
            e1_flows > 0 && e2_flows > 0,
            "both entry points should contribute flows"
        );
    }

    #[test]
    fn enumerate_flows_branching() {
        let edges = vec![
            make_edge(EdgeKind::Calls, "main", "a"),
            make_edge(EdgeKind::Calls, "main", "b"),
            make_edge(EdgeKind::Calls, "a", "c"),
            make_edge(EdgeKind::Calls, "b", "c"),
        ];
        let entry_points = vec![EntryPoint {
            qualified_name: "main".into(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        }];
        let flows = enumerate_flows(&entry_points, &edges, &FlowConfig::default());
        assert_eq!(flows.len(), 2);
    }
}
