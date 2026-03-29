# M02-S01: Execution Flows — Implementation Plan

> For agentic workers: execute task-by-task with TDD.

**Goal:** Add execution flow detection and criticality scoring to the code graph. Users can trace flows from entry points through the codebase and rank symbols by betweenness centrality.

**Architecture:** Domain analysis layer (algorithms in `analysis/flow.rs`), use case orchestration (`use_cases/flow.rs`), CLI command (`commands/flows.rs`). No new dependencies — stdlib data structures only.

**Tech Stack:** Rust, clap (CLI), serde (serialization), SQLite (storage via existing GraphStore)

## File Structure

### New Files
| File | Responsibility |
|------|----------------|
| `crates/domain/src/analysis/flow.rs` | Core algorithms: entry point detection, Brandes' betweenness centrality, bounded DFS flow enumeration |
| `crates/domain/src/use_cases/flow.rs` | FlowUseCase orchestrating analysis algorithms via GraphStore |
| `crates/cli/src/commands/flows.rs` | CLI handler for `code-graph flows` command |

### Modified Files
| File | Change |
|------|--------|
| `crates/domain/src/model.rs` | Add 7 new types (EntryPoint, EntryPointKind, ExecutionFlow, CriticalityScore, FlowAnalysis, FlowStats, FlowConfig) + extend GraphStats with optional fields |
| `crates/domain/src/analysis/mod.rs` | Add `pub mod flow;` |
| `crates/domain/src/use_cases/mod.rs` | Add `pub mod flow;` |
| `crates/domain/src/lib.rs` | Re-export new flow types |
| `crates/domain/src/test_support.rs` | Update InMemoryGraphStore::stats() for new GraphStats fields |
| `crates/storage/src/graph_store.rs` | Update SqliteStore::stats() for new GraphStats fields |
| `crates/cli/src/commands/mod.rs` | Add `Flows(FlowsArgs)` variant + `FlowsArgs` struct |
| `crates/cli/src/commands/stats.rs` | Instantiate FlowUseCase for on-demand entry point count + avg criticality |
| `crates/cli/src/output.rs` | Displayable impls for FlowAnalysis, Vec<CriticalityScore>, GraphStats extension |
| `crates/cli/src/config.rs` | Add `FlowsConfig` section to `CodeGraphConfig` |
| `crates/cli/src/lib.rs` | Wire `Commands::Flows` match arm |

---

### Task 1: Domain Model Types
**Files:** Modify `crates/domain/src/model.rs`, `crates/domain/src/test_support.rs`, `crates/storage/src/graph_store.rs`
**Traces to:** AC6, AC7, AC8, AC10

- [ ] Step 1: Write failing test — add a test in `model.rs` that constructs all new types and checks GraphStats optional fields

```rust
// In crates/domain/src/model.rs, inside mod tests {}
#[test]
fn flow_types_serde_roundtrip() {
    use super::*;

    let entry = EntryPoint {
        qualified_name: "src/main.rs::main".into(),
        kind: EntryPointKind::Main,
        confidence: 1.0,
    };
    let json = serde_json::to_string(&entry).unwrap();
    let _: EntryPoint = serde_json::from_str(&json).unwrap();

    let flow = ExecutionFlow {
        entry: "src/main.rs::main".into(),
        path: vec!["src/main.rs::main".into(), "src/db.rs::connect".into()],
        depth: 2,
        truncated: false,
    };
    let json = serde_json::to_string(&flow).unwrap();
    let _: ExecutionFlow = serde_json::from_str(&json).unwrap();

    let score = CriticalityScore {
        qualified_name: "src/db.rs::connect".into(),
        betweenness: 0.75,
        flow_count: 42,
        is_entry_point: false,
    };
    let json = serde_json::to_string(&score).unwrap();
    let _: CriticalityScore = serde_json::from_str(&json).unwrap();

    let config = FlowConfig::default();
    assert_eq!(config.max_depth, 20);
    assert_eq!(config.max_flows, 1000);
    assert_eq!(config.visit_budget, 100_000);
    assert_eq!(config.max_public_roots, 50);

    let analysis = FlowAnalysis {
        entry_points: vec![entry],
        flows: vec![flow],
        criticality: vec![score],
        stats: FlowStats {
            total_entry_points: 1,
            total_flows: 1,
            max_depth: 2,
            avg_depth: 2.0,
        },
    };
    let json = serde_json::to_string(&analysis).unwrap();
    let _: FlowAnalysis = serde_json::from_str(&json).unwrap();
}

#[test]
fn graph_stats_optional_fields_default_none() {
    let stats = GraphStats {
        files: 10,
        symbols: 50,
        edges: 100,
        entry_point_count: None,
        avg_criticality: None,
    };
    let json = serde_json::to_string(&stats).unwrap();
    // Optional fields omitted from JSON when None
    assert!(!json.contains("entry_point_count"));
    assert!(!json.contains("avg_criticality"));
}

#[test]
fn graph_stats_optional_fields_present() {
    let stats = GraphStats {
        files: 10,
        symbols: 50,
        edges: 100,
        entry_point_count: Some(5),
        avg_criticality: Some(0.034),
    };
    let json = serde_json::to_string(&stats).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["entry_point_count"], 5);
}
```

