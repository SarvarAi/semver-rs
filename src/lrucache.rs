//! Port of `internal/lrucache.js`.
//!
//! Upstream leans on JavaScript `Map`'s insertion-order iteration: `get` deletes
//! and re-inserts to move a key to the most-recent end, and eviction pops the
//! first key. std has no insertion-ordered map, so recency is tracked with a
//! monotonic tick and a `BTreeMap` from tick to key — `get`, `set` and eviction
//! are all O(log n), matching the O(1)-ish behaviour upstream gets from `Map`.
//!
//! An earlier version scanned a `VecDeque` to find the key on every `get`, which
//! is O(n) with n up to 1000. That showed up plainly in the `range_parse`
//! benchmark as the port losing to Node, and is why the benchmark exists.

use std::collections::{BTreeMap, HashMap};

pub struct LruCache<V> {
    max: usize,
    map: HashMap<String, (V, u64)>,
    /// recency tick -> key, so the least-recently-used entry is `first`.
    order: BTreeMap<u64, String>,
    tick: u64,
}

impl<V: Clone> LruCache<V> {
    pub fn new() -> Self {
        Self { max: 1000, map: HashMap::new(), order: BTreeMap::new(), tick: 0 }
    }

    pub fn get(&mut self, key: &str) -> Option<V> {
        let (value, old_tick) = self.map.get(key)?;
        let value = value.clone();
        let old_tick = *old_tick;

        self.tick += 1;
        let new_tick = self.tick;
        self.order.remove(&old_tick);
        self.order.insert(new_tick, key.to_string());
        if let Some(slot) = self.map.get_mut(key) {
            slot.1 = new_tick;
        }
        Some(value)
    }

    pub fn set(&mut self, key: String, value: V) {
        // Faithful to upstream: `set` deletes first, and only re-inserts when
        // the key was absent. Setting an existing key therefore removes it.
        if let Some((_, old_tick)) = self.map.remove(&key) {
            self.order.remove(&old_tick);
            return;
        }

        if self.map.len() >= self.max {
            if let Some((&oldest_tick, oldest_key)) = self.order.iter().next() {
                let oldest_key = oldest_key.clone();
                self.order.remove(&oldest_tick);
                self.map.remove(&oldest_key);
            }
        }

        self.tick += 1;
        self.order.insert(self.tick, key.clone());
        self.map.insert(key, (value, self.tick));
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

    #[test]
    fn stays_within_its_bound() {
        let mut c: LruCache<u32> = LruCache::new();
        for i in 0..5000u32 {
            c.set(format!("k{i}"), i);
        }
        assert_eq!(c.map.len(), 1000);
        assert_eq!(c.order.len(), 1000);
    }
}
