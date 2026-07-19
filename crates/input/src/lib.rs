//! Trigger engine: native-switcher suppression, hold/release tap, key capture, in-session keymap.

mod suppress;
mod tap;

pub use suppress::{Suppression, WATCHDOG_FLAG, watchdog_main};
pub use tap::{Disposition, EventTap, event_mask, flags, keycode};