- [ ] Step 2: Run `cargo test -p domain --lib model::tests`, verify FAIL (types don't exist)
- [ ] Step 3: Implement — add types to `crates/domain/src/model.rs`:

```rust
// After the DiffImpactReport struct, before QualifiedName

// ---------------------------------------------------------------------------
// Flow analysis types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPoint {
    pub qualified_name: String,
    pub kind: EntryPointKind,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryPointKind {
    Main,
    Test,
    HttpHandler,
    CliCommand,
    PublicRoot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionFlow {
    pub entry: String,
    pub path: Vec<String>,
    pub depth: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalityScore {
    pub qualified_name: String,
    pub betweenness: f64,
    pub flow_count: usize,
    pub is_entry_point: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowAnalysis {
    pub entry_points: Vec<EntryPoint>,
    pub flows: Vec<ExecutionFlow>,
    pub criticality: Vec<CriticalityScore>,
    pub stats: FlowStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStats {
    pub total_entry_points: usize,
    pub total_flows: usize,
    pub max_depth: usize,
    pub avg_depth: f64,
}

#[derive(Debug, Clone)]
pub struct FlowConfig {
    pub max_depth: usize,
    pub max_flows: usize,
    pub visit_budget: usize,
    pub max_public_roots: usize,
    pub extra_entry_points: Vec<String>,
    pub excluded_entry_points: Vec<String>,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            max_depth: 20,
            max_flows: 1000,
            visit_budget: 100_000,
            max_public_roots: 50,
            extra_entry_points: Vec::new(),
            excluded_entry_points: Vec::new(),
        }
    }
}
```

Extend GraphStats:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub files: usize,
    pub symbols: usize,
    pub edges: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_point_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_criticality: Option<f64>,
}
```

Update all existing GraphStats constructors (exhaustive list):
- `crates/domain/src/test_support.rs:113-118` — `InMemoryGraphStore::stats()`: add `entry_point_count: None, avg_criticality: None`
- `crates/storage/src/graph_store.rs:471-475` — `SqliteStore::stats()`: add `entry_point_count: None, avg_criticality: None`
- `crates/domain/src/model.rs:597-601` — `serde_roundtrip_all_supporting_types` test: add `entry_point_count: None, avg_criticality: None`
- `crates/cli/src/output.rs:625-630` — `sample_graph_stats()` helper: add `entry_point_count: None, avg_criticality: None`

- [ ] Step 4: Run `cargo test -p domain --lib model::tests && cargo test -p storage && cargo test -p cli`, verify PASS
- [ ] Step 5: `git add crates/domain/src/model.rs crates/domain/src/test_support.rs crates/storage/src/graph_store.rs crates/cli/src/output.rs && git commit -m "feat(S01/T01): add flow analysis domain types and extend GraphStats"`

---

### Task 2: Entry Point Detection
**Files:** Create `crates/domain/src/analysis/flow.rs`, Modify `crates/domain/src/analysis/mod.rs`
**Traces to:** AC4, AC7

- [ ] Step 1: Write failing test — create `crates/domain/src/analysis/flow.rs` with tests for all 5 EntryPointKind variants

```rust
// crates/domain/src/analysis/flow.rs
use crate::model::*;
use std::collections::{HashMap, HashSet};

/// Detect entry points from symbols and edges.
pub fn detect_entry_points(
    symbols: &[SymbolNode],
    edges: &[Edge],
    config: &FlowConfig,
) -> Vec<EntryPoint> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // No incoming Calls edges -> PublicRoot
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert!(entries.iter().any(|e| e.kind == EntryPointKind::PublicRoot));
    }

    #[test]
    fn public_root_excluded_when_has_incoming_calls() {
        let sym = make_symbol("helper", "src/lib.rs::helper", SymbolKind::Function);
        let edge = Edge {
            kind: EdgeKind::Calls,
            source: "src/main.rs::main".into(),
            target: "src/lib.rs::helper".into(),
            metadata: None,
        };
        let entries = detect_entry_points(&[sym], &[edge], &FlowConfig::default());
        assert!(!entries.iter().any(|e| e.kind == EntryPointKind::PublicRoot));
    }

    #[test]
    fn public_root_capped_at_max() {
        let config = FlowConfig { max_public_roots: 2, ..FlowConfig::default() };
        let syms: Vec<_> = (0..10).map(|i| {
            make_symbol(&format!("fn{i}"), &format!("src/lib.rs::fn{i}"), SymbolKind::Function)
        }).collect();
        let entries = detect_entry_points(&syms, &[], &config);
        let public_roots: Vec<_> = entries.iter().filter(|e| e.kind == EntryPointKind::PublicRoot).collect();
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
        let edge = Edge {
            kind: EdgeKind::Calls,
            source: "src/main.rs::main".into(),
            target: "src/lib.rs::custom".into(),
            metadata: None,
        };
        let config = FlowConfig {
            extra_entry_points: vec!["src/lib.rs::custom".into()],
            ..FlowConfig::default()
        };
        // Has incoming calls but is forced as extra entry point
        let entries = detect_entry_points(&[sym], &[edge], &config);
        assert!(!entries.is_empty());
    }

    #[test]
    fn detect_python_dunder_main() {
        // Python: if __name__ == "__main__" detected as Main entry point
        // Parser sets name="__main__" or similar for top-level call detected
        let sym = make_symbol("main", "app.py::main", SymbolKind::Function);
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert!(entries.iter().any(|e| e.kind == EntryPointKind::Main));
    }

    #[test]
    fn detect_test_prefix_python() {
        let mut sym = make_symbol("test_login", "test_auth.py::test_login", SymbolKind::Function);
        sym.is_test = false; // parser didn't set is_test, but name starts with test_
        let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
        assert!(entries.iter().any(|e| e.kind == EntryPointKind::Test));
    }

    #[test]
    fn public_root_excludes_non_callable_kinds() {
        // Const, Enum, TypeAlias should NOT be classified as PublicRoot
        // even with no incoming calls
        for kind in [SymbolKind::Const, SymbolKind::Enum, SymbolKind::TypeAlias,
                     SymbolKind::Variable, SymbolKind::Property, SymbolKind::Interface, SymbolKind::Trait] {
            let sym = make_symbol("MyType", "src/lib.rs::MyType", kind);
            let entries = detect_entry_points(&[sym], &[], &FlowConfig::default());
            assert!(
                !entries.iter().any(|e| e.kind == EntryPointKind::PublicRoot),
                "{kind:?} should not be classified as PublicRoot"
            );
        }
    }
}
```

Add module registration in `crates/domain/src/analysis/mod.rs`:
```rust
pub mod flow;
```

- [ ] Step 2: Run `cargo test -p domain --lib analysis::flow::tests`, verify FAIL (todo!())
- [ ] Step 3: Implement `detect_entry_points` in `crates/domain/src/analysis/flow.rs`:
  - Classify symbols by SymbolKind + naming conventions + decorators
  - Build incoming-calls set for PublicRoot exclusion
  - Apply config overrides (extra/excluded)
  - Cap PublicRoot at max_public_roots (sorted by outgoing edge count)
- [ ] Step 4: Run `cargo test -p domain --lib analysis::flow::tests`, verify PASS
- [ ] Step 5: `git add crates/domain/src/analysis/flow.rs crates/domain/src/analysis/mod.rs && git commit -m "feat(S01/T02): implement entry point detection for all 5 variants"`

---

### Task 3: Betweenness Centrality (Brandes' Algorithm)
**Files:** Modify `crates/domain/src/analysis/flow.rs`
**Traces to:** AC3, AC6, AC7

- [ ] Step 1: Write failing test in `analysis/flow.rs`

```rust
#[test]
fn brandes_linear_graph_center_has_highest_centrality() {
    // A -> B -> C -> D -> E
    // B, C, D are on all shortest paths; C should have highest betweenness
    let edges = vec![
        Edge { kind: EdgeKind::Calls, source: "a".into(), target: "b".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "b".into(), target: "c".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "c".into(), target: "d".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "d".into(), target: "e".into(), metadata: None },
    ];
    let nodes: HashSet<String> = ["a","b","c","d","e"].iter().map(|s| s.to_string()).collect();
    let scores = brandes_betweenness(&nodes, &edges);
    // C is center of the linear graph — highest betweenness
    let c_score = scores.get("c").copied().unwrap_or(0.0);
    let a_score = scores.get("a").copied().unwrap_or(0.0);
    let e_score = scores.get("e").copied().unwrap_or(0.0);
    assert!(c_score > a_score, "center should have higher betweenness than endpoints");
    assert!(c_score > e_score);
    // All values in [0, 1]
    for (_, &v) in &scores {
        assert!(v >= 0.0 && v <= 1.0, "betweenness must be normalized to [0,1]");
    }
}

