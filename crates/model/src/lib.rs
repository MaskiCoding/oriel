//! Pure state: windows, apps, spaces, MRU, and the filter/order/group/match resolvers.

mod filter;
mod lantern;
mod lens;
mod mru;
mod rules;
mod session;
mod spaces;
mod state;

pub use filter::{Field, Match, Tier, filter, match_str, match_window};
pub use lantern::{Lit, Proc, burn, by_owner, descendants, lit};
pub use lens::{
    AppKey, AppScope, Disposition, Grouping, Lens, Order, ResolveCtx, ScreenId, ScreenScope,
    SpaceId, SpaceScope, Window, resolve,
};
pub use mru::Mru;
pub use rules::{HideWindows, PassTriggers, Rule, Rules};
pub use session::Session;
pub use spaces::{SpaceDesc, SpaceMap};
pub use state::WindowState;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowId(pub u32);

/// A process id. Distinct from [`WindowId`] on purpose: both are small integers
/// that travel together through the same functions, and nothing but the type
/// stops one being passed where the other belongs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Pid(pub i32);

impl std::fmt::Display for Pid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
