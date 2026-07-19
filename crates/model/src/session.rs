/// Selection state for one switching session over a fixed window list.
///
/// Starts at index 1 (the previous window) so an immediate release — the
/// quick flick — jumps back without any extra logic.
#[derive(Debug)]
pub struct Session {
    len: usize,
    selected: usize,
}

impl Session {
    pub fn start(len: usize) -> Option<Self> {
        (len > 0).then_some(Self {
            len,
            selected: usize::from(len > 1),
        })
    }

    pub fn cycle(&mut self, backward: bool) {
        self.selected = if backward {
            self.selected.checked_sub(1).unwrap_or(self.len - 1)
        } else {
            (self.selected + 1) % self.len
        };
    }

    pub fn select(&mut self, index: usize) {
        if index < self.len {
            self.selected = index;
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list_has_no_session() {
        assert!(Session::start(0).is_none());
    }

    #[test]
    fn starts_on_previous_window() {
        assert_eq!(Session::start(3).unwrap().selected(), 1);
    }

    #[test]
    fn single_window_starts_on_itself() {
        assert_eq!(Session::start(1).unwrap().selected(), 0);
    }

    #[test]
    fn single_window_cycles_in_place() {
        let mut s = Session::start(1).unwrap();
        s.cycle(false);
        assert_eq!(s.selected(), 0);
        s.cycle(true);
        assert_eq!(s.selected(), 0);
    }

    #[test]
    fn cycles_forward_with_wrap() {
        let mut s = Session::start(3).unwrap();
        s.cycle(false);
        assert_eq!(s.selected(), 2);
        s.cycle(false);
        assert_eq!(s.selected(), 0);
    }

    #[test]
    fn cycles_backward_with_wrap() {
        let mut s = Session::start(3).unwrap();
        s.cycle(true);
        assert_eq!(s.selected(), 0);
        s.cycle(true);
        assert_eq!(s.selected(), 2);
    }

    #[test]
    fn select_ignores_out_of_range() {
        let mut s = Session::start(2).unwrap();
        s.select(5);
        assert_eq!(s.selected(), 1);
        s.select(0);
        assert_eq!(s.selected(), 0);
    }
}