#[test]
fn brandes_disconnected_nodes_have_zero_betweenness() {
    let nodes: HashSet<String> = ["a","b"].iter().map(|s| s.to_string()).collect();
    let scores = brandes_betweenness(&nodes, &[]);
    assert_eq!(*scores.get("a").unwrap_or(&0.0), 0.0);
    assert_eq!(*scores.get("b").unwrap_or(&0.0), 0.0);
}

#[test]
fn brandes_diamond_graph_intermediaries_have_betweenness() {
    // a -> b, a -> c, b -> d, c -> d
    // b and c are intermediaries on a->d paths
    let edges = vec![
        Edge { kind: EdgeKind::Calls, source: "a".into(), target: "b".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "a".into(), target: "c".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "b".into(), target: "d".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "c".into(), target: "d".into(), metadata: None },
    ];
    let nodes: HashSet<String> = ["a","b","c","d"].iter().map(|s| s.to_string()).collect();
    let scores = brandes_betweenness(&nodes, &edges);
    let b = scores.get("b").copied().unwrap_or(0.0);
    let c = scores.get("c").copied().unwrap_or(0.0);
    // b and c each lie on half the shortest paths a->d
    assert!(b > 0.0, "intermediary b must have nonzero betweenness");
    assert!(c > 0.0, "intermediary c must have nonzero betweenness");
    // b and c should have equal betweenness (symmetric)
    assert!((b - c).abs() < 1e-9, "symmetric intermediaries should have equal betweenness");
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
        Edge { kind: EdgeKind::Calls, source: "a".into(), target: "b".into(), metadata: None },
        Edge { kind: EdgeKind::ImportsFrom, source: "a".into(), target: "c".into(), metadata: None },
    ];
    let nodes: HashSet<String> = ["a","b","c"].iter().map(|s| s.to_string()).collect();
    let scores = brandes_betweenness(&nodes, &edges);
    // c should have 0 betweenness — ImportsFrom is Medium, not traversed
    assert_eq!(*scores.get("c").unwrap_or(&0.0), 0.0);
}

#[test]
fn brandes_normalization_directed() {
    // Linear: A -> B -> C -> D -> E
    // n=5, normalization factor = (5-1)*(5-2) = 12
    // C has raw betweenness = 6 (on shortest paths: A→D, A→E, B→D, B→E, A→C...→, B→C...→)
    // Normalized: 6/12 = 0.5
    let edges = vec![
        Edge { kind: EdgeKind::Calls, source: "a".into(), target: "b".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "b".into(), target: "c".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "c".into(), target: "d".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "d".into(), target: "e".into(), metadata: None },
    ];
    let nodes: HashSet<String> = ["a","b","c","d","e"].iter().map(|s| s.to_string()).collect();
    let scores = brandes_betweenness(&nodes, &edges);
    for &v in scores.values() {
        assert!(v <= 1.0, "normalized score must not exceed 1.0");
    }
    // Concrete expected value for center node
    let c_score = scores.get("c").copied().unwrap_or(0.0);
    assert!((c_score - 0.5).abs() < 1e-9, "center of 5-node linear graph should have betweenness 0.5, got {c_score}");
}
```

- [ ] Step 2: Run `cargo test -p domain --lib analysis::flow::tests::brandes`, verify FAIL
- [ ] Step 3: Implement `brandes_betweenness` function:
  - Guard: if `n < 3`, return all-zero scores (normalization factor `(n-1)*(n-2)` is 0)
  - Build adjacency list from High-confidence edges only
  - For each source node: BFS to compute shortest paths (sigma counts + predecessor lists)
  - Back-propagation to accumulate dependency values
  - Normalize by `(n-1)*(n-2)` (directed graph factor)
  - Return `HashMap<String, f64>` of normalized betweenness scores
- [ ] Step 4: Run `cargo test -p domain --lib analysis::flow::tests::brandes`, verify PASS
- [ ] Step 5: `git add crates/domain/src/analysis/flow.rs && git commit -m "feat(S01/T03): implement Brandes betweenness centrality algorithm"`

---

### Task 4: Flow Enumeration (Bounded DFS)
**Files:** Modify `crates/domain/src/analysis/flow.rs`
**Traces to:** AC1, AC2, AC8, AC9

- [ ] Step 1: Write failing tests

```rust
#[test]
fn enumerate_flows_linear_graph() {
    // main -> A -> B (terminal)
    let edges = vec![
        Edge { kind: EdgeKind::Calls, source: "main".into(), target: "a".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "a".into(), target: "b".into(), metadata: None },
    ];
    let entry_points = vec![EntryPoint {
        qualified_name: "main".into(),
        kind: EntryPointKind::Main,
        confidence: 1.0,
    }];
    let config = FlowConfig::default();
    let flows = enumerate_flows(&entry_points, &edges, &config);
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].path, vec!["main", "a", "b"]);
    assert_eq!(flows[0].depth, 3);
    assert!(!flows[0].truncated);
}

