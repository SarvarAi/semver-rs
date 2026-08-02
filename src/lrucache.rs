//! Port of `internal/lrucache.js`.
//!
//! Upstream leans on JavaScript `Map`'s insertion-order iteration: `get` deletes
//! and re-inserts to move a key to the end, and eviction pops the first key.
//! `IndexMap`-style ordering is not in std, so this keeps a `HashMap` for lookup
//! alongside a `VecDeque` recording recency.
//!
//! This exists because `Range::parse_range` is a hot, fully deterministic path
//! that upstream memoizes; dropping the cache would change the benchmark story
//! without changing behaviour.

use std::collections::{HashMap, VecDeque};

pub struct LruCache<V> {
    max: usize,
    map: HashMap<String, V>,
    order: VecDeque<String>,
}

impl<V: Clone> LruCache<V> {
    pub fn new() -> Self {
        Self { max: 1000, map: HashMap::new(), order: VecDeque::new() }
    }

    pub fn get(&mut self, key: &str) -> Option<V> {
        let value = self.map.get(key)?.clone();
        // Move to the most-recently-used end.
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key.to_string());
        Some(value)
    }

    pub fn set(&mut self, key: String, value: V) {
        let existed = self.map.remove(&key).is_some();
        if existed {
            if let Some(pos) = self.order.iter().position(|k| *k == key) {
                self.order.remove(pos);
            }
            // Upstream returns early on an existing key: `set` on a key that was
            // already present deletes it and does NOT re-insert.
            return;
        }
        if self.map.len() >= self.max {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }
}

impl<V: Clone> Default for LruCache<V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used() {
        let mut c: LruCache<u32> = LruCache::new();
        for i in 0..1000u32 {
            c.set(format!("k{i}"), i);
        }
        // Touch k0 so it is no longer the oldest.
        assert_eq!(c.get("k0"), Some(0));
        c.set("k1000".into(), 1000);
        assert_eq!(c.get("k0"), Some(0), "recently used key survived");
        assert_eq!(c.get("k1"), None, "true LRU victim was evicted");
    }

    #[test]
    fn re_setting_an_existing_key_deletes_it() {
        // Faithful to upstream: `set` deletes first and only re-inserts when the
        // key was absent, so setting an existing key removes it.
        let mut c: LruCache<u32> = LruCache::new();
        c.set("a".into(), 1);
        c.set("a".into(), 2);
        assert_eq!(c.get("a"), None);
    }
}
