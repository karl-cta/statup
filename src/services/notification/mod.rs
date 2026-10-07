//! Notifications: what Statup tells the destinations an admin set up.

mod deliver;
mod format;
mod notice;
mod queue;
mod sender;

pub use deliver::{deliver_due, page_address, spawn_notifications};
pub use format::*;
pub use notice::*;
pub use queue::notify;
pub use sender::{Failure, Notifier, SEND_TIMEOUT, Sender};
