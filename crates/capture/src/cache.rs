//! Preview cache: a hard byte budget, evicting the least recently shown.

pub struct Cache<T> {
    budget: usize,
    used: usize,
    /// Most recently shown first.
    entries: Vec<Entry<T>>,
}

struct Entry<T> {
    wid: u32,
    bytes: usize,
    payload: T,
}

impl<T> Cache<T> {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            used: 0,
            entries: Vec::new(),
        }
    }

    /// The cached preview for `wid`, marking it just-shown.
    pub fn shown(&mut self, wid: u32) -> Option<&T> {
        let index = self.entries.iter().position(|e| e.wid == wid)?;
        let entry = self.entries.remove(index);
        self.entries.insert(0, entry);
        self.entries.first().map(|e| &e.payload)
    }

    /// Inserts or refreshes `wid` — a refresh keeps its recency slot — then
    /// evicts least-recently-shown entries until the budget holds again.
    pub fn insert(&mut self, wid: u32, payload: T, bytes: usize) {
        if let Some(index) = self.entries.iter().position(|e| e.wid == wid) {
            // A refresh that cannot fit would evict the very entry it is
            // replacing, leaving a miss where a usable stale preview stood.
            // Keep the old one instead — stale beats absent.
            let older: usize = self.entries[index + 1..].iter().map(|e| e.bytes).sum();
            let without_self = self.used - self.entries[index].bytes;
            if bytes > self.budget || bytes + (without_self - older) > self.budget {
                return;
            }
            let entry = &mut self.entries[index];
            self.used = self.used - entry.bytes + bytes;
            entry.bytes = bytes;
            entry.payload = payload;
        } else {
            self.used += bytes;
            self.entries.insert(
                0,
                Entry {
                    wid,
                    bytes,
                    payload,
                },
            );
        }
        while self.used > self.budget {
            let Some(evicted) = self.entries.pop() else {
                break;
            };
            self.used -= evicted.bytes;
        }
    }

    /// Forgets windows that no longer exist.
    pub fn retain(&mut self, alive: impl Fn(u32) -> bool) {
        self.entries.retain(|e| alive(e.wid));
        self.used = self.entries.iter().map(|e| e.bytes).sum();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wids<T>(cache: &Cache<T>) -> Vec<u32> {
        cache.entries.iter().map(|e| e.wid).collect()
    }

    #[test]
    fn miss_is_none() {
        let mut cache: Cache<()> = Cache::new(10);
        assert!(cache.shown(1).is_none());
    }

    #[test]
    fn shown_marks_recency() {
        let mut cache = Cache::new(10);
        for wid in [1, 2, 3] {
            cache.insert(wid, (), 1);
        }
        assert_eq!(wids(&cache), [3, 2, 1]);
        assert!(cache.shown(1).is_some());
        assert_eq!(wids(&cache), [1, 3, 2]);
    }

    #[test]
    fn evicts_least_recently_shown() {
        let mut cache = Cache::new(3);
        for wid in [1, 2, 3] {
            cache.insert(wid, (), 1);
        }
        cache.shown(1);
        cache.insert(4, (), 1);
        assert_eq!(wids(&cache), [4, 1, 3]);
        assert!(cache.shown(2).is_none());
    }

    #[test]
    fn refresh_keeps_slot_and_rebalances_budget() {
        let mut cache = Cache::new(4);
        cache.insert(1, (), 3);
        cache.insert(2, (), 1);
        cache.insert(1, (), 1);
        assert_eq!(wids(&cache), [2, 1]);
        cache.insert(3, (), 2);
        assert_eq!(wids(&cache), [3, 2, 1]);
    }

    #[test]
    fn budget_is_hard_even_for_a_fresh_entry() {
        let mut cache = Cache::new(1);
        cache.insert(1, (), 2);
        assert!(cache.shown(1).is_none());
    }

    /// Payload identity, not just presence: a cache that returned the wrong
    /// image for a window would satisfy every `is_some()` assertion above.
    #[test]
    fn shown_returns_that_window_s_payload() {
        let mut cache = Cache::new(100);
        for wid in [1, 2, 3] {
            cache.insert(wid, format!("payload-{wid}"), 1);
        }
        assert_eq!(cache.shown(2).map(String::as_str), Some("payload-2"));
        assert_eq!(cache.shown(1).map(String::as_str), Some("payload-1"));
        assert_eq!(cache.shown(3).map(String::as_str), Some("payload-3"));
    }

    #[test]
    fn refresh_replaces_the_payload() {
        let mut cache = Cache::new(100);
        cache.insert(7, "old".to_string(), 1);
        cache.insert(7, "new".to_string(), 1);
        assert_eq!(cache.shown(7).map(String::as_str), Some("new"));
        assert_eq!(wids(&cache), [7], "a refresh must not duplicate the entry");
    }

    /// The budget is a hard ceiling on the *refresh* path too — growing an
    /// existing entry has to evict just as an insert does.
    #[test]
    fn a_refresh_that_grows_past_the_budget_evicts() {
        let mut cache = Cache::new(4);
        cache.insert(1, "a".to_string(), 1);
        cache.insert(2, "b".to_string(), 1);
        cache.insert(3, "c".to_string(), 1);
        assert_eq!(cache.used, 3);
        // Grow the most-recent entry well past the budget.
        cache.insert(3, "big".to_string(), 4);
        assert!(cache.used <= 4, "budget broken: used={}", cache.used);
        assert_eq!(cache.shown(3).map(String::as_str), Some("big"));
        assert!(cache.shown(1).is_none(), "oldest should have been evicted");
    }

    /// A refresh that cannot fit must leave the usable stale preview in place
    /// rather than replace it and then evict itself.
    #[test]
    fn a_refresh_that_cannot_fit_keeps_the_old_preview() {
        let mut cache = Cache::new(1000);
        cache.insert(3, "small".to_string(), 200);
        cache.insert(1, "a".to_string(), 400);
        cache.insert(2, "b".to_string(), 400);
        assert_eq!(cache.used, 1000);
        // 3 is least-recently-shown; growing it cannot fit beside 1 and 2.
        cache.insert(3, "grown".to_string(), 201);
        assert_eq!(cache.shown(3).map(String::as_str), Some("small"));
    }

    #[test]
    fn retain_forgets_dead_windows() {
        let mut cache = Cache::new(10);
        for wid in [1, 2, 3] {
            cache.insert(wid, (), 1);
        }
        cache.retain(|wid| wid != 2);
        assert_eq!(wids(&cache), [3, 1]);
        cache.insert(4, (), 8);
        assert_eq!(wids(&cache), [4, 3, 1]);
    }
}
