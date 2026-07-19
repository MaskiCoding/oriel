//! Pure state: windows, apps, spaces, MRU, and the filter/order/group/match resolvers.

mod mru;
mod session;

pub use mru::Mru;
pub use session::Session;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowId(pub u32);
