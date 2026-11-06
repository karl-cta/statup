//! The people told by email about what the page publishes.

use chrono::{DateTime, Utc};

use super::{Happening, Lifecycle};

/// The kinds of events a subscriber receives, one box each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct Choices {
    #[sqlx(rename = "on_incidents")]
    pub incidents: bool,
    #[sqlx(rename = "on_maintenances")]
    pub maintenances: bool,
    #[sqlx(rename = "on_publications")]
    pub publications: bool,
}

impl Choices {
    /// Every box, as the form offers them first.
    pub const EVERYTHING: Self = Self {
        incidents: true,
        maintenances: true,
        publications: true,
    };

    /// At least one box: a subscription to nothing is none.
    pub fn any(self) -> bool {
        self.incidents || self.maintenances || self.publications
    }
}

/// A member, at the address of their account, or a visitor, at the address
/// they confirmed. Not `Debug`: it holds the unsubscribe secret in clear.
#[derive(Clone, sqlx::FromRow)]
pub struct Subscriber {
    pub id: i64,
    pub user_id: Option<i64>,
    pub email: Option<String>,
    #[sqlx(flatten)]
    pub choices: Choices,
    pub unsubscribe_secret: String,
    /// The first consent.
    pub created_at: DateTime<Utc>,
}

/// A subscriber as the administrators list them, with the outcome of their
/// latest email. Holds no secret.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SubscriberRow {
    pub id: i64,
    pub user_id: Option<i64>,
    /// The member's display name; none for a visitor.
    pub name: Option<String>,
    /// The visitor's address, or the account address of the member.
    pub email: String,
    #[sqlx(flatten)]
    pub choices: Choices,
    pub created_at: DateTime<Utc>,
    /// `sent` or `failed`: a skipped email says nothing of the address.
    pub last_status: Option<String>,
    pub last_failure: Option<String>,
    /// When it left, or when it was given up.
    pub last_at: Option<DateTime<Utc>>,
}

/// A visitor's subscription until they open the link sent to the address.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SubscriptionRequest {
    pub email: String,
    pub secret_hash: String,
    pub locale: String,
    #[sqlx(flatten)]
    pub choices: Choices,
}

/// The next email a subscriber is waiting for, with what it takes to write
/// and address it. Not `Debug`: it holds the unsubscribe secret.
#[derive(Clone, sqlx::FromRow)]
pub struct SubscriberMessage {
    pub id: i64,
    pub subscriber_id: i64,
    pub happening: Happening,
    pub event_id: i64,
    pub update_id: Option<i64>,
    /// The state the event moved to, when it moved.
    pub lifecycle: Option<Lifecycle>,
    pub attempts: i64,
    /// When it happened, which the email tells.
    pub created_at: DateTime<Utc>,
    pub user_id: Option<i64>,
    pub email: Option<String>,
    pub locale: String,
    pub unsubscribe_secret: String,
}
