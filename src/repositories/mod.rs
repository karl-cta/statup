//! Data access: the SQL queries of each model.

mod api_key_repo;
mod dashboard_layout_repo;
mod event_repo;
mod event_review_repo;
mod event_template_repo;
mod folder_repo;
mod group_repo;
mod icon_repo;
mod journal_repo;
mod member_notice_repo;
mod notification_repo;
mod outage_repo;
mod password_reset_repo;
mod series_repo;
mod service_repo;
mod settings_repo;
mod subscriber_delivery_repo;
mod subscriber_repo;
mod subscription_request_repo;
mod user_repo;

pub use api_key_repo::*;
pub use dashboard_layout_repo::*;
pub use event_repo::*;
pub use event_review_repo::*;
pub use event_template_repo::*;
pub use folder_repo::*;
pub use group_repo::*;
pub use icon_repo::*;
pub use journal_repo::*;
pub use member_notice_repo::*;
pub use notification_repo::*;
pub use outage_repo::*;
pub use password_reset_repo::*;
pub use series_repo::*;
pub use service_repo::*;
pub use settings_repo::*;
pub use subscriber_delivery_repo::*;
pub use subscriber_repo::*;
pub use subscription_request_repo::*;
pub use user_repo::*;

use crate::models::{Grants, Visibility};

/// The access tables of a service or of a folder: its groups, its members.
#[derive(Clone, Copy)]
pub(crate) enum GrantTables {
    Service,
    Folder,
}

impl GrantTables {
    const fn names(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Service => ("service_access", "service_member_access", "service_id"),
            Self::Folder => ("folder_access", "folder_member_access", "folder_id"),
        }
    }
}

/// Replaces who a service or folder is opened to. The grants only count for
/// [`Visibility::Groups`]: groups that exist and members who are active
/// readers, since publishers and admins see everything already.
pub(crate) async fn replace_grants(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    tables: GrantTables,
    owner_id: i64,
    visibility: Visibility,
    grants: Grants<'_>,
) -> Result<(), sqlx::Error> {
    let (groups_table, members_table, owner) = tables.names();
    for table in [groups_table, members_table] {
        sqlx::query(&format!("DELETE FROM {table} WHERE {owner} = ?"))
            .bind(owner_id)
            .execute(&mut **tx)
            .await?;
    }
    if visibility != Visibility::Groups {
        return Ok(());
    }
    let groups = serde_json::to_string(grants.groups).unwrap_or_default();
    sqlx::query(&format!(
        "INSERT INTO {groups_table} ({owner}, group_id) \
         SELECT ?, id FROM member_groups WHERE id IN (SELECT value FROM json_each(?))"
    ))
    .bind(owner_id)
    .bind(&groups)
    .execute(&mut **tx)
    .await?;
    let members = serde_json::to_string(grants.members).unwrap_or_default();
    sqlx::query(&format!(
        "INSERT INTO {members_table} ({owner}, user_id) SELECT ?, id FROM users \
         WHERE id IN (SELECT value FROM json_each(?)) AND role = 'reader' AND is_active = 1"
    ))
    .bind(owner_id)
    .bind(&members)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// A search for `50%` looks for the sign, not for anything.
pub(crate) fn escape_like(words: &str) -> String {
    words
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
