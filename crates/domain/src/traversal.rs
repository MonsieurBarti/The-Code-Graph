use std::collections::{HashMap, HashSet, VecDeque};
use crate::model::*;

// ---------------------------------------------------------------------------
// InMemoryGraph
// ---------------------------------------------------------------------------

pub struct InMemoryGraph {
    outgoing: HashMap<String, Vec<(String, EdgeKind)>>,
    incoming: HashMap<String, Vec<(String, EdgeKind)>>,
}

impl InMemoryGraph {
    pub fn from_edges(edges: Vec<Edge>) -> Self {
        let mut outgoing: HashMap<String, Vec<(String, EdgeKind)>> = HashMap::new();
        let mut incoming: HashMap<String, Vec<(String, EdgeKind)>> = HashMap::new();

        for edge in edges {
            outgoing
                .entry(edge.source.clone())
                .or_default()
                .push((edge.target.clone(), edge.kind));
            incoming
                .entry(edge.target.clone())
                .or_default()
                .push((edge.source.clone(), edge.kind));
        }

        InMemoryGraph { outgoing, incoming }
    }

    pub fn bfs(&self, start: &str, direction: Direction, max_depth: usize) -> Vec<TraversalResult> {
        self.bfs_inner(start, direction, max_depth, |_| true)
    }

    pub fn bfs_filtered(
        &self,
        start: &str,
        direction: Direction,
        max_depth: usize,
        min_confidence: Confidence,
    ) -> Vec<TraversalResult> {
        self.bfs_inner(start, direction, max_depth, move |kind| {
            kind.confidence() >= min_confidence
        })
    }

    fn bfs_inner<F>(
        &self,
        start: &str,
        direction: Direction,
        max_depth: usize,
        edge_filter: F,
    ) -> Vec<TraversalResult>
    where
        F: Fn(&EdgeKind) -> bool,
    {
        let adjacency = match direction {
            Direction::Forward => &self.outgoing,
            Direction::Backward => &self.incoming,
        };

        let mut results = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        // Queue entries: (node, depth, path)
        let mut queue: VecDeque<(String, usize, Vec<String>, EdgeKind)> = VecDeque::new();

        visited.insert(start.to_string());

        if let Some(neighbors) = adjacency.get(start) {
            for (neighbor, kind) in neighbors {
                if edge_filter(kind) && !visited.contains(neighbor.as_str()) {
                    queue.push_back((neighbor.clone(), 1, vec![start.to_string()], *kind));
                }
            }
        }

        while let Some((node, depth, path, edge_kind)) = queue.pop_front() {
            if visited.contains(&node) {
                continue;
            }
            visited.insert(node.clone());

            let mut new_path = path.clone();
            new_path.push(node.clone());

            results.push(TraversalResult {
                node: node.clone(),
                depth,
                path: new_path.clone(),
                edge_kind,
            });

            if depth < max_depth {
                if let Some(neighbors) = adjacency.get(&node) {
                    for (neighbor, kind) in neighbors {
                        if edge_filter(kind) && !visited.contains(neighbor.as_str()) {
                            queue.push_back((
                                neighbor.clone(),
                                depth + 1,
                                new_path.clone(),
                                *kind,
                            ));
                        }
                    }
                }
            }
        }

        results
    }

    pub fn dfs(&self, start: &str, direction: Direction) -> Vec<TraversalResult> {
        let adjacency = match direction {
            Direction::Forward => &self.outgoing,
            Direction::Backward => &self.incoming,
        };

        let mut results = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        // Stack entries: (node, depth, path, edge_kind)
        let mut stack: Vec<(String, usize, Vec<String>, EdgeKind)> = Vec::new();

        visited.insert(start.to_string());

        if let Some(neighbors) = adjacency.get(start) {
            for (neighbor, kind) in neighbors.iter().rev() {
                if !visited.contains(neighbor.as_str()) {
                    stack.push((neighbor.clone(), 1, vec![start.to_string()], *kind));
                }
            }
        }

        while let Some((node, depth, path, edge_kind)) = stack.pop() {
            if visited.contains(&node) {
                continue;
            }
            visited.insert(node.clone());

            let mut new_path = path.clone();
            new_path.push(node.clone());

            results.push(TraversalResult {
                node: node.clone(),
                depth,
                path: new_path.clone(),
                edge_kind,
            });

            if let Some(neighbors) = adjacency.get(&node) {
                for (neighbor, kind) in neighbors.iter().rev() {
                    if !visited.contains(neighbor.as_str()) {
                        stack.push((neighbor.clone(), depth + 1, new_path.clone(), *kind));
                    }
                }
            }
        }

        results
    }
}

// ---------------------------------------------------------------------------

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
            make_edge("A", "B", EdgeKind::Calls),       // High
            make_edge("A", "C", EdgeKind::DependsOn),    // Low
            make_edge("B", "D", EdgeKind::ImportsFrom),  // Medium
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
