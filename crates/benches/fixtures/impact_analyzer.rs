use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FileNode {
    pub path: PathBuf,
    pub symbols: Vec<String>,
    pub imports: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ImpactReport {
    pub changed: PathBuf,
    pub direct: Vec<PathBuf>,
    pub transitive: Vec<PathBuf>,
}

impl ImpactReport {
    pub fn total_affected(&self) -> usize {
        let mut seen = HashSet::new();
        for p in self.direct.iter().chain(self.transitive.iter()) {
            seen.insert(p.clone());
        }
        seen.len()
    }
}

pub struct ImpactAnalyzer {
    nodes: HashMap<PathBuf, FileNode>,
    reverse: HashMap<PathBuf, HashSet<PathBuf>>,
}

impl ImpactAnalyzer {
    pub fn new() -> Self {
        Self { nodes: HashMap::new(), reverse: HashMap::new() }
    }

    pub fn register(&mut self, node: FileNode) {
        for import in &node.imports {
            self.reverse.entry(import.clone()).or_default().insert(node.path.clone());
        }
        self.nodes.insert(node.path.clone(), node);
    }

    pub fn compute_impact(&self, path: &PathBuf) -> ImpactReport {
        let direct: Vec<PathBuf> = self.reverse.get(path).map(|s| s.iter().cloned().collect()).unwrap_or_default();
        let mut transitive = Vec::new();
        let mut visited: HashSet<PathBuf> = direct.iter().cloned().collect();
        visited.insert(path.clone());
        let mut queue: VecDeque<PathBuf> = direct.iter().cloned().collect();
        while let Some(cur) = queue.pop_front() {
            if let Some(parents) = self.reverse.get(&cur) {
                for p in parents {
                    if visited.insert(p.clone()) {
                        transitive.push(p.clone());
                        queue.push_back(p.clone());
                    }
                }
            }
        }
        ImpactReport { changed: path.clone(), direct, transitive }
    }

    pub fn most_impactful(&self, top_n: usize) -> Vec<(PathBuf, usize)> {
        let mut scores: Vec<(PathBuf, usize)> = self.nodes.keys()
            .map(|p| (p.clone(), self.reverse.get(p).map(|s| s.len()).unwrap_or(0)))
            .collect();
        scores.sort_by(|a, b| b.1.cmp(&a.1));
        scores.truncate(top_n);
        scores
    }
}
