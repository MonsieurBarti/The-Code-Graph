# M01-S01 Implementation Plan

> For agentic workers: execute task-by-task with TDD.

**Goal:** Create a Cargo workspace with a fully implemented `domain` crate containing all business types, error hierarchy, port traits, use-case structs, traversal, and analysis logic.

**Architecture:** Hexagonal — domain crate has zero external deps beyond serde (derive) and thiserror. Port traits define contracts; no adapters in this slice.

**Tech Stack:** Rust, serde 1.x (derive), thiserror 2.x

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `Cargo.toml` | Create | Workspace root |
| `crates/domain/Cargo.toml` | Create | Domain crate manifest |
| `crates/domain/src/lib.rs` | Create | Re-exports all public modules |
| `crates/domain/src/error.rs` | Create | CodeGraphError + Result type alias |
| `crates/domain/src/model.rs` | Create | All domain types, enums, Node, Edge, supporting types, QualifiedName |
| `crates/domain/src/ports.rs` | Create | GraphStore, SearchIndex, GitProvider, FileSystem traits |
| `crates/domain/src/traversal.rs` | Create | InMemoryGraph, BFS, DFS, bfs_filtered |
| `crates/domain/src/use_cases/mod.rs` | Create | Module declarations |
| `crates/domain/src/use_cases/index.rs` | Create | IndexUseCase<S, F, G> |
| `crates/domain/src/use_cases/query.rs` | Create | QueryUseCase<S, I> |
| `crates/domain/src/use_cases/impact.rs` | Create | ImpactUseCase<S> |
| `crates/domain/src/analysis/mod.rs` | Create | Module declarations |
| `crates/domain/src/analysis/blast_radius.rs` | Create | compute_blast_radius() |
| `crates/domain/src/analysis/change_detection.rs` | Create | find_affected_symbols() |
| `crates/domain/src/analysis/impact.rs` | Create | compute_diff_impact() |
| `crates/domain/src/test_support.rs` | Create | InMemoryGraphStore mock (cfg(test)) |

---

## Wave 0 — Foundation (parallel, no deps)

### T01: Workspace scaffold + error hierarchy
**Files:** Create `Cargo.toml`, `crates/domain/Cargo.toml`, `crates/domain/src/lib.rs`, `crates/domain/src/error.rs`
**Traces to:** AC1, AC8, AC10

