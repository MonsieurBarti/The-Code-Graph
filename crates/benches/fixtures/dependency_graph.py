from __future__ import annotations

import ast
import sys
from collections import defaultdict, deque
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterator, Optional


@dataclass
class Module:
    name: str
    path: Path
    imports: list[str] = field(default_factory=list)


class DependencyGraph:
    def __init__(self) -> None:
        self._nodes: dict[str, Module] = {}
        self._edges: dict[str, set[str]] = defaultdict(set)
        self._reverse: dict[str, set[str]] = defaultdict(set)

    def add_module(self, module: Module) -> None:
        self._nodes[module.name] = module
        for imp in module.imports:
            self._edges[module.name].add(imp)
            self._reverse[imp].add(module.name)

    def dependencies(self, name: str) -> set[str]:
        return self._edges.get(name, set())

    def dependents(self, name: str) -> set[str]:
        return self._reverse.get(name, set())

    def transitive_deps(self, name: str) -> set[str]:
        visited: set[str] = set()
        queue = deque([name])
        while queue:
            current = queue.popleft()
            for dep in self._edges.get(current, set()):
                if dep not in visited:
                    visited.add(dep)
                    queue.append(dep)
        return visited

    def impact_set(self, name: str) -> set[str]:
        visited: set[str] = set()
        queue = deque([name])
        while queue:
            current = queue.popleft()
            for dep in self._reverse.get(current, set()):
                if dep not in visited:
                    visited.add(dep)
                    queue.append(dep)
        return visited

    def cycles(self) -> list[list[str]]:
        visited: set[str] = set()
        rec_stack: set[str] = set()
        found: list[list[str]] = []

        def dfs(node: str, path: list[str]) -> None:
            visited.add(node)
            rec_stack.add(node)
            for neighbor in self._edges.get(node, set()):
                if neighbor not in visited:
                    dfs(neighbor, path + [neighbor])
                elif neighbor in rec_stack:
                    found.append(path + [neighbor])
            rec_stack.discard(node)

        for node in list(self._nodes):
            if node not in visited:
                dfs(node, [node])
        return found
