use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

#[derive(Debug)]
struct CacheEntry<V> {
    value: V,
    inserted_at: Instant,
    ttl: Option<Duration>,
}

impl<V> CacheEntry<V> {
    fn is_expired(&self) -> bool {
        self.ttl.map(|ttl| self.inserted_at.elapsed() > ttl).unwrap_or(false)
    }
}

#[derive(Debug)]
pub struct CacheStore<K, V> {
    inner: RwLock<HashMap<K, CacheEntry<V>>>,
    max_size: usize,
}

impl<K: Eq + Hash + Clone, V: Clone> CacheStore<K, V> {
    pub fn new(max_size: usize) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            max_size,
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let guard = self.inner.read();
        let entry = guard.get(key)?;
        if entry.is_expired() { return None; }
        Some(entry.value.clone())
    }

    pub fn insert(&self, key: K, value: V, ttl: Option<Duration>) {
        let mut guard = self.inner.write();
        if guard.len() >= self.max_size {
            let oldest = guard.iter()
                .min_by_key(|(_, e)| e.inserted_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest { guard.remove(&k); }
        }
        guard.insert(key, CacheEntry { value, inserted_at: Instant::now(), ttl });
    }

    pub fn remove(&self, key: &K) -> bool {
        self.inner.write().remove(key).is_some()
    }

    pub fn evict_expired(&self) -> usize {
        let mut guard = self.inner.write();
        let before = guard.len();
        guard.retain(|_, v| !v.is_expired());
        before - guard.len()
    }

    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }
}
