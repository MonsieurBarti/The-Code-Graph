from __future__ import annotations

import sqlite3
from contextlib import contextmanager
from dataclasses import dataclass, field
from pathlib import Path
from typing import Generator, Optional

import networkx as nx


@dataclass
class Node:
    id: str
    kind: str
    label: str
    file_path: str
    line: int
    metadata: dict = field(default_factory=dict)


@dataclass
class Edge:
    source: str
    target: str
    kind: str


class GraphStore:
    def __init__(self, db_path: Path) -> None:
        self.db_path = db_path
        self._graph = nx.DiGraph()
        self._conn = sqlite3.connect(str(db_path))
        self._init_schema()

    def _init_schema(self) -> None:
        self._conn.executescript("""
            CREATE TABLE IF NOT EXISTS nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                label TEXT NOT NULL,
                file_path TEXT NOT NULL,
                line INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS edges (
                source TEXT NOT NULL,
                target TEXT NOT NULL,
                kind TEXT NOT NULL,
                PRIMARY KEY (source, target, kind)
            );
        """)
        self._conn.commit()

    @contextmanager
    def transaction(self) -> Generator[None, None, None]:
        try:
            yield
            self._conn.commit()
        except Exception:
            self._conn.rollback()
            raise

    def add_node(self, node: Node) -> None:
        self._graph.add_node(node.id, **node.__dict__)
        self._conn.execute(
            "INSERT OR REPLACE INTO nodes VALUES (?, ?, ?, ?, ?)",
            (node.id, node.kind, node.label, node.file_path, node.line),
        )

    def add_edge(self, edge: Edge) -> None:
        self._graph.add_edge(edge.source, edge.target, kind=edge.kind)
        self._conn.execute(
            "INSERT OR REPLACE INTO edges VALUES (?, ?, ?)",
            (edge.source, edge.target, edge.kind),
        )

    def get_node(self, node_id: str) -> Optional[Node]:
        cur = self._conn.execute("SELECT * FROM nodes WHERE id = ?", (node_id,))
        row = cur.fetchone()
        if row is None:
            return None
        return Node(id=row[0], kind=row[1], label=row[2], file_path=row[3], line=row[4])

    def neighbors(self, node_id: str) -> list[str]:
        return list(self._graph.successors(node_id))

    def shortest_path(self, source: str, target: str) -> list[str]:
        try:
            return nx.shortest_path(self._graph, source, target)
        except nx.NetworkXNoPath:
            return []

    def close(self) -> None:
        self._conn.close()
