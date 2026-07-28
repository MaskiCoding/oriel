//! In-session key bindings and typed-character extraction.

use objc2_core_graphics::{CGEvent, CGEventFlags};

/// Resolved virtual keycodes for the configurable action keys.
#[derive(Clone, Debug)]
pub struct ActionKeys {
    pub focus: Option<i64>,
    pub cancel: Option<i64>,
    pub close: Option<i64>,
    pub minimize: Option<i64>,
    pub fullscreen: Option<i64>,
    pub quit_app: Option<i64>,
    pub hide_app: Option<i64>,
}

impl ActionKeys {
    pub fn resolve(keys: &config::Keys) -> Self {
        let code = |name: &str| input::keymap::keycode(name).map(i64::from);
        Self {
            focus: code(&keys.focus),
            cancel: code(&keys.cancel),
            close: code(&keys.close),
            minimize: code(&keys.minimize),
            fullscreen: code(&keys.fullscreen),
            quit_app: code(&keys.quit_app),
            hide_app: code(&keys.hide_app),
        }
    }
}

/// Backspace / Delete virtual keycode (Carbon).
pub const KEY_DELETE: i64 = 51;

/// Arrow and vim movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Maps a keycode to a movement direction given the control toggles.
pub fn movement(code: i64, controls: &config::Controls) -> Option<Dir> {
    if controls.arrow_keys {
        match code {
            123 => return Some(Dir::Left),
            124 => return Some(Dir::Right),
            125 => return Some(Dir::Down),
            126 => return Some(Dir::Up),
            _ => {}
        }
    }
    if controls.vim_keys {
        match code {
            4 => return Some(Dir::Left),   // h
            38 => return Some(Dir::Down),  // j
            40 => return Some(Dir::Up),    // k
            37 => return Some(Dir::Right), // l
            _ => {}
        }
    }
    None
}

/// 2D step on a wrapping grid of `n` tiles with `cols` columns.
/// Left from 0 goes to the last tile; up/down wrap within the column.
pub fn step(selected: usize, n: usize, cols: usize, dir: Dir) -> usize {
    if n == 0 {
        return 0;
    }
    if n == 1 {
        return 0;
    }
    let cols = cols.max(1);
    match dir {
        Dir::Left => selected.checked_sub(1).unwrap_or(n - 1),
        Dir::Right => (selected + 1) % n,
        Dir::Up => {
            let col = selected % cols;
            if selected >= cols {
                selected - cols
            } else {
                let mut row = (n - 1) / cols;
                while row * cols + col >= n && row > 0 {
                    row -= 1;
                }
                row * cols + col
            }
        }
        Dir::Down => {
            let col = selected % cols;
            let next = selected + cols;
            if next < n { next } else { col }
        }
    }
}

/// Column count for movement: list is one column; otherwise ~7 per row.
pub fn grid_cols(n: usize, style: ui::Style) -> usize {
    match style {
        ui::Style::List => 1,
        ui::Style::Gallery | ui::Style::Icons => {
            if n == 0 {
                1
            } else {
                n.min(7)
            }
        }
    }
}

/// Unicode string produced by a key-down, via the event's keyboard string
/// (layout-correct — not a keycode table).
pub fn event_chars(event: &CGEvent) -> String {
    let mut buf = [0u16; 16];
    let mut len: core::ffi::c_ulong = 0;
    // SAFETY: buffer length matches `max_string_length`; both out-pointers are valid.
    unsafe {
        CGEvent::keyboard_get_unicode_string(
            Some(event),
            buf.len() as core::ffi::c_ulong,
            &raw mut len,
            buf.as_mut_ptr(),
        );
    }
    let n = usize::try_from(len).unwrap_or(0).min(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// Whether `chars` should append to the Filter query (no control characters).
pub fn is_typing(chars: &str) -> bool {
    !chars.is_empty()
        && chars
            .chars()
            .all(|c| !c.is_control() && !is_function_key(c))
}

/// Arrows, F-keys, Home/End and friends arrive as Unicode private-use
/// characters, which are not control characters — without this they would be
/// typed into the Filter query instead of moving the selection.
fn is_function_key(c: char) -> bool {
    ('\u{e000}'..='\u{f8ff}').contains(&c)
}

/// Cmd/Ctrl held — not a Filter keystroke.
pub fn has_cmd_or_ctrl(flags: CGEventFlags) -> bool {
    flags.contains(CGEventFlags::MaskCommand) || flags.contains(CGEventFlags::MaskControl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_keys_resolve_defaults() {
        let k = ActionKeys::resolve(&config::Keys::default());
        assert_eq!(k.focus, Some(36));
        assert_eq!(k.cancel, Some(53));
        assert_eq!(k.close, Some(13));
        assert_eq!(k.minimize, Some(46));
        assert_eq!(k.fullscreen, Some(3));
        assert_eq!(k.quit_app, Some(12));
        assert_eq!(k.hide_app, Some(4));
    }

    #[test]
    fn left_from_first_wraps_to_last() {
        assert_eq!(step(0, 5, 3, Dir::Left), 4);
        assert_eq!(step(4, 5, 3, Dir::Right), 0);
    }

    #[test]
    fn up_down_wrap_in_column() {
        // 0 1 2
        // 3 4
        assert_eq!(step(0, 5, 3, Dir::Down), 3);
        assert_eq!(step(3, 5, 3, Dir::Down), 0);
        assert_eq!(step(1, 5, 3, Dir::Down), 4);
        assert_eq!(step(4, 5, 3, Dir::Down), 1);
        assert_eq!(step(0, 5, 3, Dir::Up), 3);
        assert_eq!(step(2, 5, 3, Dir::Up), 2); // short last row → stay in col 2
    }

    #[test]
    fn list_is_one_column() {
        assert_eq!(grid_cols(9, ui::Style::List), 1);
        assert_eq!(grid_cols(9, ui::Style::Gallery), 7);
        assert_eq!(grid_cols(3, ui::Style::Icons), 3);
    }

    #[test]
    fn typing_rejects_controls() {
        assert!(is_typing("a"));
        assert!(is_typing("ö"));
        assert!(!is_typing(""));
        assert!(!is_typing("\u{1b}"));
        assert!(!is_typing("\n"));
        // AppKit reports arrows and F-keys in the private use area.
        assert!(!is_typing("\u{f700}")); // up
        assert!(!is_typing("\u{f702}")); // left
        assert!(!is_typing("\u{f704}")); // F1
        assert!(!is_typing("a\u{f702}")); // mixed is still not typing
    }

    #[test]
    fn movement_respects_toggles() {
        let arrows = config::Controls {
            arrow_keys: true,
            vim_keys: false,
            ..config::Controls::default()
        };
        assert_eq!(movement(123, &arrows), Some(Dir::Left));
        assert_eq!(movement(4, &arrows), None);

        let vim = config::Controls {
            arrow_keys: false,
            vim_keys: true,
            ..config::Controls::default()
        };
        assert_eq!(movement(4, &vim), Some(Dir::Left));
        assert_eq!(movement(123, &vim), None);
    }
}