- [ ] Step 1: Create minimal scaffold first (both `Cargo.toml` files + `lib.rs` with `pub mod error;`), then write failing test in `crates/domain/src/error.rs` — tests fail because error types don't exist yet, not because the crate is missing
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use std::path::PathBuf;

      #[test]
      fn parse_error_display_contains_file_and_message() {
          let err = CodeGraphError::Parse {
              file: PathBuf::from("src/main.rs"),
              message: "unexpected token".into(),
          };
          let msg = format!("{err}");
          assert!(msg.contains("src/main.rs"), "missing file in: {msg}");
          assert!(msg.contains("unexpected token"), "missing message in: {msg}");
      }

      #[test]
      fn filesystem_error_display_contains_path() {
          let err = CodeGraphError::FileSystem {
              path: PathBuf::from("/some/path"),
              source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
          };
          let msg = format!("{err}");
          assert!(msg.contains("/some/path"), "missing path in: {msg}");
      }

      #[test]
      fn all_variants_display_nonempty() {
          let errors = vec![
              CodeGraphError::Parse { file: "f".into(), message: "m".into() },
              CodeGraphError::Resolution("r".into()),
              CodeGraphError::Storage("s".into()),
              CodeGraphError::Git("g".into()),
              CodeGraphError::FileSystem {
                  path: "p".into(),
                  source: std::io::Error::new(std::io::ErrorKind::Other, "e"),
              },
              CodeGraphError::NoProject,
              CodeGraphError::BlocklistedRoot("/".into()),
              CodeGraphError::IndexNotBuilt,
              CodeGraphError::Other("o".into()),
          ];
          for err in &errors {
              assert!(!format!("{err}").is_empty(), "empty display for {err:?}");
          }
      }
  }
  ```
- [ ] Step 2: Run `cargo test -p domain -- error`, verify FAIL (module doesn't exist yet)
- [ ] Step 3: Create workspace `Cargo.toml`, domain `Cargo.toml` (serde + thiserror), `lib.rs` (with `pub mod error;`), and full `error.rs` implementation
  ```toml
  # Cargo.toml (workspace root)
  [workspace]
  members = ["crates/domain"]
  resolver = "2"
  ```
  ```toml
  # crates/domain/Cargo.toml
  [package]
  name = "domain"
  version = "0.1.0"
  edition = "2021"

  [dependencies]
  serde = { version = "1", features = ["derive"] }
  thiserror = "2"

  [dev-dependencies]
  serde_json = "1"
  ```
- [ ] Step 4: Run `cargo test -p domain -- error`, verify PASS
- [ ] Step 5: `git add Cargo.toml crates/domain/ && git commit -m "feat(S01/T01): workspace scaffold + error hierarchy"`

### T02: Core domain types + QualifiedName
**Files:** Create `crates/domain/src/model.rs`
**Traces to:** AC2, AC9, AC13, AC14, AC15

- [ ] Step 1: Write failing tests — `crates/domain/src/model.rs` test module
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn confidence_ordering() {
          assert!(Confidence::Structural < Confidence::Low);
          assert!(Confidence::Low < Confidence::Medium);
          assert!(Confidence::Medium < Confidence::High);
      }

      #[test]
      fn all_16_edge_kinds_have_confidence() {
          let edges = [
              (EdgeKind::Calls, Confidence::High),
              (EdgeKind::Extends, Confidence::High),
              (EdgeKind::Implements, Confidence::High),
              (EdgeKind::Embeds, Confidence::High),
              (EdgeKind::ImportsFrom, Confidence::Medium),
              (EdgeKind::BarrelReExportAll, Confidence::Medium),
              (EdgeKind::ReExport, Confidence::Medium),
              (EdgeKind::TypeReference, Confidence::Medium),
              (EdgeKind::DotImport, Confidence::Medium),
              (EdgeKind::DependsOn, Confidence::Low),
              (EdgeKind::ConditionalImport, Confidence::Low),
              (EdgeKind::SideEffectImport, Confidence::Low),
              (EdgeKind::Contains, Confidence::Structural),
              (EdgeKind::ChildOf, Confidence::Structural),
              (EdgeKind::HasDecorator, Confidence::Structural),
              (EdgeKind::TestedBy, Confidence::Structural),
          ];
          for (kind, expected) in &edges {
              assert_eq!(kind.confidence(), *expected, "wrong confidence for {kind:?}");
          }
          assert_eq!(edges.len(), 16, "expected 16 edge kinds");
      }

      #[test]
      fn qualified_name_parse_valid() {
          let qn = QualifiedName::parse("src/file.rs::MyStruct.method").unwrap();
          assert_eq!(qn.file_path(), "src/file.rs");
          assert_eq!(qn.symbol_path(), "MyStruct.method");
          assert_eq!(qn.as_str(), "src/file.rs::MyStruct.method");
      }

      #[test]
      fn qualified_name_rejects_empty() {
          assert!(QualifiedName::parse("").is_err());
      }

      #[test]
      fn qualified_name_rejects_missing_separator() {
          assert!(QualifiedName::parse("no_separator").is_err());
      }

      #[test]
      fn qualified_name_rejects_empty_file_path() {
          assert!(QualifiedName::parse("::symbol").is_err());
      }

      #[test]
      fn qualified_name_rejects_empty_symbol_path() {
          assert!(QualifiedName::parse("file::").is_err());
      }

      #[test]
      fn qualified_name_borrow_str_hashmap_lookup() {
          use std::collections::HashMap;
          let mut map: HashMap<QualifiedName, u32> = HashMap::new();
          let qn = QualifiedName::parse("src/lib.rs::foo").unwrap();
          map.insert(qn, 42);
          assert_eq!(map.get("src/lib.rs::foo"), Some(&42));
      }

      #[test]
      fn qualified_name_serde_roundtrip() {
          let qn = QualifiedName::parse("src/lib.rs::Foo.bar").unwrap();
          let json = serde_json::to_string(&qn).unwrap();
          let qn2: QualifiedName = serde_json::from_str(&json).unwrap();
          assert_eq!(qn, qn2);
      }

      #[test]
      fn node_id_returns_correct_identifier() {
          let file = Node::File(FileNode {
              path: "src/main.rs".into(),
              language: Language::Rust,
              hash: "abc".into(),
          });
          assert_eq!(file.id(), "src/main.rs");

          let sym = Node::Symbol(SymbolNode {
              name: "foo".into(),
              qualified_name: "src/lib.rs::foo".into(),
              kind: SymbolKind::Function,
              location: Location {
                  file: "src/lib.rs".into(),
                  line_start: 1, line_end: 5, col_start: 0, col_end: 1,
              },
              visibility: Visibility::Public,
              is_exported: true, is_async: false, is_test: false,
              decorators: vec![], signature: None,
          });
          assert_eq!(sym.id(), "src/lib.rs::foo");
      }

      #[test]
      fn serde_roundtrip_all_supporting_types() {
          // AC9 + AC15: every public type (except CodeGraphError) must round-trip
          macro_rules! assert_roundtrip {
              ($val:expr, $ty:ty) => {{
                  let json = serde_json::to_string(&$val).unwrap();
                  let _: $ty = serde_json::from_str(&json).unwrap();
              }};
          }

          // Enums
          assert_roundtrip!(Language::Rust, Language);
          assert_roundtrip!(NodeKind::Symbol, NodeKind);
          assert_roundtrip!(SymbolKind::Function, SymbolKind);
          assert_roundtrip!(NonParsedKind::Doc, NonParsedKind);
          assert_roundtrip!(Visibility::Public, Visibility);
          assert_roundtrip!(Confidence::High, Confidence);
          assert_roundtrip!(EdgeKind::Calls, EdgeKind);
          assert_roundtrip!(Direction::Forward, Direction);

          // Core types
          let loc = Location { file: "f".into(), line_start: 1, line_end: 2, col_start: 0, col_end: 10 };
          assert_roundtrip!(loc, Location);

          let file_node = FileNode { path: "f".into(), language: Language::Rust, hash: "h".into() };
          assert_roundtrip!(file_node.clone(), FileNode);
          assert_roundtrip!(Node::File(file_node), Node);

          let sym = SymbolNode {
              name: "s".into(), qualified_name: "f::s".into(), kind: SymbolKind::Function,
              location: Location { file: "f".into(), line_start: 1, line_end: 2, col_start: 0, col_end: 0 },
              visibility: Visibility::Public, is_exported: true, is_async: false, is_test: false,
              decorators: vec![], signature: None,
          };
          assert_roundtrip!(sym, SymbolNode);

          let np = NonParsedNode { path: "r.md".into(), file_kind: NonParsedKind::Doc, hash: "h".into() };
          assert_roundtrip!(np, NonParsedNode);

          let edge = Edge { kind: EdgeKind::Calls, source: "a".into(), target: "b".into(), metadata: None };
          assert_roundtrip!(edge, Edge);

          // Supporting types (AC15 list)
          assert_roundtrip!(ImpactTarget::File("f".into()), ImpactTarget);
          assert_roundtrip!(ImpactTarget::Symbol("s".into()), ImpactTarget);
          assert_roundtrip!(TraversalResult { node: "n".into(), depth: 1, path: vec![], edge_kind: EdgeKind::Calls }, TraversalResult);
          assert_roundtrip!(SearchResult { qualified_name: "f::s".into(), name: "s".into(), kind: SymbolKind::Function, file_path: "f".into(), score: 1.0 }, SearchResult);
          assert_roundtrip!(Reference { source: "s".into(), edge_kind: EdgeKind::Calls, location: None }, Reference);
          assert_roundtrip!(IndexStats { files_indexed: 1, symbols_extracted: 2, edges_created: 3, duration: std::time::Duration::from_secs(1) }, IndexStats);
          assert_roundtrip!(GraphStats { files: 1, symbols: 2, edges: 3 }, GraphStats);
          assert_roundtrip!(DiffHunk { file: "f".into(), old_start: 1, old_count: 2, new_start: 1, new_count: 3 }, DiffHunk);
          assert_roundtrip!(AffectedNode { qualified_name: "q".into(), depth: 1, confidence: Confidence::High, path: vec![] }, AffectedNode);
          assert_roundtrip!(ImpactReport { targets: vec![], affected: vec![], depth: 3, min_confidence: Confidence::Structural }, ImpactReport);
          assert_roundtrip!(DiffImpactReport { changed_symbols: vec![], impact: ImpactReport { targets: vec![], affected: vec![], depth: 0, min_confidence: Confidence::Structural } }, DiffImpactReport);
      }
  }
  ```
