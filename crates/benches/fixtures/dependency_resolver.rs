use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

#[derive(Debug, Clone)]
pub struct Crate {
    pub name: String,
    pub version: String,
    pub manifest_path: PathBuf,
}

pub struct DependencyResolver {
    graph: DiGraph<Crate, ()>,
    name_to_idx: HashMap<String, NodeIndex>,
}

impl DependencyResolver {
    pub fn new() -> Self {
        Self { graph: DiGraph::new(), name_to_idx: HashMap::new() }
    }

    pub fn add_crate(&mut self, krate: Crate) -> NodeIndex {
        if let Some(&idx) = self.name_to_idx.get(&krate.name) { return idx; }
        let idx = self.graph.add_node(krate.clone());
        self.name_to_idx.insert(krate.name, idx);
        idx
    }

    pub fn add_dependency(&mut self, from: &str, to: &str) {
        let from_idx = self.name_to_idx[from];
        let to_idx = self.name_to_idx[to];
        self.graph.add_edge(from_idx, to_idx, ());
    }

    pub fn transitive_deps(&self, name: &str) -> Vec<String> {
        let Some(&start) = self.name_to_idx.get(name) else { return vec![]; };
        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([start]);
        let mut result = Vec::new();
        while let Some(idx) = queue.pop_front() {
            for edge in self.graph.edges(idx) {
                let target = edge.target();
                if visited.insert(target) {
                    result.push(self.graph[target].name.clone());
                    queue.push_back(target);
                }
            }
        }
        result
    }

    pub fn detect_cycles(&self) -> Vec<Vec<String>> {
        petgraph::algo::tarjan_scc(&self.graph)
            .into_iter()
            .filter(|scc| scc.len() > 1)
            .map(|scc| scc.iter().map(|&i| self.graph[i].name.clone()).collect())
            .collect()
    }

    pub fn topological_order(&self) -> Option<Vec<String>> {
        petgraph::algo::toposort(&self.graph, None)
            .ok()
            .map(|order| order.iter().map(|&i| self.graph[i].name.clone()).collect())
    }
}
