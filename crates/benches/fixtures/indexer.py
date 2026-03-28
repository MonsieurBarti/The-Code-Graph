from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass, field
from pathlib import Path
from typing import AsyncIterator, Optional

import aiofiles

logger = logging.getLogger(__name__)


@dataclass
class IndexEntry:
    file_path: str
    symbols: list[str] = field(default_factory=list)
    imports: list[str] = field(default_factory=list)
    checksum: str = ""


@dataclass
class IndexStats:
    files_indexed: int = 0
    symbols_found: int = 0
    errors: int = 0
    duration_ms: float = 0.0


class FileIndexer:
    def __init__(self, root: Path, workers: int = 4) -> None:
        self.root = root
        self.workers = workers
        self._index: dict[str, IndexEntry] = {}
        self._sem = asyncio.Semaphore(workers)

    async def index_file(self, path: Path) -> Optional[IndexEntry]:
        async with self._sem:
            try:
                async with aiofiles.open(path, "r", encoding="utf-8", errors="ignore") as f:
                    content = await f.read()
                entry = IndexEntry(file_path=str(path))
                for line in content.splitlines():
                    stripped = line.strip()
                    if stripped.startswith("import ") or stripped.startswith("from "):
                        entry.imports.append(stripped)
                    elif stripped.startswith("def ") or stripped.startswith("class "):
                        name = stripped.split("(")[0].split(":")[0].split()[-1]
                        entry.symbols.append(name)
                import hashlib
                entry.checksum = hashlib.md5(content.encode()).hexdigest()
                self._index[str(path)] = entry
                return entry
            except Exception as exc:
                logger.warning("Failed to index %s: %s", path, exc)
                return None

    async def index_all(self, ext: str = "py") -> IndexStats:
        import time
        start = time.monotonic()
        paths = list(self.root.rglob(f"*.{ext}"))
        results = await asyncio.gather(*(self.index_file(p) for p in paths))
        stats = IndexStats(
            files_indexed=sum(1 for r in results if r is not None),
            symbols_found=sum(len(r.symbols) for r in results if r is not None),
            errors=sum(1 for r in results if r is None),
            duration_ms=(time.monotonic() - start) * 1000,
        )
        return stats

    def lookup(self, symbol: str) -> list[IndexEntry]:
        return [e for e in self._index.values() if symbol in e.symbols]

    def invalidate(self, path: Path) -> bool:
        return self._index.pop(str(path), None) is not None
