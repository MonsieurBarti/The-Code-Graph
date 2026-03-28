from __future__ import annotations

from collections import defaultdict, deque
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


@dataclass
class FileNode:
    path: str
    symbols: list[str] = field(default_factory=list)
    imports: list[str] = field(default_factory=list)


@dataclass
class ImpactReport:
    changed_file: str
    direct_dependents: list[str]
    transitive_dependents: list[str]
    total_affected: int

    @property
    def all_affected(self) -> list[str]:
        seen: set[str] = set()
        result = []
        for path in self.direct_dependents + self.transitive_dependents:
            if path not in seen:
                seen.add(path)
                result.append(path)
        return result


class ImpactAnalyzer:
    def __init__(self) -> None:
        self._nodes: dict[str, FileNode] = {}
        self._reverse_deps: dict[str, set[str]] = defaultdict(set)

    def register(self, node: FileNode) -> None:
        self._nodes[node.path] = node
        for imp in node.imports:
            self._reverse_deps[imp].add(node.path)

    def compute_impact(self, changed_path: str) -> ImpactReport:
        direct = list(self._reverse_deps.get(changed_path, set()))
        transitive: list[str] = []
        visited: set[str] = set(direct) | {changed_path}
        queue = deque(direct)
        while queue:
            current = queue.popleft()
            for dep in self._reverse_deps.get(current, set()):
                if dep not in visited:
                    visited.add(dep)
                    transitive.append(dep)
                    queue.append(dep)
        return ImpactReport(
            changed_file=changed_path,
            direct_dependents=direct,
            transitive_dependents=transitive,
            total_affected=len(direct) + len(transitive),
        )

    def batch_impact(self, changed_paths: list[str]) -> dict[str, ImpactReport]:
        return {p: self.compute_impact(p) for p in changed_paths}

    def most_impactful(self, top_n: int = 10) -> list[tuple[str, int]]:
        scores = [(p, len(self._reverse_deps.get(p, set()))) for p in self._nodes]
        scores.sort(key=lambda x: x[1], reverse=True)
        return scores[:top_n]
