//! Events: incidents, maintenances and announcements.
//!
//! Six orthogonal dimensions, checked by SQL constraints:
//! - `kind`: incident, maintenance or publication (an announcement).
//! - `severity`: impact on services, absent for publications.
//! - `planned`: a maintenance announced ahead, which starts and ends on its
//!   own at the planned times.
//! - `lifecycle`: workflow state, its values depend on the kind, absent for
//!   publications.
//! - `category`: kind of announcement, publications only.
//! - `keeps_services_up`: a maintenance that leaves its services in their
//!   state while it runs.

mod dimensions;
mod input;
mod record;
mod service_tag;
#[cfg(test)]
mod tests;

pub use dimensions::*;
pub use input::*;
pub use record::*;
pub use service_tag::*;
