//! Domain models - User, Service, Event, Icon, `EventTemplate` and related types.

mod builtin_icon;
mod event;
mod event_template;
mod icon;
mod service;
mod user;

pub use builtin_icon::*;
pub use event::*;
pub use event_template::*;
pub use icon::*;
pub use service::*;
pub use user::*;
