from __future__ import annotations

import sqlite3
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Generator, Iterator, Optional


class StorageError(Exception):
    pass


class StorageBackend:
    def __init__(self, path: Path) -> None:
        self._path = path
        self._conn: Optional[sqlite3.Connection] = None

    def connect(self) -> None:
        self._conn = sqlite3.connect(str(self._path), check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        self._conn.execute("PRAGMA journal_mode=WAL")
        self._conn.execute("PRAGMA synchronous=NORMAL")
        self._conn.execute("PRAGMA cache_size=-64000")

    def disconnect(self) -> None:
        if self._conn:
            self._conn.close()
            self._conn = None

    @contextmanager
    def cursor(self) -> Generator[sqlite3.Cursor, None, None]:
        if not self._conn:
            raise StorageError("Not connected")
        cur = self._conn.cursor()
        try:
            yield cur
            self._conn.commit()
        except Exception:
            self._conn.rollback()
            raise
        finally:
            cur.close()

    def execute(self, sql: str, params: tuple = ()) -> list[dict[str, Any]]:
        with self.cursor() as cur:
            cur.execute(sql, params)
            if cur.description:
                return [dict(row) for row in cur.fetchall()]
            return []

    def execute_many(self, sql: str, params_list: list[tuple]) -> int:
        with self.cursor() as cur:
            cur.executemany(sql, params_list)
            return cur.rowcount

    def table_exists(self, name: str) -> bool:
        result = self.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (name,))
        return len(result) > 0

    def __enter__(self) -> "StorageBackend":
        self.connect()
        return self

    def __exit__(self, *args: Any) -> None:
        self.disconnect()
