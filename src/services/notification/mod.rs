//! Notifications: what Statup tells the destinations an admin set up.

mod deliver;
mod destinations;
mod format;
mod link_mail;
mod mail;
mod notice;
mod queue;
mod reminder_mail;
mod reminders;
mod sender;
mod subscribers;

pub use deliver::{KEEP_HISTORY, page_address, spawn_notifications};
pub(crate) use deliver::{PAGE_ADDRESS_SETTING, channel_detail};
pub use destinations::{
    completed_target, destination_allowed, destination_name_refusal, email_addresses,
};
pub use format::*;
pub use link_mail::{LinkEmail, LinkPurpose};
pub use mail::Unsubscribe;
pub use notice::*;
pub use queue::{notify, notify_later};
pub use reminder_mail::ReminderEmail;
pub use reminders::remind_overdue;
pub use sender::{Failure, Notifier, SEND_TIMEOUT, Sender, SubscriberEmail, SubscriberSender};
pub use subscribers::{Round, deliver_to_subscribers, spawn_subscriber_mail};