#[test]
fn enumerate_flows_cycle_detection() {
    // main -> A -> B -> A (cycle)
    let edges = vec![
        Edge { kind: EdgeKind::Calls, source: "main".into(), target: "a".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "a".into(), target: "b".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "b".into(), target: "a".into(), metadata: None },
    ];
    let entry_points = vec![EntryPoint {
        qualified_name: "main".into(),
        kind: EntryPointKind::Main,
        confidence: 1.0,
    }];
    let flows = enumerate_flows(&entry_points, &edges, &FlowConfig::default());
    // Should find flows but no node appears twice in any single path
    for flow in &flows {
        let unique: HashSet<&String> = flow.path.iter().collect();
        assert_eq!(unique.len(), flow.path.len(), "no duplicates in flow path");
    }
}

#[test]
fn enumerate_flows_depth_limit() {
    // Chain: e0 -> e1 -> e2 -> ... -> e25
    let edges: Vec<Edge> = (0..25).map(|i| Edge {
        kind: EdgeKind::Calls,
        source: format!("e{i}"),
        target: format!("e{}", i + 1),
        metadata: None,
    }).collect();
    let entry_points = vec![EntryPoint {
        qualified_name: "e0".into(),
        kind: EntryPointKind::Main,
        confidence: 1.0,
    }];
    let config = FlowConfig { max_depth: 5, ..FlowConfig::default() };
    let flows = enumerate_flows(&entry_points, &edges, &config);
    for flow in &flows {
        assert!(flow.path.len() <= 5, "flow depth must not exceed max_depth");
    }
}

#[test]
fn enumerate_flows_global_cap() {
    // Fan-out: entry -> a1, a2, ..., a100
    let edges: Vec<Edge> = (0..100).map(|i| Edge {
        kind: EdgeKind::Calls,
        source: "entry".into(),
        target: format!("a{i}"),
        metadata: None,
    }).collect();
    let entry_points = vec![EntryPoint {
        qualified_name: "entry".into(),
        kind: EntryPointKind::Main,
        confidence: 1.0,
    }];
    let config = FlowConfig { max_flows: 10, ..FlowConfig::default() };
    let flows = enumerate_flows(&entry_points, &edges, &config);
    assert!(flows.len() <= 10, "global flow cap must be respected");
}

#[test]
fn enumerate_flows_visit_budget() {
    // Large fan-out with small visit budget
    let edges: Vec<Edge> = (0..1000).map(|i| Edge {
        kind: EdgeKind::Calls,
        source: "entry".into(),
        target: format!("a{i}"),
        metadata: None,
    }).collect();
    let entry_points = vec![EntryPoint {
        qualified_name: "entry".into(),
        kind: EntryPointKind::Main,
        confidence: 1.0,
    }];
    let config = FlowConfig { visit_budget: 50, ..FlowConfig::default() };
    let flows = enumerate_flows(&entry_points, &edges, &config);
    // Should have some flows but be truncated
    assert!(flows.iter().any(|f| f.truncated) || flows.len() < 1000);
}

#[test]
fn enumerate_flows_only_high_confidence_edges() {
    let edges = vec![
        Edge { kind: EdgeKind::Calls, source: "main".into(), target: "a".into(), metadata: None },
        Edge { kind: EdgeKind::ImportsFrom, source: "a".into(), target: "b".into(), metadata: None },
    ];
    let entry_points = vec![EntryPoint {
        qualified_name: "main".into(),
        kind: EntryPointKind::Main,
        confidence: 1.0,
    }];
    let flows = enumerate_flows(&entry_points, &edges, &FlowConfig::default());
    // b should not appear in any flow — ImportsFrom is Medium
    for flow in &flows {
        assert!(!flow.path.contains(&"b".to_string()));
    }
}

#[test]
fn enumerate_flows_multiple_entry_points_share_global_cap() {
    // Two entry points, each can produce 10 flows, but global cap is 15
    let mut edges = Vec::new();
    for i in 0..10 {
        edges.push(Edge { kind: EdgeKind::Calls, source: "e1".into(), target: format!("a{i}"), metadata: None });
        edges.push(Edge { kind: EdgeKind::Calls, source: "e2".into(), target: format!("b{i}"), metadata: None });
    }
    let entry_points = vec![
        EntryPoint { qualified_name: "e1".into(), kind: EntryPointKind::Main, confidence: 1.0 },
        EntryPoint { qualified_name: "e2".into(), kind: EntryPointKind::Test, confidence: 1.0 },
    ];
    let config = FlowConfig { max_flows: 15, ..FlowConfig::default() };
    let flows = enumerate_flows(&entry_points, &edges, &config);
    assert!(flows.len() <= 15, "global cap must be shared across entry points");
    // Both entry points should contribute some flows
    let e1_flows = flows.iter().filter(|f| f.entry == "e1").count();
    let e2_flows = flows.iter().filter(|f| f.entry == "e2").count();
    assert!(e1_flows > 0 && e2_flows > 0, "both entry points should contribute flows");
}

