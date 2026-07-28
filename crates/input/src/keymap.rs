//! Config key-name → virtual keycode, and trigger-string parsing.

use crate::hotkey::{CMD, OPTION, SHIFT};

/// Carbon `controlKey` (`1 << 12`). Kept here so `hotkey` stays untouched.
pub const CONTROL: u32 = 1 << 12;

/// A parsed key + modifier mask, without a hotkey registration id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub key: u32,
    pub modifiers: u32,
}

/// Virtual keycode for a config key name: `"w"`, `"return"`, `"escape"`, `"left"`, `"f1"`, …
pub fn keycode(name: &str) -> Option<u32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "v" => 9,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "9" => 25,
        "7" => 26,
        "8" => 28,
        "0" => 29,
        "o" => 31,
        "u" => 32,
        "i" => 34,
        "p" => 35,
        "return" | "enter" => 36,
        "l" => 37,
        "j" => 38,
        "k" => 40,
        "n" => 45,
        "m" => 46,
        "tab" => 48,
        "space" => 49,
        "`" | "grave" => 50,
        "delete" => 51,
        "escape" | "esc" => 53,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f3" => 99,
        "f8" => 100,
        "f9" => 101,
        "f11" => 103,
        "f10" => 109,
        "f12" => 111,
        "f4" => 118,
        "f2" => 120,
        "f1" => 122,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        _ => return None,
    })
}

fn modifier_mask(name: &str) -> Option<u32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "cmd" | "command" | "⌘" => CMD,
        "alt" | "opt" | "option" | "⌥" => OPTION,
        "ctrl" | "control" | "⌃" => CONTROL,
        "shift" | "⇧" => SHIFT,
        _ => return None,
    })
}

