//! Trigger engine: native-switcher suppression, hold/release tap, key capture, in-session keymap.

mod suppress;

pub use suppress::{Suppression, WATCHDOG_FLAG, watchdog_main};
