//! Notifications: what Statup tells the destinations an admin set up.

mod format;
mod notice;
mod queue;

pub use format::*;
pub use notice::*;
pub use queue::notify;
