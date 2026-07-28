//! The Strip and Peek: a non-activating `NSPanel` driven via `objc2`.

mod look;
mod peek;
mod settings;
mod strip;

pub use look::{Look, ShowOn, Size, Style, Theme, TitleShow, TitleTruncate};
pub use peek::Peek;
pub use settings::Settings;
pub use strip::{Strip, Tile};
