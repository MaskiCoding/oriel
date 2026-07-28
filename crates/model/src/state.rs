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
/// Set in `tags` on windows that belong to the user rather than the system.
/// Every real window carries it; the chrome the `WindowServer` conjures does
/// not — notably the 1800x52 title strip an app gets while it is fullscreen,
/// which is otherwise a level-0, on-screen, perfectly switchable-looking
/// window that renders as an absurd 34:1 tile.
const TAG_USER_WINDOW: u64 = 1 << 0;

// Four independent bits decoded straight from the WindowServer; grouping them
// into sub-structs would obscure that they are one flat set of flags.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowState {
    pub minimized: bool,
    pub hidden: bool,
    on_screen: bool,
    user: bool,
}

impl WindowState {
    pub fn decode(tags: u64, attributes: u64) -> Self {
        Self {
            minimized: tags & TAG_MINIMIZED != 0,
            hidden: tags & TAG_APP_HIDDEN != 0,
            on_screen: attributes & ATTR_ON_SCREEN != 0,
            user: tags & TAG_USER_WINDOW != 0,
        }
    }

    /// Whether a switcher should offer this window: on screen, or off it for a
    /// reason the user meant (minimized, app hidden). Anything else in the
    /// enumeration is `WindowServer` residue.
    pub fn switchable(self) -> bool {
        self.user && (self.on_screen || self.minimized || self.hidden)
    }

    /// Frame large enough to be a real tile. Rejects 0×0 / 1×1 helper windows
    /// from Electron and overlay toolkits; 2 pt on both edges still lets a
    /// genuine tiny utility through.
    pub fn meets_min_size(width: f64, height: f64) -> bool {
        width >= MIN_TILE_EDGE && height >= MIN_TILE_EDGE
    }

    /// Caption for a tile: AX title → `WindowServer` title → app name. Never
    /// empty when `app_name` is non-empty.
    pub fn tile_title(ax_title: Option<&str>, ws_title: Option<&str>, app_name: &str) -> String {
        for raw in [ax_title, ws_title].into_iter().flatten() {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }
        app_name.to_owned()
    }
}

/// Smallest edge (points) still accepted as a switchable window frame.
const MIN_TILE_EDGE: f64 = 2.0;

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
    /// The title strip macOS adds beside a fullscreen window, measured from a
    /// fullscreen Finder: level 0, on screen, and 1800x52.
    const FULLSCREEN_CHROME: (u64, u64) = (0x0804_0401_400c_2080, 0x3);

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
        // Every combination, including the ones the markers can hold at once.
        assert_eq!(badge(true, true, None), "● ⤢");
        assert_eq!(badge(false, true, Some(4)), "⤢ 4");
        assert_eq!(badge(true, true, Some(5)), "● ⤢ 5");
    }

    #[test]
    fn min_size_rejects_helper_stubs() {
        assert!(!WindowState::meets_min_size(0.0, 0.0));
        assert!(!WindowState::meets_min_size(1.0, 1.0));
        assert!(!WindowState::meets_min_size(1.0, 100.0));
        assert!(!WindowState::meets_min_size(100.0, 1.0));
    }

    #[test]
    fn min_size_keeps_small_utilities() {
        assert!(WindowState::meets_min_size(2.0, 2.0));
        assert!(WindowState::meets_min_size(32.0, 24.0));
        assert!(WindowState::meets_min_size(200.0, 40.0));
    }

    #[test]
    fn tile_title_falls_back_ax_then_ws_then_app() {
        assert_eq!(
            WindowState::tile_title(Some(" AX "), Some("WS"), "App"),
            "AX"
        );
        assert_eq!(
            WindowState::tile_title(Some("  "), Some(" WS "), "App"),
            "WS"
        );
        assert_eq!(WindowState::tile_title(None, Some(""), "App"), "App");
        assert_eq!(WindowState::tile_title(None, None, "App"), "App");
        assert_eq!(WindowState::tile_title(None, None, ""), "");
    }

    #[test]
    fn the_chrome_beside_a_fullscreen_window_is_not_switchable() {
        let state = WindowState::decode(FULLSCREEN_CHROME.0, FULLSCREEN_CHROME.1);
        assert!(
            !state.switchable(),
            "the fullscreen title strip must never reach the strip"
        );
    }

    #[test]
    fn every_real_window_shape_still_switches() {
        for (tags, attrs) in [NORMAL, MINIMIZED, HIDDEN] {
            assert!(WindowState::decode(tags, attrs).switchable());
        }
    }
}
