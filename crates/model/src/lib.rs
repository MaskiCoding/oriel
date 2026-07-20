//! Pure state: windows, apps, spaces, MRU, and the filter/order/group/match resolvers.

mod mru;
mod session;
mod state;

pub use mru::Mru;
pub use session::Session;
pub use state::{WindowState, badge};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowId(pub u32);
