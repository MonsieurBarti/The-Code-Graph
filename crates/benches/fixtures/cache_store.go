package cache

import (
	"sync"
	"time"
)

type entry struct {
	value     interface{}
	expiresAt time.Time
	hasExpiry bool
}

func (e *entry) expired() bool {
	return e.hasExpiry && time.Now().After(e.expiresAt)
}

// CacheStore is a generic in-memory LRU-like cache with optional TTL.
type CacheStore struct {
	mu      sync.RWMutex
	data    map[string]*entry
	maxSize int
	hits    int64
	misses  int64
}

// NewCacheStore creates a cache with the given max size.
func NewCacheStore(maxSize int) *CacheStore {
	return &CacheStore{data: make(map[string]*entry, maxSize), maxSize: maxSize}
}

// Get retrieves a value, returning (value, true) on hit.
func (c *CacheStore) Get(key string) (interface{}, bool) {
	c.mu.RLock()
	e, ok := c.data[key]
	c.mu.RUnlock()
	if !ok || e.expired() {
		c.mu.Lock()
		delete(c.data, key)
		c.mu.Unlock()
		c.misses++
		return nil, false
	}
	c.hits++
	return e.value, true
}

// Set stores a value with an optional TTL (0 = no expiry).
func (c *CacheStore) Set(key string, value interface{}, ttl time.Duration) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if len(c.data) >= c.maxSize {
		for k := range c.data { delete(c.data, k); break }
	}
	e := &entry{value: value}
	if ttl > 0 {
		e.expiresAt = time.Now().Add(ttl)
		e.hasExpiry = true
	}
	c.data[key] = e
}

// Delete removes a key.
func (c *CacheStore) Delete(key string) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	_, ok := c.data[key]
	delete(c.data, key)
	return ok
}

// Stats returns cache hit/miss counts.
func (c *CacheStore) Stats() (hits, misses int64) { return c.hits, c.misses }

// Len returns the number of entries.
func (c *CacheStore) Len() int { c.mu.RLock(); defer c.mu.RUnlock(); return len(c.data) }
