//! The Strip and Peek: a non-activating `NSPanel` driven via `objc2`.

mod look;
mod strip;

pub use look::{Look, ShowOn, Size, Style, Theme};
pub use strip::{Strip, Tile};
