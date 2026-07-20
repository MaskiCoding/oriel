//! Space topology resolved for badges: which Spaces are on screen now, which
//! belong to fullscreen apps, and each user desktop's number.

use std::collections::HashMap;

use crate::state::badge;

pub struct SpaceDesc {
    pub id: u64,
    pub current: bool,
    pub fullscreen: bool,
}

struct Mark {
    fullscreen: bool,
    current: bool,
    desktop: usize,
}

/// Built once per enumeration from the Space list in Mission Control order;
/// desktop numbers count user Spaces only.
pub struct SpaceMap {
    marks: HashMap<u64, Mark>,
    uniform: bool,
}

impl SpaceMap {
    pub fn new(spaces: impl IntoIterator<Item = SpaceDesc>) -> Self {
        let mut desktop = 0;
        let mut marks = HashMap::new();
        for s in spaces {
            if !s.fullscreen {
                desktop += 1;
            }
            marks.insert(
                s.id,
                Mark {
                    fullscreen: s.fullscreen,
                    current: s.current,
                    desktop,
                },
            );
        }
        let uniform = marks.values().all(|m| m.current && !m.fullscreen);
        Self { marks, uniform }
    }

    /// True when no window could earn a Space badge — every Space is a user
    /// desktop currently on screen — so callers can skip per-window lookups.
    pub fn uniform(&self) -> bool {
        self.uniform
    }

    /// The chip text for a window: ● minimized, ⤢ fullscreen, the Desktop
    /// number when it lives on an off-screen user Space. Empty means no chip.
    pub fn badge(&self, minimized: bool, space: Option<u64>) -> String {
        let mark = space.and_then(|id| self.marks.get(&id));
        let fullscreen = mark.is_some_and(|m| m.fullscreen);
        let desktop = mark.and_then(|m| (!m.fullscreen && !m.current).then_some(m.desktop));
        badge(minimized, fullscreen, desktop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> SpaceMap {
        SpaceMap::new([
            SpaceDesc {
                id: 1,
                current: true,
                fullscreen: false,
            },
            SpaceDesc {
                id: 279,
                current: false,
                fullscreen: false,
            },
            SpaceDesc {
                id: 310,
                current: true,
                fullscreen: true,
            },
        ])
    }

    #[test]
    fn current_desktop_earns_no_badge() {
        assert_eq!(map().badge(false, Some(1)), "");
    }

    #[test]
    fn off_screen_desktop_earns_its_number() {
        assert_eq!(map().badge(false, Some(279)), "2");
    }

    #[test]
    fn fullscreen_space_earns_the_marker_not_a_number() {
        assert_eq!(map().badge(false, Some(310)), "⤢");
    }

    #[test]
    fn minimized_composes_with_the_space_badge() {
        assert_eq!(map().badge(true, Some(279)), "● 2");
        assert_eq!(map().badge(true, None), "●");
    }

    #[test]
    fn unknown_space_falls_back_to_state_only() {
        assert_eq!(map().badge(false, Some(999)), "");
    }

    #[test]
    fn uniform_when_every_space_is_a_current_desktop() {
        assert!(!map().uniform());
        let single = SpaceMap::new([SpaceDesc {
            id: 1,
            current: true,
            fullscreen: false,
        }]);
        assert!(single.uniform());
        assert!(SpaceMap::new([]).uniform());
    }
}