#[test]
fn enumerate_flows_branching() {
    // main -> A, main -> B, A -> C, B -> C
    let edges = vec![
        Edge { kind: EdgeKind::Calls, source: "main".into(), target: "a".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "main".into(), target: "b".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "a".into(), target: "c".into(), metadata: None },
        Edge { kind: EdgeKind::Calls, source: "b".into(), target: "c".into(), metadata: None },
    ];
    let entry_points = vec![EntryPoint {
        qualified_name: "main".into(),
        kind: EntryPointKind::Main,
        confidence: 1.0,
    }];
    let flows = enumerate_flows(&entry_points, &edges, &FlowConfig::default());
    // Should find 2 flows: main->a->c and main->b->c
    assert_eq!(flows.len(), 2);
}
```

- [ ] Step 2: Run `cargo test -p domain --lib analysis::flow::tests::enumerate`, verify FAIL
- [ ] Step 3: Implement `enumerate_flows`:
  - Build adjacency list from High-confidence edges
  - For each entry point: DFS with per-path visited set
  - Track visit_budget per entry point
  - Terminal = no outgoing behavioral edges from current node (or all neighbors already in path)
  - `depth` = `path.len()` (node count, not edge count). `max_depth` caps `path.len()`.
  - Respect depth limit, global flow cap, visit budget
  - Mark entry point as truncated if budget exhausted
- [ ] Step 4: Run `cargo test -p domain --lib analysis::flow::tests::enumerate`, verify PASS
- [ ] Step 5: `git add crates/domain/src/analysis/flow.rs && git commit -m "feat(S01/T04): implement bounded DFS flow enumeration with cycle detection"`

---

### Task 5: FlowUseCase + Config
**Files:** Create `crates/domain/src/use_cases/flow.rs`, Modify `crates/domain/src/use_cases/mod.rs`, `crates/cli/src/config.rs`
**Traces to:** AC1, AC2, AC3, AC5

- [ ] Step 1: Write failing tests

```rust
// crates/domain/src/use_cases/flow.rs
use crate::analysis::flow::{brandes_betweenness, detect_entry_points, enumerate_flows};
use crate::error::Result;
use crate::model::*;
use crate::ports::GraphStore;
use std::collections::HashSet;

pub struct FlowUseCase<S> {
    store: S,
}

