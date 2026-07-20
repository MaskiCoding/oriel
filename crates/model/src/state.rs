//! Window state decoded from the `WindowServer` iterator's raw bits.
//!
//! The positions are private API with no headers; each was established
//! empirically on macOS 26 by diffing `--window-bits` dumps around
//! minimize/hide/restore of real windows.

/// Set in `tags` while a window sits minimized in the Dock.
const TAG_MINIMIZED: u64 = 1 << 60;
/// Set in `tags` while the window's app is hidden (⌘H).
const TAG_APP_HIDDEN: u64 = 1 << 39;
/// Set in `attributes` while the window is ordered in on its Space.
const ATTR_ON_SCREEN: u64 = 0x2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowState {
    pub minimized: bool,
    pub hidden: bool,
    on_screen: bool,
}

impl WindowState {
    pub fn decode(tags: u64, attributes: u64) -> Self {
        Self {
            minimized: tags & TAG_MINIMIZED != 0,
            hidden: tags & TAG_APP_HIDDEN != 0,
            on_screen: attributes & ATTR_ON_SCREEN != 0,
        }
    }

    /// Whether a switcher should offer this window: on screen, or off it for a
    /// reason the user meant (minimized, app hidden). Anything else in the
    /// enumeration is `WindowServer` residue.
    pub fn switchable(self) -> bool {
        self.on_screen || self.minimized || self.hidden
    }
}

/// The marker text for a tile: minimized ●, fullscreen ⤢, and the Desktop
/// number when the window lives on another user Space. Empty means no chip.
pub(crate) fn badge(minimized: bool, fullscreen: bool, desktop: Option<usize>) -> String {
    let mut parts = Vec::new();
    if minimized {
        parts.push("●".to_string());
    }
    if fullscreen {
        parts.push("⤢".to_string());
    }
    if let Some(n) = desktop {
        parts.push(n.to_string());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sampled from `--window-bits` on macOS 26: an on-screen window, the same
    // window minimized, a hidden app's window, and an unordered ghost window
    // that only appears once enumeration includes minimized Spaces.
    const NORMAL: (u64, u64) = (0x0300_0001_0008_2401, 0x3);
    const MINIMIZED: (u64, u64) = (0x1300_0001_0048_0001, 0x1);
    const HIDDEN: (u64, u64) = (0x0300_0081_0008_0401, 0x1);
    const GHOST: (u64, u64) = (0x0300_0001_0048_0001, 0x1);

    #[test]
    fn on_screen_window_is_switchable() {
        let state = WindowState::decode(NORMAL.0, NORMAL.1);
        assert!(!state.minimized);
        assert!(!state.hidden);
        assert!(state.switchable());
    }

    #[test]
    fn minimized_window_is_marked_and_switchable() {
        let state = WindowState::decode(MINIMIZED.0, MINIMIZED.1);
        assert!(state.minimized);
        assert!(state.switchable());
    }

    #[test]
    fn hidden_app_window_is_marked_and_switchable() {
        let state = WindowState::decode(HIDDEN.0, HIDDEN.1);
        assert!(state.hidden);
        assert!(!state.minimized);
        assert!(state.switchable());
    }

    #[test]
    fn unordered_ghost_is_not_switchable() {
        assert!(!WindowState::decode(GHOST.0, GHOST.1).switchable());
    }

    #[test]
    fn badge_composes_in_order() {
        assert_eq!(badge(false, false, None), "");
        assert_eq!(badge(true, false, None), "●");
        assert_eq!(badge(false, true, None), "⤢");
        assert_eq!(badge(false, false, Some(3)), "3");
        assert_eq!(badge(true, false, Some(2)), "● 2");
    }
}
