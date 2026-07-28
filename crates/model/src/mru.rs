use crate::WindowId;

/// Global most-recently-focused order, most recent first.
#[derive(Default, Debug)]
pub struct Mru {
    order: Vec<WindowId>,
}

impl Mru {
    pub fn touch(&mut self, id: WindowId) {
        self.order.retain(|&w| w != id);
        self.order.insert(0, id);
    }

    /// Reconciles with a fresh enumeration: forgets vanished windows and
    /// appends never-focused ones at the end in enumeration order.
    pub fn sync(&mut self, current: &[WindowId]) {
        self.order.retain(|w| current.contains(w));
        for &id in current {
            if !self.order.contains(&id) {
                self.order.push(id);
            }
        }
    }

    pub fn order(&self) -> &[WindowId] {
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(raw: &[u32]) -> Vec<WindowId> {
        raw.iter().copied().map(WindowId).collect()
    }

    #[test]
    fn touch_moves_to_front() {
        let mut mru = Mru::default();
        mru.sync(&ids(&[1, 2, 3]));
        mru.touch(WindowId(3));
        assert_eq!(mru.order(), ids(&[3, 1, 2]));
        mru.touch(WindowId(3));
        assert_eq!(mru.order(), ids(&[3, 1, 2]));
    }

    #[test]
    fn touch_admits_unknown_windows() {
        let mut mru = Mru::default();
        mru.touch(WindowId(9));
        assert_eq!(mru.order(), ids(&[9]));
    }

    #[test]
    fn sync_drops_vanished_keeps_order() {
        let mut mru = Mru::default();
        for id in [3, 2, 1] {
            mru.touch(WindowId(id));
        }
        // Recency is 1, 2, 3. Hand `sync` the survivors in the *opposite*
        // order, so an implementation that simply adopted the enumeration
        // order would produce [3, 2] and fail.
        mru.sync(&ids(&[3, 2]));
        assert_eq!(mru.order(), ids(&[2, 3]));
    }

    #[test]
    fn sync_appends_new_at_end_in_enumeration_order() {
        let mut mru = Mru::default();
        mru.touch(WindowId(5));
        mru.sync(&ids(&[7, 5, 6]));
        assert_eq!(mru.order(), ids(&[5, 7, 6]));
    }
}
