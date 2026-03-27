use std::collections::{HashMap, HashSet, VecDeque};

use rusqlite::{Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub file_path: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: String,
}

pub struct GraphStore {
    conn: Connection,
    adjacency: HashMap<String, HashSet<String>>,
    reverse: HashMap<String, HashSet<String>>,
}

impl GraphStore {
    pub fn open(path: &str) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS nodes (id TEXT PRIMARY KEY, kind TEXT, label TEXT, file_path TEXT, line INTEGER);
            CREATE TABLE IF NOT EXISTS edges (source TEXT, target TEXT, kind TEXT, PRIMARY KEY (source, target, kind));
        ")?;
        Ok(Self { conn, adjacency: HashMap::new(), reverse: HashMap::new() })
    }

    pub fn insert_node(&mut self, node: &Node) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO nodes VALUES (?1, ?2, ?3, ?4, ?5)",
            (&node.id, &node.kind, &node.label, &node.file_path, node.line),
        )?;
        Ok(())
    }

    pub fn insert_edge(&mut self, edge: &Edge) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO edges VALUES (?1, ?2, ?3)",
            (&edge.source, &edge.target, &edge.kind),
        )?;
        self.adjacency.entry(edge.source.clone()).or_default().insert(edge.target.clone());
        self.reverse.entry(edge.target.clone()).or_default().insert(edge.source.clone());
        Ok(())
    }

    pub fn get_node(&self, id: &str) -> SqlResult<Option<Node>> {
        let mut stmt = self.conn.prepare("SELECT id, kind, label, file_path, line FROM nodes WHERE id = ?1")?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(Node { id: row.get(0)?, kind: row.get(1)?, label: row.get(2)?, file_path: row.get(3)?, line: row.get(4)? }));
        }
        Ok(None)
    }

    pub fn successors(&self, id: &str) -> Vec<String> {
        self.adjacency.get(id).map(|s| s.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn impact_set(&self, id: &str) -> HashSet<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(id.to_string());
        while let Some(cur) = queue.pop_front() {
            if let Some(parents) = self.reverse.get(&cur) {
                for p in parents {
                    if visited.insert(p.clone()) { queue.push_back(p.clone()); }
                }
            }
        }
        visited
    }
}