/// Parses a lens trigger string like `cmd+tab`, `alt+shift+grave`, `ctrl+f1`
/// into a [`Binding`]. Returns `None` if unparseable.
pub fn parse_trigger(s: &str) -> Option<Binding> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let mut modifiers = 0u32;
    let mut key = None;

    for part in s.split('+') {
        let part = part.trim();
        if part.is_empty() {
            return None;
        }
        if let Some(mask) = modifier_mask(part) {
            modifiers |= mask;
            continue;
        }
        if key.is_some() {
            return None;
        }
        key = Some(keycode(part)?);
    }

    Some(Binding {
        key: key?,
        modifiers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_digits() {
        assert_eq!(keycode("w"), Some(13));
        assert_eq!(keycode("m"), Some(46));
        assert_eq!(keycode("f"), Some(3));
        assert_eq!(keycode("q"), Some(12));
        assert_eq!(keycode("h"), Some(4));
        assert_eq!(keycode("a"), Some(0));
        assert_eq!(keycode("0"), Some(29));
        assert_eq!(keycode("5"), Some(23));
        assert_eq!(keycode("9"), Some(25));
    }

    #[test]
    fn named_keys() {
        assert_eq!(keycode("tab"), Some(48));
        assert_eq!(keycode("return"), Some(36));
        assert_eq!(keycode("enter"), Some(36));
        assert_eq!(keycode("escape"), Some(53));
        assert_eq!(keycode("esc"), Some(53));
        assert_eq!(keycode("space"), Some(49));
        assert_eq!(keycode("delete"), Some(51));
        assert_eq!(keycode("left"), Some(123));
        assert_eq!(keycode("right"), Some(124));
        assert_eq!(keycode("down"), Some(125));
        assert_eq!(keycode("up"), Some(126));
        assert_eq!(keycode("`"), Some(50));
        assert_eq!(keycode("grave"), Some(50));
    }

    #[test]
    fn function_keys() {
        assert_eq!(keycode("f1"), Some(122));
        assert_eq!(keycode("f2"), Some(120));
        assert_eq!(keycode("f3"), Some(99));
        assert_eq!(keycode("f4"), Some(118));
        assert_eq!(keycode("f5"), Some(96));
        assert_eq!(keycode("f6"), Some(97));
        assert_eq!(keycode("f7"), Some(98));
        assert_eq!(keycode("f8"), Some(100));
        assert_eq!(keycode("f9"), Some(101));
        assert_eq!(keycode("f10"), Some(109));
        assert_eq!(keycode("f11"), Some(103));
        assert_eq!(keycode("f12"), Some(111));
    }

    #[test]
    fn keycode_case_insensitive() {
        assert_eq!(keycode("W"), Some(13));
        assert_eq!(keycode("Escape"), Some(53));
        assert_eq!(keycode("F1"), Some(122));
        assert_eq!(keycode("TAB"), Some(48));
    }

    #[test]
    fn keycode_unknown() {
        assert_eq!(keycode(""), None);
        assert_eq!(keycode("f13"), None);
        assert_eq!(keycode("comma"), None);
        assert_eq!(keycode("plus"), None);
    }

    #[test]
    fn cmd_aliases() {
        let want = Binding {
            key: 48,
            modifiers: CMD,
        };
        assert_eq!(parse_trigger("cmd+tab"), Some(want));
        assert_eq!(parse_trigger("command+tab"), Some(want));
        assert_eq!(parse_trigger("⌘+tab"), Some(want));
    }

    #[test]
    fn alt_aliases() {
        let want = Binding {
            key: 13,
            modifiers: OPTION,
        };
        assert_eq!(parse_trigger("alt+w"), Some(want));
        assert_eq!(parse_trigger("opt+w"), Some(want));
        assert_eq!(parse_trigger("option+w"), Some(want));
        assert_eq!(parse_trigger("⌥+w"), Some(want));
    }

    #[test]
    fn ctrl_aliases() {
        let want = Binding {
            key: 122,
            modifiers: CONTROL,
        };
        assert_eq!(parse_trigger("ctrl+f1"), Some(want));
        assert_eq!(parse_trigger("control+f1"), Some(want));
        assert_eq!(parse_trigger("⌃+f1"), Some(want));
    }

    #[test]
    fn shift_aliases() {
        let want = Binding {
            key: 49,
            modifiers: SHIFT,
        };
        assert_eq!(parse_trigger("shift+space"), Some(want));
        assert_eq!(parse_trigger("⇧+space"), Some(want));
    }

    #[test]
    fn order_independent() {
        let want = Binding {
            key: 50,
            modifiers: OPTION | SHIFT,
        };
        assert_eq!(parse_trigger("alt+shift+`"), Some(want));
        assert_eq!(parse_trigger("shift+alt+grave"), Some(want));
        assert_eq!(parse_trigger("`+alt+shift"), Some(want));
    }

    #[test]
    fn case_and_whitespace() {
        assert_eq!(
            parse_trigger("  CMD + Tab  "),
            Some(Binding {
                key: 48,
                modifiers: CMD,
            })
        );
        assert_eq!(
            parse_trigger("Alt+Shift+Return"),
            Some(Binding {
                key: 36,
                modifiers: OPTION | SHIFT,
            })
        );
    }

    #[test]
    fn zero_modifiers() {
        assert_eq!(
            parse_trigger("w"),
            Some(Binding {
                key: 13,
                modifiers: 0,
            })
        );
        assert_eq!(
            parse_trigger("escape"),
            Some(Binding {
                key: 53,
                modifiers: 0,
            })
        );
    }

    #[test]
    fn all_four_modifiers() {
        assert_eq!(
            parse_trigger("cmd+ctrl+alt+shift+m"),
            Some(Binding {
                key: 46,
                modifiers: CMD | CONTROL | OPTION | SHIFT,
            })
        );
    }

    #[test]
    fn digit_and_arrow_triggers() {
        assert_eq!(
            parse_trigger("cmd+1"),
            Some(Binding {
                key: 18,
                modifiers: CMD,
            })
        );
        assert_eq!(
            parse_trigger("ctrl+left"),
            Some(Binding {
                key: 123,
                modifiers: CONTROL,
            })
        );
    }

    #[test]
    fn empty_and_junk() {
        assert_eq!(parse_trigger(""), None);
        assert_eq!(parse_trigger("   "), None);
        assert_eq!(parse_trigger("nope"), None);
        assert_eq!(parse_trigger("cmd+nope"), None);
        assert_eq!(parse_trigger("+++"), None);
    }

    #[test]
    fn modifier_only() {
        assert_eq!(parse_trigger("cmd"), None);
        assert_eq!(parse_trigger("cmd+shift"), None);
        assert_eq!(parse_trigger("ctrl+alt+shift"), None);
    }

    #[test]
    fn doubled_separators() {
        assert_eq!(parse_trigger("cmd++tab"), None);
        assert_eq!(parse_trigger("+tab"), None);
        assert_eq!(parse_trigger("tab+"), None);
        assert_eq!(parse_trigger("cmd+ +tab"), None);
    }

    #[test]
    fn two_keys_rejected() {
        assert_eq!(parse_trigger("cmd+w+m"), None);
        assert_eq!(parse_trigger("tab+escape"), None);
    }

    #[test]
    fn action_keys_used_in_session() {
        assert_eq!(
            parse_trigger("w"),
            Some(Binding {
                key: 13,
                modifiers: 0,
            })
        );
        assert_eq!(
            parse_trigger("m"),
            Some(Binding {
                key: 46,
                modifiers: 0,
            })
        );
        assert_eq!(
            parse_trigger("f"),
            Some(Binding {
                key: 3,
                modifiers: 0,
            })
        );
        assert_eq!(
            parse_trigger("h"),
            Some(Binding {
                key: 4,
                modifiers: 0,
            })
        );
    }
}
