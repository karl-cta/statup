//! Group repository: named sets of members, used to choose who sees a service.

use crate::db::DbPool;
use crate::models::{GroupMember, MemberGroup};

pub struct GroupRepository;

/// A group with its count of active members.
const GROUP_COLUMNS: &str = "SELECT g.id, g.name, \
     (SELECT COUNT(*) FROM group_members gm JOIN users u ON u.id = gm.user_id \
      WHERE gm.group_id = g.id AND u.is_active = 1) AS member_count \
     FROM member_groups g";

impl GroupRepository {
    pub async fn create(pool: &DbPool, name: &str) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("INSERT INTO member_groups (name) VALUES (?) RETURNING id")
            .bind(name)
            .fetch_one(pool)
            .await
    }

    /// Whether another group already has this name, ignoring case, accents
    /// included: SQLite folds the case of ASCII letters only, so "Équipe"
    /// and "équipe" are compared here. `except` is the group being renamed.
    pub async fn name_taken(
        pool: &DbPool,
        name: &str,
        except: Option<i64>,
    ) -> Result<bool, sqlx::Error> {
        let groups: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM member_groups")
            .fetch_all(pool)
            .await?;
        let wanted = name.to_lowercase();
        Ok(groups
            .iter()
            .any(|(id, taken)| Some(*id) != except && taken.to_lowercase() == wanted))
    }

    /// Returns false when no such group exists.
    pub async fn rename(pool: &DbPool, id: i64, name: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("UPDATE member_groups SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Returns false when no such group exists. Memberships and service
    /// access to the group go with it.
    pub async fn delete(pool: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM member_groups WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Every group by name, with its count of active members.
    pub async fn list(pool: &DbPool) -> Result<Vec<MemberGroup>, sqlx::Error> {
        sqlx::query_as::<_, MemberGroup>(&format!("{GROUP_COLUMNS} ORDER BY g.name COLLATE NOCASE"))
            .fetch_all(pool)
            .await
    }

    pub async fn find(pool: &DbPool, id: i64) -> Result<Option<MemberGroup>, sqlx::Error> {
        sqlx::query_as::<_, MemberGroup>(&format!("{GROUP_COLUMNS} WHERE g.id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// How many of these ids name a group that exists.
    pub async fn count_of(pool: &DbPool, ids: &[i64]) -> Result<i64, sqlx::Error> {
        let ids = serde_json::to_string(ids).unwrap_or_default();
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM member_groups WHERE id IN (SELECT value FROM json_each(?))",
        )
        .bind(&ids)
        .fetch_one(pool)
        .await
    }

    /// The members of a group by name, disabled ones included.
    pub async fn members(pool: &DbPool, group_id: i64) -> Result<Vec<GroupMember>, sqlx::Error> {
        sqlx::query_as::<_, GroupMember>(
            "SELECT u.id, u.display_name, u.email, u.is_active FROM users u \
             JOIN group_members gm ON gm.user_id = u.id \
             WHERE gm.group_id = ? ORDER BY u.display_name COLLATE NOCASE, u.id",
        )
        .bind(group_id)
        .fetch_all(pool)
        .await
    }

    /// The active readers who are not in the group, by name: those it can
    /// take in. Editors and administrators see every service already.
    pub async fn outsiders(pool: &DbPool, group_id: i64) -> Result<Vec<GroupMember>, sqlx::Error> {
        sqlx::query_as::<_, GroupMember>(
            "SELECT u.id, u.display_name, u.email, u.is_active FROM users u \
             WHERE u.is_active = 1 AND u.role = 'reader' \
             AND NOT EXISTS (SELECT 1 FROM group_members gm \
                   WHERE gm.group_id = ? AND gm.user_id = u.id) \
             ORDER BY u.display_name COLLATE NOCASE, u.id",
        )
        .bind(group_id)
        .fetch_all(pool)
        .await
    }

    /// Puts readers in the group and returns how many joined. Members
    /// already in it, editors, administrators, disabled ones and ids of no
    /// member are passed over, as the panel that offers them does.
    pub async fn add_members(
        pool: &DbPool,
        group_id: i64,
        user_ids: &[i64],
    ) -> Result<u64, sqlx::Error> {
        let ids = serde_json::to_string(user_ids).unwrap_or_default();
        let result = sqlx::query(
            "INSERT OR IGNORE INTO group_members (group_id, user_id) \
             SELECT ?, id FROM users \
             WHERE is_active = 1 AND role = 'reader' \
             AND id IN (SELECT value FROM json_each(?))",
        )
        .bind(group_id)
        .bind(&ids)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Returns false when the member was not in the group.
    pub async fn remove_member(
        pool: &DbPool,
        group_id: i64,
        user_id: i64,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM group_members WHERE group_id = ? AND user_id = ?")
            .bind(group_id)
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// The active readers of every group, as (group, reader) pairs: what a
    /// group opens a restricted service to.
    pub async fn reader_ids(pool: &DbPool) -> Result<Vec<(i64, i64)>, sqlx::Error> {
        sqlx::query_as(
            "SELECT gm.group_id, gm.user_id FROM group_members gm \
             JOIN users u ON u.id = gm.user_id \
             WHERE u.role = 'reader' AND u.is_active = 1 \
             ORDER BY gm.group_id, gm.user_id",
        )
        .fetch_all(pool)
        .await
    }

    /// The restricted services opened to this group on their own, not
    /// through the folder they follow.
    pub async fn own_service_ids(pool: &DbPool, group_id: i64) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT a.service_id FROM service_access a \
             JOIN services s ON s.id = a.service_id \
             WHERE a.group_id = ? AND s.visibility = 'groups' AND s.follows_folder = 0",
        )
        .bind(group_id)
        .fetch_all(pool)
        .await
    }

    /// The restricted folders opened to this group.
    pub async fn folder_ids(pool: &DbPool, group_id: i64) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT a.folder_id FROM folder_access a \
             JOIN service_folders f ON f.id = a.folder_id \
             WHERE a.group_id = ? AND f.visibility = 'groups'",
        )
        .bind(group_id)
        .fetch_all(pool)
        .await
    }

    /// Opens restricted folders and services to the group and returns those
    /// it opened. Only what is restricted is opened, so nothing is ever hidden
    /// from anyone; a service that follows its folder is reached through the
    /// folder, which its followers then copy.
    pub async fn open_to(
        pool: &DbPool,
        group_id: i64,
        folder_ids: &[i64],
        service_ids: &[i64],
    ) -> Result<(Vec<i64>, Vec<i64>), sqlx::Error> {
        let folders = serde_json::to_string(folder_ids).unwrap_or_default();
        let services = serde_json::to_string(service_ids).unwrap_or_default();
        // The writes come first: a deferred transaction that read first
        // could copy a value another writer has changed since.
        let mut tx = pool.begin().await?;
        let opened_folders: Vec<i64> = sqlx::query_scalar(
            "INSERT INTO folder_access (folder_id, group_id) \
             SELECT f.id, g.id FROM service_folders f JOIN member_groups g ON g.id = ?1 \
             WHERE f.visibility = 'groups' AND f.id IN (SELECT value FROM json_each(?2)) \
             AND NOT EXISTS (SELECT 1 FROM folder_access a \
                             WHERE a.folder_id = f.id AND a.group_id = g.id) \
             RETURNING folder_id",
        )
        .bind(group_id)
        .bind(&folders)
        .fetch_all(&mut *tx)
        .await?;
        let opened_services: Vec<i64> = sqlx::query_scalar(
            "INSERT INTO service_access (service_id, group_id) \
             SELECT s.id, g.id FROM services s JOIN member_groups g ON g.id = ?1 \
             WHERE s.visibility = 'groups' AND s.follows_folder = 0 \
             AND s.id IN (SELECT value FROM json_each(?2)) \
             AND NOT EXISTS (SELECT 1 FROM service_access a \
                             WHERE a.service_id = s.id AND a.group_id = g.id) \
             RETURNING service_id",
        )
        .bind(group_id)
        .bind(&services)
        .fetch_all(&mut *tx)
        .await?;
        for folder_id in &opened_folders {
            super::folder_repo::propagate(&mut tx, *folder_id).await?;
        }
        tx.commit().await?;
        Ok((opened_folders, opened_services))
    }

    /// Takes the folder's access away from the group, and from the services
    /// that follow it. Returns false when it was not opened to the group.
    pub async fn close_folder(
        pool: &DbPool,
        group_id: i64,
        folder_id: i64,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let result = sqlx::query("DELETE FROM folder_access WHERE folder_id = ? AND group_id = ?")
            .bind(folder_id)
            .bind(group_id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Ok(false);
        }
        super::folder_repo::propagate(&mut tx, folder_id).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Takes a service's own access away from the group; a follower keeps
    /// its folder's. Returns false when it was not opened to the group.
    pub async fn close_service(
        pool: &DbPool,
        group_id: i64,
        service_id: i64,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM service_access WHERE service_id = ? AND group_id = ? \
             AND service_id IN (SELECT id FROM services WHERE follows_folder = 0)",
        )
        .bind(service_id)
        .bind(group_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// How many restricted services have this group as their only one and
    /// no member opened to by name: once it is deleted, only publishers and
    /// admins see them.
    pub async fn services_closed_by_deleting(
        pool: &DbPool,
        group_id: i64,
    ) -> Result<usize, sqlx::Error> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM services s \
             WHERE s.visibility = 'groups' \
             AND EXISTS (SELECT 1 FROM service_access a \
                         WHERE a.service_id = s.id AND a.group_id = ?) \
             AND (SELECT COUNT(*) FROM service_access a WHERE a.service_id = s.id) = 1 \
             AND NOT EXISTS (SELECT 1 FROM service_member_access m WHERE m.service_id = s.id)",
        )
        .bind(group_id)
        .fetch_one(pool)
        .await?;
        Ok(usize::try_from(count).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Grants, Role, Visibility};
    use crate::repositories::{ServiceRepository, UserRepository};
    use crate::test_helpers::test_pool;

    async fn member(pool: &DbPool, email: &str) -> i64 {
        UserRepository::create(pool, email, "hash", "Someone", Role::Reader)
            .await
            .unwrap()
            .id
    }

    async fn service(pool: &DbPool, name: &str) -> i64 {
        ServiceRepository::create(pool, name, &name.to_lowercase(), None, None, None)
            .await
            .unwrap()
            .id
    }

    async fn count(pool: &DbPool, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_group_is_created_renamed_and_deleted() {
        let pool = test_pool().await;
        let id = GroupRepository::create(&pool, "Support").await.unwrap();
        assert!(
            GroupRepository::rename(&pool, id, "Helpdesk")
                .await
                .unwrap()
        );
        assert_eq!(
            GroupRepository::list(&pool).await.unwrap()[0].name,
            "Helpdesk"
        );
        assert!(
            !GroupRepository::rename(&pool, id + 1, "Nope")
                .await
                .unwrap()
        );

        assert!(GroupRepository::delete(&pool, id).await.unwrap());
        assert!(!GroupRepository::delete(&pool, id).await.unwrap());
        assert_eq!(GroupRepository::list(&pool).await.unwrap(), Vec::new());
    }

    #[tokio::test]
    async fn the_schema_refuses_two_groups_with_the_same_name_ignoring_case() {
        let pool = test_pool().await;
        GroupRepository::create(&pool, "Support").await.unwrap();
        assert!(GroupRepository::create(&pool, "SUPPORT").await.is_err());
    }

    #[tokio::test]
    async fn a_name_is_taken_ignoring_case_except_by_the_group_renamed() {
        let pool = test_pool().await;
        let id = GroupRepository::create(&pool, "Support").await.unwrap();
        assert!(
            GroupRepository::name_taken(&pool, "support", None)
                .await
                .unwrap()
        );
        assert!(
            !GroupRepository::name_taken(&pool, "support", Some(id))
                .await
                .unwrap()
        );
        assert!(
            GroupRepository::name_taken(&pool, "support", Some(id + 1))
                .await
                .unwrap()
        );
        assert!(
            !GroupRepository::name_taken(&pool, "Sales", None)
                .await
                .unwrap()
        );
        GroupRepository::create(&pool, "Équipe").await.unwrap();
        assert!(
            GroupRepository::name_taken(&pool, "équipe", None)
                .await
                .unwrap(),
            "an accented capital is the same letter"
        );
    }

    #[tokio::test]
    async fn deleting_a_group_removes_its_memberships_and_service_access() {
        let pool = test_pool().await;
        let group = GroupRepository::create(&pool, "Support").await.unwrap();
        let user = member(&pool, "a@b.com").await;
        let svc = service(&pool, "Wiki").await;
        GroupRepository::add_members(&pool, group, &[user])
            .await
            .unwrap();
        ServiceRepository::set_visibility(
            &pool,
            svc,
            Visibility::Groups,
            Grants {
                groups: &[group],
                ..Default::default()
            },
        )
        .await
        .unwrap();

        GroupRepository::delete(&pool, group).await.unwrap();
        assert_eq!(count(&pool, "group_members").await, 0);
        assert_eq!(count(&pool, "service_access").await, 0);
    }

    #[tokio::test]
    async fn the_listing_counts_active_members_only() {
        let pool = test_pool().await;
        let group = GroupRepository::create(&pool, "support").await.unwrap();
        GroupRepository::create(&pool, "Admins").await.unwrap();
        let active = member(&pool, "a@b.com").await;
        let gone = member(&pool, "c@d.com").await;
        GroupRepository::add_members(&pool, group, &[active, gone])
            .await
            .unwrap();
        UserRepository::set_active(&pool, gone, false)
            .await
            .unwrap();

        let groups = GroupRepository::list(&pool).await.unwrap();
        let summary: Vec<(&str, i64)> = groups
            .iter()
            .map(|g| (g.name.as_str(), g.member_count))
            .collect();
        assert_eq!(summary, vec![("Admins", 0), ("support", 1)]);
    }

    #[tokio::test]
    async fn members_join_once_and_leave_and_outsiders_are_the_active_rest() {
        let pool = test_pool().await;
        let group = GroupRepository::create(&pool, "Support").await.unwrap();
        let ann = member(&pool, "ann@b.com").await;
        let bob = member(&pool, "bob@b.com").await;
        let gone = member(&pool, "gone@b.com").await;
        UserRepository::set_active(&pool, gone, false)
            .await
            .unwrap();

        let added = GroupRepository::add_members(&pool, group, &[ann, ann, 999])
            .await
            .unwrap();
        assert_eq!(
            added, 1,
            "a member joins once, an unknown id is passed over"
        );
        assert_eq!(
            GroupRepository::add_members(&pool, group, &[ann])
                .await
                .unwrap(),
            0
        );
        let ids = |list: Vec<GroupMember>| list.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(
            ids(GroupRepository::members(&pool, group).await.unwrap()),
            vec![ann]
        );
        assert_eq!(
            ids(GroupRepository::outsiders(&pool, group).await.unwrap()),
            vec![bob],
            "a disabled member is not offered"
        );
        assert_eq!(
            GroupRepository::add_members(&pool, group, &[gone])
                .await
                .unwrap(),
            0,
            "nor taken in"
        );

        assert!(
            GroupRepository::remove_member(&pool, group, ann)
                .await
                .unwrap()
        );
        assert!(
            !GroupRepository::remove_member(&pool, group, ann)
                .await
                .unwrap()
        );
        assert_eq!(
            GroupRepository::find(&pool, group)
                .await
                .unwrap()
                .map(|g| g.member_count),
            Some(0)
        );
    }

    #[tokio::test]
    async fn deleting_a_group_closes_only_the_services_it_alone_opens() {
        let pool = test_pool().await;
        let support = GroupRepository::create(&pool, "Support").await.unwrap();
        let sales = GroupRepository::create(&pool, "Sales").await.unwrap();
        let only = service(&pool, "Only").await;
        let shared = service(&pool, "Shared").await;
        let members = service(&pool, "Members").await;
        let elsewhere = service(&pool, "Elsewhere").await;
        let groups = Visibility::Groups;
        ServiceRepository::set_visibility(
            &pool,
            only,
            groups,
            Grants {
                groups: &[support],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        ServiceRepository::set_visibility(
            &pool,
            shared,
            groups,
            Grants {
                groups: &[support, sales],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        ServiceRepository::set_visibility(
            &pool,
            members,
            Visibility::Members,
            Grants {
                groups: &[support],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        ServiceRepository::set_visibility(
            &pool,
            elsewhere,
            groups,
            Grants {
                groups: &[sales],
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let closed = GroupRepository::services_closed_by_deleting(&pool, support)
            .await
            .unwrap();
        assert_eq!(closed, 1, "only the service it alone opens");
    }

    #[tokio::test]
    async fn a_service_also_opened_to_a_member_is_not_closed_by_deleting_the_group() {
        let pool = test_pool().await;
        let support = GroupRepository::create(&pool, "Support").await.unwrap();
        let reader = member(&pool, "r@x.com").await;
        let alone = service(&pool, "Alone").await;
        let with_member = service(&pool, "WithMember").await;
        ServiceRepository::set_visibility(
            &pool,
            alone,
            Visibility::Groups,
            Grants {
                groups: &[support],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        ServiceRepository::set_visibility(
            &pool,
            with_member,
            Visibility::Groups,
            Grants {
                groups: &[support],
                members: &[reader],
            },
        )
        .await
        .unwrap();

        let closed = GroupRepository::services_closed_by_deleting(&pool, support)
            .await
            .unwrap();
        assert_eq!(closed, 1, "the one with a member stays open to them");
    }

    /// A folder and a service, both restricted to nobody yet.
    async fn restricted_pair(pool: &DbPool) -> (i64, i64) {
        let folder = crate::repositories::FolderRepository::create(pool, "Finances")
            .await
            .unwrap();
        crate::repositories::FolderRepository::save_settings(
            pool,
            folder,
            "Finances",
            Visibility::Groups,
            Grants {
                groups: &[],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let svc = service(pool, "Wiki").await;
        ServiceRepository::set_visibility(
            pool,
            svc,
            Visibility::Groups,
            Grants {
                groups: &[],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        (folder, svc)
    }

    #[tokio::test]
    async fn opening_to_an_unknown_group_opens_nothing() {
        let pool = test_pool().await;
        let (folder, svc) = restricted_pair(&pool).await;

        let opened = GroupRepository::open_to(&pool, 999, &[folder], &[svc])
            .await
            .unwrap();

        assert_eq!(opened, (Vec::new(), Vec::new()));
        assert_eq!(count(&pool, "folder_access").await, 0);
        assert_eq!(count(&pool, "service_access").await, 0);
    }

    #[tokio::test]
    async fn opening_twice_to_the_same_group_opens_once() {
        let pool = test_pool().await;
        let group = GroupRepository::create(&pool, "Support").await.unwrap();
        let (folder, svc) = restricted_pair(&pool).await;

        let first = GroupRepository::open_to(&pool, group, &[folder], &[svc])
            .await
            .unwrap();
        let second = GroupRepository::open_to(&pool, group, &[folder], &[svc])
            .await
            .unwrap();

        assert_eq!(first, (vec![folder], vec![svc]));
        assert_eq!(second, (Vec::new(), Vec::new()));
        assert_eq!(count(&pool, "folder_access").await, 1);
        assert_eq!(count(&pool, "service_access").await, 1);
    }
}