- [ ] Step 2: Run `cargo test -p domain -- model`, verify FAIL
- [ ] Step 3: Implement full `model.rs` — all enums, structs, QualifiedName newtype with FromStr, TryFrom, Borrow<str>, serde(try_from), Node::id(), EdgeKind::confidence()
- [ ] Step 4: Run `cargo test -p domain -- model`, verify PASS
- [ ] Step 5: `git add crates/domain/src/model.rs && git commit -m "feat(S01/T02): core domain types + QualifiedName newtype"`

---

## Wave 1 — Contracts (depends on Wave 0)

### T03: Port traits
**Files:** Create `crates/domain/src/ports.rs`
**Traces to:** AC3

- [ ] Step 1: Write failing test — compile-time Send + Sync assertion
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      fn assert_send_sync<T: Send + Sync>() {}

      #[test]
      fn graph_store_is_send_sync() {
          assert_send_sync::<Box<dyn GraphStore>>();
      }

      #[test]
      fn search_index_is_send_sync() {
          assert_send_sync::<Box<dyn SearchIndex>>();
      }

      #[test]
      fn git_provider_is_send_sync() {
          assert_send_sync::<Box<dyn GitProvider>>();
      }

      #[test]
      fn file_system_is_send_sync() {
          assert_send_sync::<Box<dyn FileSystem>>();
      }
  }
  ```
- [ ] Step 2: Run `cargo test -p domain -- ports`, verify FAIL
- [ ] Step 3: Implement `ports.rs` — all 4 traits with full method signatures, all returning `Result<T>`
- [ ] Step 4: Run `cargo test -p domain -- ports`, verify PASS
- [ ] Step 5: `git add crates/domain/src/ports.rs && git commit -m "feat(S01/T03): outbound port traits (GraphStore, SearchIndex, GitProvider, FileSystem)"`

### T04: InMemoryGraph + BFS/DFS
**Files:** Create `crates/domain/src/traversal.rs`
**Traces to:** AC4, AC5, AC6, AC18, AC19

- [ ] Step 1: Write failing tests
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::model::*;

      fn make_edge(src: &str, tgt: &str, kind: EdgeKind) -> Edge {
          Edge { kind, source: src.into(), target: tgt.into(), metadata: None }
      }

      #[test]
      fn bfs_returns_nodes_at_correct_depth() {
          let edges = vec![
              make_edge("A", "B", EdgeKind::Calls),
              make_edge("B", "C", EdgeKind::Calls),
              make_edge("C", "D", EdgeKind::Calls),
          ];
          let graph = InMemoryGraph::from_edges(edges);
          let results = graph.bfs("A", Direction::Forward, 3);
          assert_eq!(results.len(), 3);
          assert!(results.iter().any(|r| r.node == "B" && r.depth == 1));
          assert!(results.iter().any(|r| r.node == "C" && r.depth == 2));
          assert!(results.iter().any(|r| r.node == "D" && r.depth == 3));
      }

      #[test]
      fn bfs_respects_max_depth() {
          let edges = vec![
              make_edge("A", "B", EdgeKind::Calls),
              make_edge("B", "C", EdgeKind::Calls),
              make_edge("C", "D", EdgeKind::Calls),
          ];
          let graph = InMemoryGraph::from_edges(edges);
          let results = graph.bfs("A", Direction::Forward, 2);
          assert_eq!(results.len(), 2);
      }

      #[test]
      fn bfs_filtered_excludes_low_confidence() {
          let edges = vec![
              make_edge("A", "B", EdgeKind::Calls),
              make_edge("A", "C", EdgeKind::DependsOn),
              make_edge("B", "D", EdgeKind::ImportsFrom),
          ];
          let graph = InMemoryGraph::from_edges(edges);
          let results = graph.bfs_filtered("A", Direction::Forward, 3, Confidence::High);
          let names: Vec<&str> = results.iter().map(|r| r.node.as_str()).collect();
          assert!(names.contains(&"B"));
          assert!(!names.contains(&"C"));
          assert!(!names.contains(&"D"));
      }

      #[test]
      fn dfs_detects_cycle_without_infinite_loop() {
          let edges = vec![
              make_edge("A", "B", EdgeKind::Calls),
              make_edge("B", "C", EdgeKind::Calls),
              make_edge("C", "A", EdgeKind::Calls),
          ];
          let graph = InMemoryGraph::from_edges(edges);
          let results = graph.dfs("A", Direction::Forward);
          assert!(results.len() <= 3);
          assert!(results.iter().filter(|r| r.node == "B").count() <= 1);
      }

      #[test]
      fn bfs_empty_graph_returns_empty() {
          let graph = InMemoryGraph::from_edges(vec![]);
          let results = graph.bfs("nonexistent", Direction::Forward, 10);
          assert!(results.is_empty());
      }

      #[test]
      fn dfs_empty_graph_returns_empty() {
          let graph = InMemoryGraph::from_edges(vec![]);
          let results = graph.dfs("nonexistent", Direction::Forward);
          assert!(results.is_empty());
      }

      #[test]
      fn self_referential_edge_handled_gracefully() {
          let edges = vec![make_edge("A", "A", EdgeKind::Calls)];
          let graph = InMemoryGraph::from_edges(edges);
          let results = graph.bfs("A", Direction::Forward, 5);
          assert!(results.iter().filter(|r| r.node == "A").count() <= 1);
      }

      #[test]
      fn bfs_backward_finds_callers() {
          let edges = vec![
              make_edge("A", "C", EdgeKind::Calls),
              make_edge("B", "C", EdgeKind::Calls),
          ];
          let graph = InMemoryGraph::from_edges(edges);
          let results = graph.bfs("C", Direction::Backward, 1);
          assert_eq!(results.len(), 2);
          let names: Vec<&str> = results.iter().map(|r| r.node.as_str()).collect();
          assert!(names.contains(&"A"));
          assert!(names.contains(&"B"));
      }
  }
  ```
