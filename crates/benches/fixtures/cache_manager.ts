import { LRUCache } from 'lru-cache';
import { createHash } from 'crypto';
import { EventEmitter } from 'events';

export interface CacheOptions {
  maxSize: number;
  ttlMs?: number;
}

export interface CacheStats {
  hits: number;
  misses: number;
  evictions: number;
  size: number;
}

export class CacheManager<V> extends EventEmitter {
  private lru: LRUCache<string, V>;
  private hits = 0;
  private misses = 0;
  private evictions = 0;

  constructor(opts: CacheOptions) {
    super();
    this.lru = new LRUCache({
      max: opts.maxSize,
      ttl: opts.ttlMs,
      dispose: () => { this.evictions++; },
    });
  }

  get(key: string): V | undefined {
    const val = this.lru.get(key);
    if (val !== undefined) { this.hits++; return val; }
    this.misses++;
    return undefined;
  }

  set(key: string, value: V): void {
    this.lru.set(key, value);
    this.emit('set', key);
  }

  delete(key: string): boolean {
    return this.lru.delete(key);
  }

  has(key: string): boolean {
    return this.lru.has(key);
  }

  hashKey(...parts: string[]): string {
    return createHash('md5').update(parts.join('|')).digest('hex');
  }

  stats(): CacheStats {
    return { hits: this.hits, misses: this.misses, evictions: this.evictions, size: this.lru.size };
  }

  clear(): void {
    this.lru.clear();
    this.hits = 0; this.misses = 0; this.evictions = 0;
  }
}
