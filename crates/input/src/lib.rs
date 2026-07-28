//! Trigger engine: native-switcher suppression, hold/release tap, key capture, in-session keymap.

mod hotkey;
pub mod keymap;
mod suppress;
mod tap;

pub use hotkey::{CMD, Hotkeys, KEY_ESCAPE, KEY_TAB, OPTION, SHIFT, Trigger};
pub use keymap::{Binding, CONTROL, parse_trigger};
pub use suppress::{Suppression, WATCHDOG_FLAG, watchdog_main};
pub use tap::{Disposition, EventTap, TapHandle, event_mask, flags, keycode};