- [ ] Step 2: Run `cargo test -p domain -- traversal`, verify FAIL
- [ ] Step 3: Implement `traversal.rs` — `InMemoryGraph::from_edges`, `bfs`, `bfs_filtered`, `dfs` using private `bfs_inner<F>` helper
- [ ] Step 4: Run `cargo test -p domain -- traversal`, verify PASS
- [ ] Step 5: `git add crates/domain/src/traversal.rs && git commit -m "feat(S01/T04): InMemoryGraph with BFS/DFS + confidence filtering"`

---

## Wave 2 — Analysis (depends on Wave 1)

### T05: Change detection
**Files:** Create `crates/domain/src/analysis/mod.rs`, `crates/domain/src/analysis/change_detection.rs`
**Traces to:** AC7, AC12, AC20

- [ ] Step 1: Write failing tests
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::model::*;

      fn sym(name: &str, file: &str, start: usize, end: usize) -> SymbolNode {
          SymbolNode {
              name: name.into(),
              qualified_name: format!("{file}::{name}"),
              kind: SymbolKind::Function,
              location: Location {
                  file: file.into(),
                  line_start: start, line_end: end,
                  col_start: 0, col_end: 0,
              },
              visibility: Visibility::Public,
              is_exported: false, is_async: false, is_test: false,
              decorators: vec![], signature: None,
          }
      }

      #[test]
      fn overlapping_hunk_matches_symbol() {
          let symbols = vec![sym("foo", "src/a.rs", 10, 20)];
          let hunks = vec![DiffHunk {
              file: "src/a.rs".into(),
              old_start: 15, old_count: 3, new_start: 15, new_count: 5,
          }];
          let affected = find_affected_symbols(&hunks, &symbols);
          assert_eq!(affected.len(), 1);
          assert_eq!(affected[0].name, "foo");
      }

      #[test]
      fn non_overlapping_hunk_no_match() {
          let symbols = vec![sym("foo", "src/a.rs", 10, 20)];
          let hunks = vec![DiffHunk {
              file: "src/a.rs".into(),
              old_start: 25, old_count: 3, new_start: 25, new_count: 3,
          }];
          let affected = find_affected_symbols(&hunks, &symbols);
          assert!(affected.is_empty());
      }

      #[test]
      fn different_file_no_match() {
          let symbols = vec![sym("foo", "src/a.rs", 10, 20)];
          let hunks = vec![DiffHunk {
              file: "src/b.rs".into(),
              old_start: 15, old_count: 3, new_start: 15, new_count: 3,
          }];
          let affected = find_affected_symbols(&hunks, &symbols);
          assert!(affected.is_empty());
      }

      #[test]
      fn pure_deletion_hunk_matches_symbol() {
          let symbols = vec![sym("foo", "src/a.rs", 10, 20)];
          let hunks = vec![DiffHunk {
              file: "src/a.rs".into(),
              old_start: 12, old_count: 3, new_start: 12, new_count: 0,
          }];
          let affected = find_affected_symbols(&hunks, &symbols);
          assert_eq!(affected.len(), 1);
      }
  }
  ```
- [ ] Step 2: Run `cargo test -p domain -- change_detection`, verify FAIL
- [ ] Step 3: Implement `find_affected_symbols` — for each hunk, check file match, then line range overlap using post-diff lines (or old lines for deletions)
- [ ] Step 4: Run `cargo test -p domain -- change_detection`, verify PASS
- [ ] Step 5: `git add crates/domain/src/analysis/ && git commit -m "feat(S01/T05): change detection — DiffHunk to affected symbols"`

### T06: Blast radius + impact analysis
**Files:** Create `crates/domain/src/analysis/blast_radius.rs`, `crates/domain/src/analysis/impact.rs`
**Traces to:** AC12

- [ ] Step 1: Write failing tests
  ```rust
  // blast_radius.rs tests
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::model::*;
      use crate::traversal::InMemoryGraph;

      #[test]
      fn blast_radius_from_single_symbol() {
          let edges = vec![
              Edge { kind: EdgeKind::Calls, source: "a::foo".into(), target: "b::bar".into(), metadata: None },
              Edge { kind: EdgeKind::Calls, source: "b::bar".into(), target: "c::baz".into(), metadata: None },
          ];
          let graph = InMemoryGraph::from_edges(edges);
          let targets = vec![ImpactTarget::Symbol("a::foo".into())];
          let report = compute_blast_radius(&graph, &targets, 3, Confidence::Structural);
          assert!(!report.affected.is_empty());
          assert!(report.affected.iter().any(|n| n.qualified_name == "b::bar"));
          assert!(report.affected.iter().any(|n| n.qualified_name == "c::baz"));
      }

      #[test]
      fn blast_radius_from_file_target() {
          // AC14: ImpactTarget::File variant is exercised
          let edges = vec![
              Edge { kind: EdgeKind::Contains, source: "a.rs".into(), target: "a.rs::foo".into(), metadata: None },
              Edge { kind: EdgeKind::Calls, source: "a.rs::foo".into(), target: "b.rs::bar".into(), metadata: None },
          ];
          let graph = InMemoryGraph::from_edges(edges);
          let targets = vec![ImpactTarget::File("a.rs".into())];
          let report = compute_blast_radius(&graph, &targets, 3, Confidence::Structural);
          // File target should expand to its contained symbols and trace from there
          assert!(!report.targets.is_empty());
      }
  }

  // impact.rs tests
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::model::*;
      use crate::traversal::InMemoryGraph;

      #[test]
      fn diff_impact_non_overlapping_returns_empty() {
          let graph = InMemoryGraph::from_edges(vec![]);
          let symbols = vec![];
          let hunks = vec![DiffHunk {
              file: "src/a.rs".into(),
              old_start: 1, old_count: 1, new_start: 1, new_count: 1,
          }];
          let report = compute_diff_impact(&graph, &hunks, &symbols, 3);
          assert!(report.changed_symbols.is_empty());
          assert!(report.impact.affected.is_empty());
      }

      #[test]
      fn diff_impact_overlapping_hunk_produces_full_report() {
          // Symbol foo at lines 10-20, hunk touches lines 15-17, foo calls bar
          let symbols = vec![SymbolNode {
              name: "foo".into(), qualified_name: "a.rs::foo".into(),
              kind: SymbolKind::Function,
              location: Location { file: "a.rs".into(), line_start: 10, line_end: 20, col_start: 0, col_end: 0 },
              visibility: Visibility::Public, is_exported: false, is_async: false, is_test: false,
              decorators: vec![], signature: None,
          }];
          let edges = vec![
              Edge { kind: EdgeKind::Calls, source: "a.rs::foo".into(), target: "b.rs::bar".into(), metadata: None },
          ];
          let graph = InMemoryGraph::from_edges(edges);
          let hunks = vec![DiffHunk {
              file: "a.rs".into(), old_start: 15, old_count: 3, new_start: 15, new_count: 3,
          }];
          let report = compute_diff_impact(&graph, &hunks, &symbols, 3);
          assert_eq!(report.changed_symbols.len(), 1);
          assert_eq!(report.changed_symbols[0].name, "foo");
          assert!(report.impact.affected.iter().any(|n| n.qualified_name == "b.rs::bar"));
      }
  }
  ```
- [ ] Step 2: Run `cargo test -p domain -- blast_radius impact`, verify FAIL
- [ ] Step 3: Implement `compute_blast_radius` (uses InMemoryGraph::bfs_filtered) and `compute_diff_impact` (combines change_detection + blast_radius)
- [ ] Step 4: Run `cargo test -p domain -- blast_radius impact`, verify PASS
- [ ] Step 5: `git add crates/domain/src/analysis/ && git commit -m "feat(S01/T06): blast radius + diff impact analysis"`

---

## Wave 3 — Use Cases + Integration (depends on Wave 2)

### T07: Test support mock + use case structs
**Files:** Create `crates/domain/src/test_support.rs`, `crates/domain/src/use_cases/mod.rs`, `crates/domain/src/use_cases/index.rs`, `crates/domain/src/use_cases/query.rs`, `crates/domain/src/use_cases/impact.rs`
**Traces to:** AC16, AC17, AC21

- [ ] Step 1: Write failing tests
  ```rust
  // use_cases/impact.rs tests
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::test_support::InMemoryGraphStore;
      use crate::model::*;

      #[test]
      fn blast_radius_with_mock_returns_transitive_closure() {
          let mut store = InMemoryGraphStore::new();
          store.insert_file(FileNode { path: "a.rs".into(), language: Language::Rust, hash: "h1".into() });
          store.insert_file(FileNode { path: "b.rs".into(), language: Language::Rust, hash: "h2".into() });
          store.insert_symbol(SymbolNode {
              name: "foo".into(), qualified_name: "a.rs::foo".into(),
              kind: SymbolKind::Function,
              location: Location { file: "a.rs".into(), line_start: 1, line_end: 10, col_start: 0, col_end: 0 },
              visibility: Visibility::Public,
              is_exported: true, is_async: false, is_test: false,
              decorators: vec![], signature: None,
          });
          store.insert_symbol(SymbolNode {
              name: "bar".into(), qualified_name: "b.rs::bar".into(),
              kind: SymbolKind::Function,
              location: Location { file: "b.rs".into(), line_start: 1, line_end: 10, col_start: 0, col_end: 0 },
              visibility: Visibility::Public,
              is_exported: true, is_async: false, is_test: false,
              decorators: vec![], signature: None,
          });
          store.insert_edge(Edge {
              kind: EdgeKind::Calls, source: "a.rs::foo".into(),
              target: "b.rs::bar".into(), metadata: None,
          });

          let uc = ImpactUseCase::new(store);
          let report = uc.blast_radius(
              &[ImpactTarget::Symbol("a.rs::foo".into())], 3, Confidence::Structural,
          ).unwrap();
          assert!(report.affected.iter().any(|n| n.qualified_name == "b.rs::bar"));
      }

      #[test]
      fn diff_impact_non_overlapping_returns_empty_report() {
          let store = InMemoryGraphStore::new();
          let uc = ImpactUseCase::new(store);
          let hunks = vec![DiffHunk {
              file: "nonexistent.rs".into(),
              old_start: 1, old_count: 1, new_start: 1, new_count: 1,
          }];
          let report = uc.diff_impact(&hunks, 3).unwrap();
          assert!(report.changed_symbols.is_empty());
      }
  }
  ```
- [ ] Step 2: Run `cargo test -p domain -- use_cases`, verify FAIL
- [ ] Step 3: Implement `InMemoryGraphStore` (implements GraphStore + SearchIndex), then use-case structs:
  - `ImpactUseCase::blast_radius` — loads edges via GraphStore, builds InMemoryGraph, calls compute_blast_radius
  - `ImpactUseCase::diff_impact` — loads symbols + edges, runs find_affected_symbols, then blast_radius
  - `IndexUseCase` — structural shell (body needs parser/fs adapters)
  - `QueryUseCase` — structural shell with delegating methods
- [ ] Step 4: Run `cargo test -p domain -- use_cases`, verify PASS
- [ ] Step 5: `git add crates/domain/src/use_cases/ crates/domain/src/test_support.rs && git commit -m "feat(S01/T07): use case structs + InMemoryGraphStore mock"`

### T08: Final integration + lib.rs wiring
**Files:** Modify `crates/domain/src/lib.rs`
**Traces to:** AC1, AC11

- [ ] Step 1: Update `lib.rs` to export all modules:
  ```rust
  pub mod error;
  pub mod model;
  pub mod ports;
  pub mod traversal;
  pub mod use_cases;
  pub mod analysis;

  #[cfg(test)]
  pub mod test_support;

  pub use error::{CodeGraphError, Result};
  ```
- [ ] Step 2: Run `cargo build -p domain`, verify PASS (AC1)
- [ ] Step 3: Run `cargo test -p domain`, verify ALL tests PASS (AC11)
- [ ] Step 4: Verify `crates/domain/Cargo.toml` has only serde + thiserror (AC10)
- [ ] Step 5: `git add crates/domain/src/lib.rs && git commit -m "feat(S01/T08): wire all modules in lib.rs, verify full test suite"`
