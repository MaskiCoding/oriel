//! Pure state: windows, apps, spaces, MRU, and the filter/order/group/match resolvers.

mod caption;
mod mru;
mod session;
mod spaces;
mod state;

pub use caption::{Caption, decode_title};
pub use mru::Mru;
pub use session::Session;
pub use spaces::{SpaceDesc, SpaceMap};
pub use state::WindowState;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowId(pub u32);
