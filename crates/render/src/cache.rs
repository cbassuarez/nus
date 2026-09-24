//! Bounded second-chance caches: evict cold entries without flushing hot text.
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
struct Entry<V> {
    value: V,
    weight: usize,
    hot: bool,
}
pub(crate) struct Cache<K, V> {
    map: HashMap<K, Entry<V>>,
    clock: VecDeque<K>,
    weight: usize,
    entries: usize,
    budget: usize,
}
impl<K: Eq + Hash + Clone, V> Cache<K, V> {
    pub fn new(entries: usize, budget: usize) -> Self {
        Self {
            map: HashMap::new(),
            clock: VecDeque::new(),
            weight: 0,
            entries,
            budget,
        }
    }
    pub fn clear(&mut self) {
        self.map = HashMap::new();
        self.clock = VecDeque::new();
        self.weight = 0;
    }
    pub fn get(&mut self, key: &K) -> Option<&V> {
        let e = self.map.get_mut(key)?;
        e.hot = true;
        Some(&e.value)
    }
    pub fn insert(&mut self, key: K, value: V, weight: usize) {
        if weight > self.budget || self.entries == 0 {
            return;
        }
        if let Some(e) = self.map.remove(&key) {
            self.weight -= e.weight;
            self.clock.retain(|k| k != &key);
        }
        while self.map.len() >= self.entries || self.weight.saturating_add(weight) > self.budget {
            let Some(k) = self.clock.pop_front() else {
                break;
            };
            if let Some(e) = self.map.get_mut(&k) {
                if e.hot {
                    e.hot = false;
                    self.clock.push_back(k);
                } else {
                    self.weight -= self.map.remove(&k).unwrap().weight;
                }
            }
        }
        self.weight += weight;
        self.clock.push_back(key.clone());
        self.map.insert(
            key,
            Entry {
                value,
                weight,
                hot: false,
            },
        );
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
    #[cfg(test)]
    pub fn weight(&self) -> usize {
        self.weight
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hot_entries_survive_pressure_without_exceeding_budget() {
        let mut c = Cache::new(3, 6);
        c.insert(1, "a", 2);
        c.insert(2, "b", 2);
        c.insert(3, "c", 2);
        assert_eq!(c.get(&1), Some(&"a"));
        c.insert(4, "d", 2);
        assert_eq!(c.get(&1), Some(&"a"));
        assert_eq!(c.get(&2), None);
        assert_eq!(c.len(), 3);
        assert_eq!(c.weight(), 6);
        c.insert(1, "replacement", 6);
        assert_eq!(c.len(), 1);
        assert_eq!(c.weight(), 6);
        c.insert(9, "oversized", 7);
        assert_eq!(c.len(), 1);
    }
}