impl<S: GraphStore> FlowUseCase<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn analyze(&self, config: &FlowConfig) -> Result<FlowAnalysis> {
        todo!()
    }

    pub fn flows_through(&self, qualified_name: &str, config: &FlowConfig) -> Result<Vec<ExecutionFlow>> {
        todo!()
    }

    pub fn criticality(&self) -> Result<Vec<CriticalityScore>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryGraphStore;

    fn build_store() -> InMemoryGraphStore {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(SymbolNode {
            name: "main".into(),
            qualified_name: "src/main.rs::main".into(),
            kind: SymbolKind::Function,
            location: Location { file: "src/main.rs".into(), line_start: 1, line_end: 10, col_start: 0, col_end: 0 },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        store.insert_symbol(SymbolNode {
            name: "connect".into(),
            qualified_name: "src/db.rs::connect".into(),
            kind: SymbolKind::Function,
            location: Location { file: "src/db.rs".into(), line_start: 1, line_end: 10, col_start: 0, col_end: 0 },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        store.insert_edge(Edge {
            kind: EdgeKind::Calls,
            source: "src/main.rs::main".into(),
            target: "src/db.rs::connect".into(),
            metadata: None,
        });
        store
    }

    #[test]
    fn analyze_returns_flows_and_criticality() {
        let store = build_store();
        let uc = FlowUseCase::new(store);
        let analysis = uc.analyze(&FlowConfig::default()).unwrap();
        assert!(!analysis.entry_points.is_empty());
        assert!(!analysis.flows.is_empty());
        assert!(!analysis.criticality.is_empty());
    }

    #[test]
    fn flows_through_filters_correctly() {
        let store = build_store();
        let uc = FlowUseCase::new(store);
        let flows = uc.flows_through("src/db.rs::connect", &FlowConfig::default()).unwrap();
        for flow in &flows {
            assert!(flow.path.contains(&"src/db.rs::connect".to_string()));
        }
    }

    #[test]
    fn criticality_returns_sorted_scores() {
        let store = build_store();
        let uc = FlowUseCase::new(store);
        let scores = uc.criticality().unwrap();
        // Scores should be sorted descending by betweenness
        for w in scores.windows(2) {
            assert!(w[0].betweenness >= w[1].betweenness);
        }
    }

    #[test]
    fn flows_through_nonexistent_symbol_returns_empty() {
        let store = build_store();
        let uc = FlowUseCase::new(store);
        let flows = uc.flows_through("nonexistent::symbol", &FlowConfig::default()).unwrap();
        assert!(flows.is_empty());
    }

    #[test]
    fn flows_through_ignores_medium_confidence_reachability() {
        // Symbol reachable via ImportsFrom (Medium) but NOT via Calls (High)
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(SymbolNode {
            name: "main".into(),
            qualified_name: "src/main.rs::main".into(),
            kind: SymbolKind::Function,
            location: Location { file: "src/main.rs".into(), line_start: 1, line_end: 10, col_start: 0, col_end: 0 },
            visibility: Visibility::Public, is_exported: true, is_async: false, is_test: false,
            decorators: vec![], signature: None,
        });
        store.insert_symbol(SymbolNode {
            name: "util".into(),
            qualified_name: "src/util.rs::util".into(),
            kind: SymbolKind::Function,
            location: Location { file: "src/util.rs".into(), line_start: 1, line_end: 10, col_start: 0, col_end: 0 },
            visibility: Visibility::Public, is_exported: true, is_async: false, is_test: false,
            decorators: vec![], signature: None,
        });
        // Only Medium-confidence edge connecting them
        store.insert_edge(Edge {
            kind: EdgeKind::ImportsFrom,
            source: "src/main.rs::main".into(),
            target: "src/util.rs::util".into(),
            metadata: None,
        });
        let uc = FlowUseCase::new(store);
        let flows = uc.flows_through("src/util.rs::util", &FlowConfig::default()).unwrap();
        assert!(flows.is_empty(), "backward BFS must filter on High-confidence edges only");
    }

    #[test]
    fn analyze_empty_graph_returns_zeros() {
        let store = InMemoryGraphStore::new();
        let uc = FlowUseCase::new(store);
        let analysis = uc.analyze(&FlowConfig::default()).unwrap();
        assert!(analysis.entry_points.is_empty());
        assert!(analysis.flows.is_empty());
        assert_eq!(analysis.stats.total_entry_points, 0);
        assert_eq!(analysis.stats.total_flows, 0);
    }
}
```

Add module registration in `crates/domain/src/use_cases/mod.rs`:
```rust
pub mod flow;
```

Add FlowsConfig to `crates/cli/src/config.rs`:
```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FlowsConfig {
    pub extra_entry_points: Option<Vec<String>>,
    pub excluded_entry_points: Option<Vec<String>>,
}

// In CodeGraphConfig:
pub flows: Option<FlowsConfig>,
```

Add config parse test in `crates/cli/src/config.rs`:
```rust
#[test]
fn flows_config_parses() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(".code-graph");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        r#"
[flows]
extra_entry_points = ["src/custom.rs::handler"]
excluded_entry_points = ["src/test_helper.rs::setup"]
"#,
    ).unwrap();
    let config = load_config(tmp.path()).unwrap();
    let flows = config.flows.unwrap();
    assert_eq!(flows.extra_entry_points.unwrap(), vec!["src/custom.rs::handler"]);
    assert_eq!(flows.excluded_entry_points.unwrap(), vec!["src/test_helper.rs::setup"]);
}
```

- [ ] Step 2: Run `cargo test -p domain --lib use_cases::flow::tests`, verify FAIL (todo!())
- [ ] Step 3: Implement FlowUseCase methods:

  **`analyze()`:**
  ```rust
  pub fn analyze(&self, config: &FlowConfig) -> Result<FlowAnalysis> {
      let symbols = self.store.all_symbols()?;
      let edges = self.store.all_edges()?;
      let entry_points = detect_entry_points(&symbols, &edges, config);
      let flows = enumerate_flows(&entry_points, &edges, config);
      let nodes: HashSet<String> = symbols.iter().map(|s| s.qualified_name.clone()).collect();
      let betweenness = brandes_betweenness(&nodes, &edges);
      let entry_set: HashSet<&str> = entry_points.iter().map(|e| e.qualified_name.as_str()).collect();
      // Count flows per node
      let mut flow_counts: HashMap<String, usize> = HashMap::new();
      for flow in &flows {
          for node in &flow.path {
              *flow_counts.entry(node.clone()).or_default() += 1;
          }
      }
      let criticality: Vec<CriticalityScore> = betweenness.iter().map(|(name, &score)| {
          CriticalityScore {
              qualified_name: name.clone(),
              betweenness: score,
              flow_count: flow_counts.get(name).copied().unwrap_or(0),
              is_entry_point: entry_set.contains(name.as_str()),
          }
      }).collect();
      let stats = FlowStats {
          total_entry_points: entry_points.len(),
          total_flows: flows.len(),
          max_depth: flows.iter().map(|f| f.depth).max().unwrap_or(0),
          avg_depth: if flows.is_empty() { 0.0 } else {
              flows.iter().map(|f| f.depth as f64).sum::<f64>() / flows.len() as f64
          },
      };
      Ok(FlowAnalysis { entry_points, flows, criticality, stats })
  }
  ```

  **`flows_through()`:**
  ```rust
  pub fn flows_through(&self, qualified_name: &str, config: &FlowConfig) -> Result<Vec<ExecutionFlow>> {
      let symbols = self.store.all_symbols()?;
      let edges = self.store.all_edges()?;
      let entry_points = detect_entry_points(&symbols, &edges, config);
      // Backward BFS from target through HIGH-CONFIDENCE edges only
      let high_edges: Vec<&Edge> = edges.iter()
          .filter(|e| e.kind.confidence() == Confidence::High)
          .collect();
      let mut reachable_entries = HashSet::new();
      let mut visited = HashSet::new();
      let mut queue = std::collections::VecDeque::new();
      queue.push_back(qualified_name.to_string());
      visited.insert(qualified_name.to_string());
      while let Some(node) = queue.pop_front() {
          if entry_points.iter().any(|ep| ep.qualified_name == node) {
              reachable_entries.insert(node.clone());
          }
          for edge in &high_edges {
              if edge.target == node && !visited.contains(&edge.source) {
                  visited.insert(edge.source.clone());
                  queue.push_back(edge.source.clone());
              }
          }
      }
      // DFS only from reachable entry points, filter to paths containing target
      let filtered_entries: Vec<EntryPoint> = entry_points.into_iter()
          .filter(|ep| reachable_entries.contains(&ep.qualified_name))
          .collect();
      let all_flows = enumerate_flows(&filtered_entries, &edges, config);
      Ok(all_flows.into_iter().filter(|f| f.path.contains(&qualified_name.to_string())).collect())
  }
  ```

  **`criticality()`:**
  ```rust
  pub fn criticality(&self) -> Result<Vec<CriticalityScore>> {
      let analysis = self.analyze(&FlowConfig::default())?;
      let mut scores = analysis.criticality;
      scores.sort_by(|a, b| b.betweenness.partial_cmp(&a.betweenness).unwrap_or(std::cmp::Ordering::Equal));
      Ok(scores)
  }
  ```

- [ ] Step 4: Run `cargo test -p domain --lib use_cases::flow::tests && cargo test -p cli --lib config`, verify PASS
- [ ] Step 5: `git add crates/domain/src/use_cases/flow.rs crates/domain/src/use_cases/mod.rs crates/cli/src/config.rs && git commit -m "feat(S01/T05): implement FlowUseCase and add FlowsConfig"`

---

### Task 6: CLI Flows Command + Output Formatting
**Files:** Create `crates/cli/src/commands/flows.rs`, Modify `crates/cli/src/commands/mod.rs`, `crates/cli/src/output.rs`, `crates/cli/src/lib.rs`, `crates/domain/src/lib.rs`
**Traces to:** AC1, AC2, AC3, AC11

- [ ] Step 1: Write failing tests

In `crates/cli/src/commands/mod.rs` — add FlowsArgs and Flows variant:
```rust
pub mod flows;

/// Analyze execution flows and criticality
Flows(FlowsArgs),

#[derive(clap::Args)]
pub struct FlowsArgs {
    /// Filter flows through a specific symbol
    #[arg(long)]
    pub symbol: Option<String>,
    /// Show criticality ranking instead of flows
    #[arg(long)]
    pub rank: bool,
    /// Maximum flow depth
    #[arg(long, default_value = "20")]
    pub depth: usize,
    /// Maximum number of results to display
    #[arg(long, default_value = "20")]
    pub limit: usize,
}
```

In `crates/cli/src/output.rs` — add Displayable impls with tests for all 6 format combinations:
```rust
fn sample_flow_analysis() -> FlowAnalysis {
    FlowAnalysis {
        entry_points: vec![EntryPoint {
            qualified_name: "main".into(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        }],
        flows: vec![ExecutionFlow {
            entry: "main".into(),
            path: vec!["main".into(), "db.connect".into()],
            depth: 2,
            truncated: false,
        }],
        criticality: vec![],
        stats: FlowStats { total_entry_points: 1, total_flows: 1, max_depth: 2, avg_depth: 2.0 },
    }
}

fn sample_criticality() -> Vec<CriticalityScore> {
    vec![CriticalityScore {
        qualified_name: "db.query".into(),
        betweenness: 0.847,
        flow_count: 312,
        is_entry_point: false,
    }]
}

// --- FlowAnalysis: 3 formats ---

#[test]
fn flow_analysis_compact_format() {
    let analysis = sample_flow_analysis();
    let mut buf = Vec::new();
    analysis.fmt_compact(&mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("Entry points: 1"));
    assert!(s.contains("main"));
    assert!(s.contains("db.connect"));
}

#[test]
fn flow_analysis_table_format() {
    let analysis = sample_flow_analysis();
    let mut buf = Vec::new();
    analysis.fmt_table(&mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("Entry"));
    assert!(s.contains("Path"));
    assert!(s.contains("main"));
}

#[test]
fn flow_analysis_json_format() {
    let analysis = sample_flow_analysis();
    let mut buf = Vec::new();
    analysis.fmt_json(&mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(parsed["stats"]["total_flows"], 1);
    assert_eq!(parsed["flows"][0]["entry"], "main");
}

// --- Vec<CriticalityScore>: 3 formats ---

#[test]
fn criticality_compact_format() {
    let scores = sample_criticality();
    let mut buf = Vec::new();
    scores.fmt_compact(&mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("db.query"));
    assert!(s.contains("0.847"));
}

#[test]
fn criticality_table_format() {
    let scores = sample_criticality();
    let mut buf = Vec::new();
    scores.fmt_table(&mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("Symbol"));
    assert!(s.contains("Betweenness"));
    assert!(s.contains("db.query"));
    assert!(s.contains("0.847"));
    assert!(s.contains("312"));
}

#[test]
fn criticality_json_format() {
    let scores = sample_criticality();
    let mut buf = Vec::new();
    scores.fmt_json(&mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed[0]["qualified_name"], "db.query");
    assert_eq!(parsed[0]["betweenness"], 0.847);
}
```

- [ ] Step 2: Run `cargo test -p cli`, verify FAIL (new types not displayable)
- [ ] Step 3: Implement:

  **`commands/flows.rs` — `run_flows()`:**
  ```rust
  use domain::model::{FlowConfig, FlowAnalysis, CriticalityScore};
  use domain::use_cases::flow::FlowUseCase;
  use crate::commands::helpers::open_graph;
  use crate::config::load_config;
  use crate::output::{print, OutputFormat};

  pub fn run_flows(args: &FlowsArgs, output_format: OutputFormat) -> Result<()> {
      let (store, root) = open_graph()?;
      let config = load_config(&root)?;
      let mut flow_config = FlowConfig { max_depth: args.depth, ..FlowConfig::default() };
      // Wire FlowsConfig from config.toml into FlowConfig
      if let Some(fc) = &config.flows {
          if let Some(extra) = &fc.extra_entry_points {
              flow_config.extra_entry_points = extra.clone();
          }
          if let Some(excluded) = &fc.excluded_entry_points {
              flow_config.excluded_entry_points = excluded.clone();
          }
      }
      let uc = FlowUseCase::new(store);
      if args.rank {
          let mut scores = uc.criticality()?;
          scores.truncate(args.limit);
          print(&scores, output_format);
      } else if let Some(ref symbol) = args.symbol {
          let flows = uc.flows_through(symbol, &flow_config)?;
          // Wrap filtered flows in FlowAnalysis for display
          // ... (construct FlowAnalysis with filtered results)
          print(&analysis, output_format);
      } else {
          let mut analysis = uc.analyze(&flow_config)?;
          analysis.flows.truncate(args.limit);
          print(&analysis, output_format);
      }
      Ok(())
  }
  ```

  **`output.rs` — Displayable for FlowAnalysis:**
  - `fmt_compact`: Print entry point summary line, then numbered flows as `[i] a -> b -> c (depth N)`
  - `fmt_table`: Header `Entry | Path | Depth | Truncated`, then one row per flow
  - `fmt_json`: `serde_json::to_string_pretty(&self)`

  **`output.rs` — Displayable for Vec<CriticalityScore>:**
  - `fmt_compact`: Numbered list `1 Symbol  betweenness=0.XXX  flows=N  entry=yes/no`
  - `fmt_table`: Header `# | Symbol | Betweenness | Flows | Entry?`, columnar rows
  - `fmt_json`: `serde_json::to_string_pretty(&self)`

  **`lib.rs`:** Add `Commands::Flows(args) => commands::flows::run_flows(args, output_format)`

  **`domain/src/lib.rs`:** Re-export new types from `model` + `use_cases::flow`

- [ ] Step 4: Run `cargo test -p cli && cargo test -p domain`, verify PASS
- [ ] Step 5: `git add crates/cli/src/commands/flows.rs crates/cli/src/commands/mod.rs crates/cli/src/output.rs crates/cli/src/lib.rs crates/domain/src/lib.rs && git commit -m "feat(S01/T06): add flows CLI command with compact/table/json output"`

---

### Task 7: Stats Integration
**Files:** Modify `crates/cli/src/commands/stats.rs`, `crates/cli/src/output.rs`
**Traces to:** AC10

- [ ] Step 1: Write failing test

```rust
// In crates/cli/src/output.rs tests
#[test]
fn graph_stats_compact_with_flow_fields() {
    let stats = GraphStats {
        files: 234,
        symbols: 1892,
        edges: 5431,
        entry_point_count: Some(12),
        avg_criticality: Some(0.034),
    };
    let mut buf = Vec::new();
    stats.fmt_compact(&mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("Entry points: 12"));
    assert!(s.contains("Avg criticality: 0.034"));
}

#[test]
fn graph_stats_compact_without_flow_fields() {
    let stats = GraphStats {
        files: 10,
        symbols: 50,
        edges: 100,
        entry_point_count: None,
        avg_criticality: None,
    };
    let mut buf = Vec::new();
    stats.fmt_compact(&mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(!s.contains("Entry points"));
    assert!(!s.contains("Avg criticality"));
}
```

Add zero-symbol edge case test (AC10):
```rust
#[test]
fn graph_stats_zero_symbols_shows_zero_flow_fields() {
    let stats = GraphStats {
        files: 0,
        symbols: 0,
        edges: 0,
        entry_point_count: Some(0),
        avg_criticality: Some(0.0),
    };
    let mut buf = Vec::new();
    stats.fmt_compact(&mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("Entry points: 0"));
}
```

- [ ] Step 2: Run `cargo test -p cli --lib output::tests::graph_stats_compact_with_flow_fields`, verify FAIL
- [ ] Step 3: Implement:

  **`output.rs` — Update GraphStats Displayable:**
  ```rust
  // In fmt_compact, after the existing Files | Symbols | Edges line:
  if let Some(ep) = self.entry_point_count {
      write!(w, "Entry points: {ep}")?;
      if let Some(ac) = self.avg_criticality {
          writeln!(w, " | Avg criticality: {ac:.3}")?;
      } else {
          writeln!(w)?;
      }
  }
  // In fmt_table, after existing rows:
  if let Some(ep) = self.entry_point_count {
      writeln!(w, "Entry pts | {ep}")?;
  }
  if let Some(ac) = self.avg_criticality {
      writeln!(w, "Avg crit  | {ac:.3}")?;
  }
  ```

  **`stats.rs` — Update run_stats:**
  ```rust
  pub fn run_stats(output_format: OutputFormat) -> Result<()> {
      let (store, _root) = open_graph()?;
      let uc = QueryUseCase::new(store.clone(), store.clone());
      let mut stats = uc.stats()?;
      // On-demand flow analysis integration
      let flow_uc = FlowUseCase::new(store);
      let flow_config = FlowConfig::default();
      let symbols = flow_uc.store.all_symbols().unwrap_or_default();  // or via analyze
      let analysis = flow_uc.analyze(&flow_config)?;
      stats.entry_point_count = Some(analysis.stats.total_entry_points);
      // Only compute avg_criticality if <= 5000 symbols (expensive for large graphs)
      if stats.symbols <= 5000 {
          let avg = if analysis.criticality.is_empty() { 0.0 } else {
              analysis.criticality.iter().map(|c| c.betweenness).sum::<f64>()
                  / analysis.criticality.len() as f64
          };
          stats.avg_criticality = Some(avg);
      }
      print(&stats, output_format);
      Ok(())
  }
  ```

- [ ] Step 4: Run `cargo test -p cli`, verify PASS
- [ ] Step 5: `git add crates/cli/src/commands/stats.rs crates/cli/src/output.rs && git commit -m "feat(S01/T07): integrate flow analysis into stats command"`

---

### Task 8: Self-Test Validation
**Files:** No file changes — validation only
**Traces to:** AC12

- [ ] Step 1: Build the project: `cargo build`
- [ ] Step 2: Index the project's own codebase: `cargo run -- index`
- [ ] Step 3: Run `cargo run -- flows` — verify exit code 0, >= 1 entry point, >= 1 flow
- [ ] Step 4: Run `cargo run -- flows --rank` — verify non-empty ranked list
- [ ] Step 5: Run `cargo run -- flows --json` and `cargo run -- flows --rank --json` — verify valid JSON output
- [ ] Step 6: Run `cargo run -- stats` — verify entry point count appears
- [ ] Step 7: Run full test suite: `cargo test`

---

## Waves

```
Wave 1 (parallel):  T01 (domain types)
Wave 2 (parallel):  T02 (entry detection), T03 (betweenness), T04 (flow enum)
Wave 3 (serial):    T05 (FlowUseCase + config)
Wave 4 (parallel):  T06 (CLI + output), T07 (stats integration)
Wave 5 (serial):    T08 (self-test validation)
```

**Dependencies:**
- T02, T03, T04 all depend on T01 (need domain types)
- T05 depends on T02, T03, T04 (orchestrates all algorithms)
- T06 depends on T05 (CLI calls use case)
- T07 depends on T05 (stats calls use case)
- T08 depends on T06, T07 (end-to-end validation)
