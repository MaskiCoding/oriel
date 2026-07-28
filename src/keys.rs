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

/// 2D step across the strip's real rows, wrapping at every edge.
/// `rows` holds the layout's tile indices grouped top row first: `Left`/`Right`
/// walk the flat order, `Up`/`Down` change row keeping the nearest column.
pub fn step_rows(selected: usize, n: usize, rows: &[Vec<usize>], dir: Dir) -> usize {
    if n <= 1 {
        return 0;
    }
    match dir {
        Dir::Left => selected.checked_sub(1).unwrap_or(n - 1),
        Dir::Right => (selected + 1) % n,
        Dir::Up | Dir::Down => vertical(selected, n, rows, dir),
    }
}

fn vertical(selected: usize, n: usize, rows: &[Vec<usize>], dir: Dir) -> usize {
    let Some(r) = rows.iter().position(|row| row.contains(&selected)) else {
        // No layout recorded yet: walk the flat order rather than stand still.
        return if matches!(dir, Dir::Down) {
            (selected + 1) % n
        } else {
            selected.checked_sub(1).unwrap_or(n - 1)
        };
    };
    let col = rows[r].iter().position(|&i| i == selected).unwrap_or(0);
    let last = rows.len() - 1;
    let next_row = if matches!(dir, Dir::Down) {
        if r == last { 0 } else { r + 1 }
    } else if r == 0 {
        last
    } else {
        r - 1
    };
    let target = &rows[next_row];
    if target.is_empty() {
        return selected;
    }
    // Same column where the row reaches it, otherwise that row's last tile.
    target[col.min(target.len() - 1)]
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

    fn rows(spec: &[&[usize]]) -> Vec<Vec<usize>> {
        spec.iter().map(|r| r.to_vec()).collect()
    }

    #[test]
    fn left_right_wrap_the_flat_order() {
        let r = rows(&[&[0, 1, 2], &[3, 4]]);
        assert_eq!(step_rows(0, 5, &r, Dir::Left), 4);
        assert_eq!(step_rows(4, 5, &r, Dir::Right), 0);
        assert_eq!(step_rows(1, 5, &r, Dir::Right), 2);
    }

    #[test]
    fn up_down_keep_the_column_and_wrap() {
        // 0 1 2
        // 3 4
        let r = rows(&[&[0, 1, 2], &[3, 4]]);
        assert_eq!(step_rows(0, 5, &r, Dir::Down), 3);
        assert_eq!(step_rows(1, 5, &r, Dir::Down), 4);
        assert_eq!(step_rows(3, 5, &r, Dir::Up), 0);
        // wrap: bottom row down lands on the top row
        assert_eq!(step_rows(3, 5, &r, Dir::Down), 0);
        assert_eq!(step_rows(0, 5, &r, Dir::Up), 3);
    }

    #[test]
    fn short_row_clamps_to_its_last_tile() {
        // column 2 does not exist in the bottom row, so down from it clamps
        let r = rows(&[&[0, 1, 2], &[3, 4]]);
        assert_eq!(step_rows(2, 5, &r, Dir::Down), 4);
        // and coming back up keeps the column it landed in, not the one it left
        assert_eq!(step_rows(4, 5, &r, Dir::Up), 1);
    }

    #[test]
    fn ragged_rows_move_by_nearest_column() {
        // 0 1 2
        // 3 4
        // 5 6 7 8
        let r = rows(&[&[0, 1, 2], &[3, 4], &[5, 6, 7, 8]]);
        assert_eq!(step_rows(2, 9, &r, Dir::Down), 4);
        assert_eq!(step_rows(8, 9, &r, Dir::Up), 4);
        assert_eq!(step_rows(6, 9, &r, Dir::Up), 4);
        assert_eq!(step_rows(5, 9, &r, Dir::Down), 0);
    }

    #[test]
    fn single_row_and_single_tile() {
        let one_row = rows(&[&[0, 1, 2]]);
        assert_eq!(step_rows(1, 3, &one_row, Dir::Down), 1);
        assert_eq!(step_rows(1, 3, &one_row, Dir::Up), 1);
        let single = rows(&[&[0]]);
        for d in [Dir::Left, Dir::Right, Dir::Up, Dir::Down] {
            assert_eq!(step_rows(0, 1, &single, d), 0);
        }
    }

    #[test]
    fn list_style_is_one_tile_per_row() {
        let r = rows(&[&[0], &[1], &[2]]);
        assert_eq!(step_rows(0, 3, &r, Dir::Down), 1);
        assert_eq!(step_rows(2, 3, &r, Dir::Down), 0);
        assert_eq!(step_rows(0, 3, &r, Dir::Up), 2);
    }

    #[test]
    fn no_layout_yet_falls_back_to_flat_order() {
        let empty: Vec<Vec<usize>> = Vec::new();
        assert_eq!(step_rows(0, 4, &empty, Dir::Down), 1);
        assert_eq!(step_rows(0, 4, &empty, Dir::Up), 3);
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
