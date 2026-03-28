from __future__ import annotations

import asyncio
import logging
import signal
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional

from watchfiles import awatch, Change

logger = logging.getLogger(__name__)


@dataclass
class WatchEvent:
    kind: str
    path: str


ChangeCallback = Callable[[list[WatchEvent]], None]


class WatcherDaemon:
    def __init__(self, dirs: list[Path], debounce_ms: int = 200) -> None:
        self.dirs = dirs
        self.debounce_ms = debounce_ms
        self._callbacks: list[ChangeCallback] = []
        self._task: Optional[asyncio.Task] = None
        self._stop_event = asyncio.Event()

    def on_change(self, callback: ChangeCallback) -> None:
        self._callbacks.append(callback)

    def _map_change(self, c: Change) -> str:
        return {Change.added: "add", Change.modified: "change", Change.deleted: "unlink"}.get(c, "unknown")

    async def _watch_loop(self) -> None:
        paths = [str(d) for d in self.dirs]
        async for changes in awatch(*paths, stop_event=self._stop_event):
            events = [WatchEvent(kind=self._map_change(c), path=p) for c, p in changes]
            for cb in self._callbacks:
                try:
                    cb(events)
                except Exception as exc:
                    logger.error("Callback error: %s", exc)

    async def start(self) -> None:
        self._task = asyncio.create_task(self._watch_loop())
        logger.info("WatcherDaemon started on %s", self.dirs)

    async def stop(self) -> None:
        self._stop_event.set()
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
        logger.info("WatcherDaemon stopped")

    def register_signals(self) -> None:
        loop = asyncio.get_event_loop()
        for sig in (signal.SIGINT, signal.SIGTERM):
            loop.add_signal_handler(sig, lambda: asyncio.create_task(self.stop()))
