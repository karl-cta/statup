//! Notifications: what Statup tells the destinations an admin set up.

mod deliver;
mod destinations;
mod format;
mod notice;
mod queue;
mod sender;

pub(crate) use deliver::PAGE_ADDRESS_SETTING;
pub use deliver::{deliver_due, page_address, spawn_notifications};
pub use destinations::{destination_allowed, destination_name_refusal};
pub use format::*;
pub use notice::*;
pub use queue::notify;
pub use sender::{Failure, Notifier, SEND_TIMEOUT, Sender};
