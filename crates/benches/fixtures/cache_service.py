import asyncio
import hashlib
import json
import time
from collections import OrderedDict
from dataclasses import dataclass
from typing import Any, Optional

import redis.asyncio as aioredis


@dataclass
class CacheEntry:
    value: Any
    created_at: float
    ttl: Optional[float]

    def is_expired(self) -> bool:
        if self.ttl is None:
            return False
        return time.monotonic() - self.created_at > self.ttl


class LocalLRUCache:
    def __init__(self, max_size: int = 512) -> None:
        self._store: OrderedDict[str, CacheEntry] = OrderedDict()
        self.max_size = max_size

    def get(self, key: str) -> Optional[Any]:
        if key not in self._store:
            return None
        entry = self._store[key]
        if entry.is_expired():
            del self._store[key]
            return None
        self._store.move_to_end(key)
        return entry.value

    def set(self, key: str, value: Any, ttl: Optional[float] = None) -> None:
        if key in self._store:
            self._store.move_to_end(key)
        self._store[key] = CacheEntry(value=value, created_at=time.monotonic(), ttl=ttl)
        if len(self._store) > self.max_size:
            self._store.popitem(last=False)

    def delete(self, key: str) -> bool:
        return self._store.pop(key, None) is not None

    def clear(self) -> None:
        self._store.clear()


class CacheService:
    def __init__(self, redis_url: str) -> None:
        self._local = LocalLRUCache(max_size=256)
        self._redis: Optional[aioredis.Redis] = None
        self._redis_url = redis_url

    async def connect(self) -> None:
        self._redis = await aioredis.from_url(self._redis_url, decode_responses=True)

    async def get(self, key: str) -> Optional[Any]:
        local = self._local.get(key)
        if local is not None:
            return local
        if self._redis:
            raw = await self._redis.get(key)
            if raw:
                val = json.loads(raw)
                self._local.set(key, val)
                return val
        return None

    async def set(self, key: str, value: Any, ttl_seconds: Optional[int] = None) -> None:
        self._local.set(key, value, ttl=float(ttl_seconds) if ttl_seconds else None)
        if self._redis:
            raw = json.dumps(value)
            if ttl_seconds:
                await self._redis.setex(key, ttl_seconds, raw)
            else:
                await self._redis.set(key, raw)

    @staticmethod
    def make_key(*parts: str) -> str:
        return hashlib.sha1(":".join(parts).encode()).hexdigest()
