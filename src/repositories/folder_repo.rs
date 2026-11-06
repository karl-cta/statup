//! Folder repository: folders of services and the "who sees" they hand down.
//!
//! A service that follows its folder carries a copy of the folder's setting
//! (`services.visibility` and `service_access`), so the read rule never looks
//! at folders. [`copy_setting`] is the only code that writes those copies, for
//! groups (`service_access`) and for single members (`service_member_access`).

use std::collections::HashMap;

use sqlx::Sqlite;

use super::GrantTables;
use crate::db::DbPool;
use crate::models::{Grants, ServiceFolder, Visibility};

pub struct FolderRepository;

const FOLDER_COLUMNS: &str = "SELECT f.id, f.name, f.position, f.visibility FROM service_folders f";

/// A service moved into a folder, and whether it now follows it.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Joined {
    pub name: String,
    pub follows: bool,
}

impl FolderRepository {
    pub async fn create(pool: &DbPool, name: &str) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar(
            "INSERT INTO service_folders (name, position) \
             VALUES (?, (SELECT COALESCE(MAX(position), 0) + 1 FROM service_folders)) \
             RETURNING id",
        )
        .bind(name)
        .fetch_one(pool)
        .await
    }

    /// Creates a service already in the folder and following it, in one
    /// transaction: it is never seen outside the folder with another
    /// visibility. Returns its id.
    pub async fn create_service(
        pool: &DbPool,
        folder_id: i64,
        name: &str,
        slug: &str,
    ) -> Result<i64, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let id = sqlx::query_scalar(
            "INSERT INTO services (name, slug, position, folder_id, follows_folder) \
             VALUES (?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM services), ?, 1) \
             RETURNING id",
        )
        .bind(name)
        .bind(slug)
        .bind(folder_id)
        .fetch_one(&mut *tx)
        .await?;
        copy_setting(&mut tx, folder_id, Some(id)).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Whether another folder already has this name, ignoring case, accents
    /// included: SQLite folds the case of ASCII letters only. `except` is
    /// the folder being renamed.
    pub async fn name_taken(
        pool: &DbPool,
        name: &str,
        except: Option<i64>,
    ) -> Result<bool, sqlx::Error> {
        let folders: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM service_folders")
            .fetch_all(pool)
            .await?;
        let wanted = name.to_lowercase();
        Ok(folders
            .iter()
            .any(|(id, taken)| Some(*id) != except && taken.to_lowercase() == wanted))
    }

    pub async fn find(pool: &DbPool, id: i64) -> Result<Option<ServiceFolder>, sqlx::Error> {
        sqlx::query_as::<_, ServiceFolder>(&format!("{FOLDER_COLUMNS} WHERE f.id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// How many services follow the folder.
    pub async fn follower_count(pool: &DbPool, folder_id: i64) -> Result<usize, sqlx::Error> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM services WHERE folder_id = ? AND follows_folder = 1",
        )
        .bind(folder_id)
        .fetch_one(pool)
        .await?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    /// The name of each folder, by its id.
    pub async fn names(pool: &DbPool) -> Result<HashMap<i64, String>, sqlx::Error> {
        let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM service_folders")
            .fetch_all(pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    /// Every folder in the chosen order.
    pub async fn list(pool: &DbPool) -> Result<Vec<ServiceFolder>, sqlx::Error> {
        sqlx::query_as::<_, ServiceFolder>(&format!(
            "{FOLDER_COLUMNS} ORDER BY f.position, f.name COLLATE NOCASE, f.id"
        ))
        .fetch_all(pool)
        .await
    }

    pub async fn group_ids_of(pool: &DbPool, folder_id: i64) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT group_id FROM folder_access WHERE folder_id = ? ORDER BY group_id",
        )
        .bind(folder_id)
        .fetch_all(pool)
        .await
    }

    pub async fn member_ids_of(pool: &DbPool, folder_id: i64) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT user_id FROM folder_member_access WHERE folder_id = ? ORDER BY user_id",
        )
        .bind(folder_id)
        .fetch_all(pool)
        .await
    }

    /// Saves the name and who sees the folder together, from its panel, so
    /// that one is never written without the other, and hands the setting down
    /// to the services that follow the folder. The grants only count for
    /// [`Visibility::Groups`]; ids of groups that do not exist are dropped, and
    /// so are members who are not active readers. Returns false when no such
    /// folder exists.
    pub async fn save_settings(
        pool: &DbPool,
        folder_id: i64,
        name: &str,
        visibility: Visibility,
        grants: Grants<'_>,
    ) -> Result<bool, sqlx::Error> {
        // The write comes first: a deferred transaction that read first
        // could copy a value another writer has changed since.
        let mut tx = pool.begin().await?;
        let updated =
            sqlx::query("UPDATE service_folders SET name = ?, visibility = ? WHERE id = ?")
                .bind(name)
                .bind(visibility)
                .bind(folder_id)
                .execute(&mut *tx)
                .await?;
        if updated.rows_affected() == 0 {
            return Ok(false);
        }
        super::replace_grants(&mut tx, GrantTables::Folder, folder_id, visibility, grants).await?;
        propagate(&mut tx, folder_id).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Puts services in the folder, after the services already there.
    /// Unknown ids and services already in this folder are left alone, and
    /// an unknown folder moves nothing.
    ///
    /// A service follows the folder when it was visible to everyone or
    /// already had exactly the folder's setting: same visibility, same groups
    /// and same members. Any other restriction of
    /// its own is kept, so moving a service into a folder never shows it to
    /// more people. A follower moved from another folder carries that
    /// folder's copy, so the same rule applies to it.
    pub async fn add_services(
        pool: &DbPool,
        folder_id: i64,
        service_ids: &[i64],
    ) -> Result<Vec<Joined>, sqlx::Error> {
        let ids = serde_json::to_string(service_ids).unwrap_or_default();
        let mut tx = pool.begin().await?;
        let same_grants = [
            same_set("service_access", "folder_access", "group_id"),
            same_set("service_member_access", "folder_member_access", "user_id"),
        ]
        .join(" AND ");
        let joined = sqlx::query_as::<_, Joined>(&format!(
            "UPDATE services SET \
                 follows_folder = CASE \
                     WHEN visibility = 'everyone' THEN 1 \
                     WHEN visibility = (SELECT visibility FROM service_folders WHERE id = ?1) \
                         AND {same_grants} THEN 1 \
                     ELSE 0 END, \
                 folder_id = ?1, \
                 position = (SELECT COALESCE(MAX(position), 0) + 1 FROM services) \
                     + (SELECT key FROM json_each(?2) WHERE value = services.id LIMIT 1) \
             WHERE id IN (SELECT value FROM json_each(?2)) \
                 AND folder_id IS NOT ?1 \
                 AND EXISTS (SELECT 1 FROM service_folders WHERE id = ?1) \
             RETURNING name, follows_folder AS follows"
        ))
        .bind(folder_id)
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await?;
        propagate(&mut tx, folder_id).await?;
        tx.commit().await?;
        Ok(joined)
    }

    /// Makes a service in this folder follow it again: it takes the
    /// folder's setting. Returns false when the service is not in the folder.
    pub async fn follow(
        pool: &DbPool,
        folder_id: i64,
        service_id: i64,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let result =
            sqlx::query("UPDATE services SET follows_folder = 1 WHERE id = ? AND folder_id = ?")
                .bind(service_id)
                .bind(folder_id)
                .execute(&mut *tx)
                .await?;
        if result.rows_affected() == 0 {
            return Ok(false);
        }
        propagate(&mut tx, folder_id).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Takes the service out of the folder and keeps its copy of the
    /// setting: whoever saw it still does. Returns false when the service
    /// was not in this folder.
    pub async fn remove_service(
        pool: &DbPool,
        folder_id: i64,
        service_id: i64,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE services SET folder_id = NULL, follows_folder = 0 \
             WHERE id = ? AND folder_id = ?",
        )
        .bind(service_id)
        .bind(folder_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Returns false when no such folder exists. Its services stay, out of
    /// any folder, with the setting they had.
    pub async fn delete(pool: &DbPool, folder_id: i64) -> Result<bool, sqlx::Error> {
        let mut tx = pool.begin().await?;
        // The check on `follows_folder` would refuse ON DELETE SET NULL
        // for a follower, so the services leave first.
        sqlx::query("UPDATE services SET follows_folder = 0, folder_id = NULL WHERE folder_id = ?")
            .bind(folder_id)
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query("DELETE FROM service_folders WHERE id = ?")
            .bind(folder_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    /// Saves the order of the folders. Returns false unless the ids are
    /// exactly all the folders.
    pub async fn reorder(pool: &DbPool, ids: &[i64]) -> Result<bool, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let mut existing: Vec<i64> = sqlx::query_scalar("SELECT id FROM service_folders")
            .fetch_all(&mut *tx)
            .await?;
        let mut sent = ids.to_vec();
        existing.sort_unstable();
        sent.sort_unstable();
        if existing != sent {
            return Ok(false);
        }
        for (position, id) in (1_i64..).zip(ids) {
            sqlx::query("UPDATE service_folders SET position = ? WHERE id = ?")
                .bind(position)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(true)
    }
}

/// Whether the service's grants in `copy` are exactly the folder's (`?1`)
/// in `source`: none on one side that the other lacks.
fn same_set(copy: &str, source: &str, grantee: &str) -> String {
    format!(
        "NOT EXISTS (SELECT {grantee} FROM {copy} WHERE service_id = services.id \
                     EXCEPT SELECT {grantee} FROM {source} WHERE folder_id = ?1) \
         AND NOT EXISTS (SELECT {grantee} FROM {source} WHERE folder_id = ?1 \
                         EXCEPT SELECT {grantee} FROM {copy} WHERE service_id = services.id)"
    )
}

/// Copies the folder's setting to the services that follow it: its
/// visibility, then its groups and its members.
pub(super) async fn propagate(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    folder_id: i64,
) -> Result<(), sqlx::Error> {
    copy_setting(tx, folder_id, None).await
}

/// The copy of [`propagate`], to every follower or to one service only.
async fn copy_setting(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    folder_id: i64,
    only: Option<i64>,
) -> Result<(), sqlx::Error> {
    const FOLLOWERS: &str = "folder_id = ?1 AND follows_folder = 1 AND (?2 IS NULL OR id = ?2)";
    sqlx::query(&format!(
        "UPDATE services SET visibility = (SELECT visibility FROM service_folders WHERE id = ?1) \
         WHERE {FOLLOWERS}"
    ))
    .bind(folder_id)
    .bind(only)
    .execute(&mut **tx)
    .await?;
    let copies = [
        ("service_access", "folder_access", "group_id"),
        ("service_member_access", "folder_member_access", "user_id"),
    ];
    for (copy, source, grantee) in copies {
        sqlx::query(&format!(
            "DELETE FROM {copy} WHERE service_id IN (SELECT id FROM services WHERE {FOLLOWERS})"
        ))
        .bind(folder_id)
        .bind(only)
        .execute(&mut **tx)
        .await?;
        sqlx::query(&format!(
            "INSERT INTO {copy} (service_id, {grantee}) \
             SELECT s.id, a.{grantee} FROM services s \
             JOIN {source} a ON a.folder_id = s.folder_id \
             WHERE s.id IN (SELECT id FROM services WHERE {FOLLOWERS})"
        ))
        .bind(folder_id)
        .bind(only)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Role, Service};
    use crate::repositories::{GroupRepository, ServiceRepository, UserRepository};
    use crate::test_helpers::test_pool;

    async fn service(pool: &DbPool, name: &str) -> i64 {
        ServiceRepository::create(pool, name, &name.to_lowercase(), None, None, None)
            .await
            .unwrap()
            .id
    }

    async fn group(pool: &DbPool, name: &str) -> i64 {
        GroupRepository::create(pool, name).await.unwrap()
    }

    async fn find_service(pool: &DbPool, id: i64) -> Service {
        ServiceRepository::find_by_id(pool, id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn folder_with(pool: &DbPool, name: &str, visibility: Visibility, groups: &[i64]) -> i64 {
        let id = FolderRepository::create(pool, name).await.unwrap();
        FolderRepository::save_settings(
            pool,
            id,
            name,
            visibility,
            Grants {
                groups,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        id
    }

    async fn restrict(pool: &DbPool, service_id: i64, groups: &[i64]) {
        ServiceRepository::set_visibility(
            pool,
            service_id,
            Visibility::Groups,
            Grants {
                groups,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    async fn ordered_names(pool: &DbPool) -> Vec<String> {
        ServiceRepository::list_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|service| service.name)
            .collect()
    }

    /// Every follower carries exactly its folder's setting.
    async fn assert_followers_match(pool: &DbPool) {
        let followers = sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, folder_id FROM services WHERE follows_folder = 1",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        for (service_id, folder_id) in followers {
            let folder = FolderRepository::find(pool, folder_id)
                .await
                .unwrap()
                .unwrap();
            let service = find_service(pool, service_id).await;
            assert_eq!(service.visibility, folder.visibility);
            assert_eq!(
                ServiceRepository::group_ids_of(pool, service_id)
                    .await
                    .unwrap(),
                FolderRepository::group_ids_of(pool, folder_id)
                    .await
                    .unwrap()
            );
            assert_eq!(
                ServiceRepository::member_ids_of(pool, service_id)
                    .await
                    .unwrap(),
                FolderRepository::member_ids_of(pool, folder_id)
                    .await
                    .unwrap()
            );
        }
    }

    async fn reader(pool: &DbPool, email: &str) -> i64 {
        UserRepository::create(pool, email, "hash", "Someone", Role::Reader)
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn a_folder_is_created_renamed_and_counted() {
        let pool = test_pool().await;
        let first = FolderRepository::create(&pool, "Network").await.unwrap();
        let second = FolderRepository::create(&pool, "Apps").await.unwrap();
        let wiki = service(&pool, "Wiki").await;
        FolderRepository::add_services(&pool, second, &[wiki])
            .await
            .unwrap();

        let folders = FolderRepository::list(&pool).await.unwrap();
        assert_eq!(
            folders
                .iter()
                .map(|f| (f.id, f.position))
                .collect::<Vec<_>>(),
            vec![(first, 1), (second, 2)]
        );
        assert!(
            FolderRepository::save_settings(
                &pool,
                first,
                "Net",
                Visibility::Everyone,
                Grants::default()
            )
            .await
            .unwrap()
        );
        assert_eq!(
            FolderRepository::find(&pool, first)
                .await
                .unwrap()
                .unwrap()
                .name,
            "Net"
        );
        assert!(
            !FolderRepository::save_settings(
                &pool,
                999,
                "Nope",
                Visibility::Everyone,
                Grants::default()
            )
            .await
            .unwrap()
        );
        assert!(FolderRepository::find(&pool, 999).await.unwrap().is_none());
        assert_followers_match(&pool).await;
    }

    #[tokio::test]
    async fn a_folder_name_is_taken_whatever_its_case() {
        let pool = test_pool().await;
        let id = FolderRepository::create(&pool, "Équipe").await.unwrap();
        FolderRepository::create(&pool, "Apps").await.unwrap();

        assert!(
            FolderRepository::name_taken(&pool, "équipe", None)
                .await
                .unwrap()
        );
        assert!(
            FolderRepository::name_taken(&pool, "APPS", None)
                .await
                .unwrap()
        );
        assert!(
            !FolderRepository::name_taken(&pool, "équipe", Some(id))
                .await
                .unwrap()
        );
        assert!(
            !FolderRepository::name_taken(&pool, "Other", None)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn a_service_joins_a_folder_only_when_that_shows_it_to_no_more_people() {
        let pool = test_pool().await;
        let (g1, g2) = (group(&pool, "G1").await, group(&pool, "G2").await);
        let guarded = folder_with(&pool, "Guarded", Visibility::Groups, &[g1]).await;
        let open = folder_with(&pool, "Open", Visibility::Everyone, &[]).await;

        let public = service(&pool, "Public").await;
        let same = service(&pool, "Same").await;
        let other = service(&pool, "Other").await;
        restrict(&pool, same, &[g1]).await;
        restrict(&pool, other, &[g2]).await;

        let joined = FolderRepository::add_services(&pool, guarded, &[public, same])
            .await
            .unwrap();
        assert!(joined.iter().all(|j| j.follows));
        assert_eq!(joined.len(), 2);
        let moved = find_service(&pool, public).await;
        assert!(moved.follows_folder);
        assert_eq!(moved.visibility, Visibility::Groups);
        assert_eq!(
            ServiceRepository::group_ids_of(&pool, public)
                .await
                .unwrap(),
            vec![g1]
        );
        assert!(find_service(&pool, same).await.follows_folder);

        let joined = FolderRepository::add_services(&pool, open, &[other])
            .await
            .unwrap();
        assert_eq!(
            joined,
            vec![Joined {
                name: "Other".to_string(),
                follows: false
            }]
        );
        let kept = find_service(&pool, other).await;
        assert!(!kept.follows_folder);
        assert_eq!(kept.folder_id, Some(open));
        assert_eq!(kept.visibility, Visibility::Groups);
        assert_eq!(
            ServiceRepository::group_ids_of(&pool, other).await.unwrap(),
            vec![g2]
        );
        assert_followers_match(&pool).await;
    }

    #[tokio::test]
    async fn a_follower_moved_to_another_folder_follows_only_an_equal_one() {
        let pool = test_pool().await;
        let (g1, g2) = (group(&pool, "G1").await, group(&pool, "G2").await);
        let a = folder_with(&pool, "A", Visibility::Groups, &[g1]).await;
        let b = folder_with(&pool, "B", Visibility::Groups, &[g2]).await;
        let c = folder_with(&pool, "C", Visibility::Groups, &[g1]).await;
        let wiki = service(&pool, "Wiki").await;
        FolderRepository::add_services(&pool, a, &[wiki])
            .await
            .unwrap();

        let joined = FolderRepository::add_services(&pool, b, &[wiki])
            .await
            .unwrap();
        assert!(!joined[0].follows);
        let moved = find_service(&pool, wiki).await;
        assert_eq!((moved.folder_id, moved.follows_folder), (Some(b), false));
        assert_eq!(
            ServiceRepository::group_ids_of(&pool, wiki).await.unwrap(),
            vec![g1]
        );

        let joined = FolderRepository::add_services(&pool, c, &[wiki])
            .await
            .unwrap();
        assert!(joined[0].follows);
        assert_followers_match(&pool).await;
    }

    #[tokio::test]
    async fn a_folder_setting_reaches_its_followers_only() {
        let pool = test_pool().await;
        let (g1, g2) = (group(&pool, "G1").await, group(&pool, "G2").await);
        let folder = FolderRepository::create(&pool, "Apps").await.unwrap();
        let follower = service(&pool, "Follower").await;
        let own = service(&pool, "Own").await;
        FolderRepository::add_services(&pool, folder, &[follower])
            .await
            .unwrap();
        restrict(&pool, own, &[g2]).await;
        FolderRepository::add_services(&pool, folder, &[own])
            .await
            .unwrap();

        assert!(
            FolderRepository::save_settings(
                &pool,
                folder,
                "Apps",
                Visibility::Groups,
                Grants {
                    groups: &[g1, 999],
                    ..Default::default()
                }
            )
            .await
            .unwrap()
        );

        assert_eq!(
            FolderRepository::group_ids_of(&pool, folder).await.unwrap(),
            vec![g1]
        );
        let follower = find_service(&pool, follower).await;
        assert_eq!(follower.visibility, Visibility::Groups);
        let own_service = find_service(&pool, own).await;
        assert_eq!(own_service.visibility, Visibility::Groups);
        assert_eq!(
            ServiceRepository::group_ids_of(&pool, own).await.unwrap(),
            vec![g2]
        );

        assert!(
            FolderRepository::save_settings(
                &pool,
                folder,
                "Apps",
                Visibility::Members,
                Grants {
                    groups: &[g1],
                    ..Default::default()
                }
            )
            .await
            .unwrap()
        );
        assert_eq!(
            FolderRepository::group_ids_of(&pool, folder).await.unwrap(),
            Vec::<i64>::new()
        );
        assert_eq!(
            find_service(&pool, follower.id).await.visibility,
            Visibility::Members
        );
        assert_eq!(
            ServiceRepository::group_ids_of(&pool, follower.id)
                .await
                .unwrap(),
            Vec::<i64>::new()
        );
        assert_eq!(
            ServiceRepository::group_ids_of(&pool, own).await.unwrap(),
            vec![g2]
        );
        assert_followers_match(&pool).await;
    }

    #[tokio::test]
    async fn members_granted_on_a_folder_reach_its_followers_only() {
        let pool = test_pool().await;
        let g1 = group(&pool, "G1").await;
        let (ann, bob) = (
            reader(&pool, "ann@x.com").await,
            reader(&pool, "bob@x.com").await,
        );
        let folder = FolderRepository::create(&pool, "Apps").await.unwrap();
        let follower = service(&pool, "Follower").await;
        let own = service(&pool, "Own").await;
        FolderRepository::add_services(&pool, folder, &[follower])
            .await
            .unwrap();
        ServiceRepository::set_visibility(
            &pool,
            own,
            Visibility::Groups,
            Grants {
                groups: &[],
                members: &[bob],
            },
        )
        .await
        .unwrap();
        FolderRepository::add_services(&pool, folder, &[own])
            .await
            .unwrap();

        FolderRepository::save_settings(
            &pool,
            folder,
            "Apps",
            Visibility::Groups,
            Grants {
                groups: &[g1],
                members: &[ann],
            },
        )
        .await
        .unwrap();

        assert_eq!(
            FolderRepository::member_ids_of(&pool, folder)
                .await
                .unwrap(),
            vec![ann]
        );
        assert_eq!(
            ServiceRepository::member_ids_of(&pool, follower)
                .await
                .unwrap(),
            vec![ann]
        );
        assert_eq!(
            ServiceRepository::member_ids_of(&pool, own).await.unwrap(),
            vec![bob]
        );
        assert_followers_match(&pool).await;

        FolderRepository::save_settings(
            &pool,
            folder,
            "Apps",
            Visibility::Members,
            Grants::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            ServiceRepository::member_ids_of(&pool, follower)
                .await
                .unwrap(),
            Vec::<i64>::new()
        );
        assert_followers_match(&pool).await;
    }

    #[tokio::test]
    async fn a_service_follows_only_a_folder_with_the_same_groups_and_members() {
        let pool = test_pool().await;
        let g1 = group(&pool, "G1").await;
        let (ann, bob) = (
            reader(&pool, "ann@x.com").await,
            reader(&pool, "bob@x.com").await,
        );
        let folder = FolderRepository::create(&pool, "Guarded").await.unwrap();
        FolderRepository::save_settings(
            &pool,
            folder,
            "Guarded",
            Visibility::Groups,
            Grants {
                groups: &[g1],
                members: &[ann],
            },
        )
        .await
        .unwrap();
        let same = service(&pool, "Same").await;
        let other_member = service(&pool, "OtherMember").await;
        let no_member = service(&pool, "NoMember").await;
        for (id, members) in [
            (same, vec![ann]),
            (other_member, vec![bob]),
            (no_member, vec![]),
        ] {
            ServiceRepository::set_visibility(
                &pool,
                id,
                Visibility::Groups,
                Grants {
                    groups: &[g1],
                    members: &members,
                },
            )
            .await
            .unwrap();
        }

        let joined =
            FolderRepository::add_services(&pool, folder, &[same, other_member, no_member])
                .await
                .unwrap();

        let follows = |name: &str| joined.iter().find(|j| j.name == name).map(|j| j.follows);
        assert_eq!(follows("Same"), Some(true));
        assert_eq!(follows("OtherMember"), Some(false));
        assert_eq!(follows("NoMember"), Some(false));
        assert_eq!(
            ServiceRepository::member_ids_of(&pool, other_member)
                .await
                .unwrap(),
            vec![bob]
        );
        assert_followers_match(&pool).await;
    }

    #[tokio::test]
    async fn a_setting_made_on_a_follower_is_its_own() {
        let pool = test_pool().await;
        let g1 = group(&pool, "G1").await;
        let folder = FolderRepository::create(&pool, "Apps").await.unwrap();
        let wiki = service(&pool, "Wiki").await;
        FolderRepository::add_services(&pool, folder, &[wiki])
            .await
            .unwrap();

        restrict(&pool, wiki, &[g1]).await;
        FolderRepository::save_settings(
            &pool,
            folder,
            "Apps",
            Visibility::Everyone,
            Grants::default(),
        )
        .await
        .unwrap();

        let wiki = find_service(&pool, wiki).await;
        assert!(!wiki.follows_folder);
        assert_eq!(wiki.visibility, Visibility::Groups);
    }

    #[tokio::test]
    async fn a_service_follows_its_folder_again() {
        let pool = test_pool().await;
        let g1 = group(&pool, "G1").await;
        let folder = folder_with(&pool, "Guarded", Visibility::Groups, &[g1]).await;
        let other = FolderRepository::create(&pool, "Other").await.unwrap();
        let wiki = service(&pool, "Wiki").await;
        FolderRepository::add_services(&pool, folder, &[wiki])
            .await
            .unwrap();
        ServiceRepository::set_visibility(&pool, wiki, Visibility::Everyone, Grants::default())
            .await
            .unwrap();

        assert!(!FolderRepository::follow(&pool, other, wiki).await.unwrap());
        assert!(FolderRepository::follow(&pool, folder, wiki).await.unwrap());

        let wiki = find_service(&pool, wiki).await;
        assert!(wiki.follows_folder);
        assert_eq!(wiki.visibility, Visibility::Groups);
        assert_followers_match(&pool).await;
    }

    #[tokio::test]
    async fn an_unknown_folder_is_not_changed() {
        let pool = test_pool().await;
        let g1 = group(&pool, "G1").await;
        let wiki = service(&pool, "Wiki").await;

        assert!(
            !FolderRepository::save_settings(
                &pool,
                999,
                "Ghost",
                Visibility::Groups,
                Grants {
                    groups: &[g1],
                    ..Default::default()
                }
            )
            .await
            .unwrap()
        );
        assert_eq!(
            FolderRepository::add_services(&pool, 999, &[wiki])
                .await
                .unwrap(),
            Vec::<Joined>::new()
        );
        assert!(!FolderRepository::delete(&pool, 999).await.unwrap());
        let wiki = find_service(&pool, wiki).await;
        assert_eq!((wiki.folder_id, wiki.follows_folder), (None, false));
        assert_eq!(wiki.visibility, Visibility::Everyone);
    }

    #[tokio::test]
    async fn adding_services_passes_over_unknown_ids_and_members_of_the_folder() {
        let pool = test_pool().await;
        let folder = FolderRepository::create(&pool, "Apps").await.unwrap();
        let first = service(&pool, "First").await;
        let second = service(&pool, "Second").await;
        let last = service(&pool, "Last").await;

        let joined = FolderRepository::add_services(&pool, folder, &[first, 999])
            .await
            .unwrap();
        assert_eq!(joined.len(), 1);
        let position = find_position(&pool, first).await;

        let joined = FolderRepository::add_services(&pool, folder, &[first, second])
            .await
            .unwrap();
        assert_eq!(
            joined.iter().map(|j| j.name.as_str()).collect::<Vec<_>>(),
            vec!["Second"]
        );
        assert_eq!(find_position(&pool, first).await, position);
        assert!(find_position(&pool, second).await > position);
        assert_eq!(ordered_names(&pool).await, vec!["Last", "First", "Second"]);
        assert_eq!(find_service(&pool, last).await.folder_id, None);
    }

    #[tokio::test]
    async fn services_added_together_keep_the_order_they_were_sent_in() {
        let pool = test_pool().await;
        let folder = FolderRepository::create(&pool, "Network").await.unwrap();
        let alpha = service(&pool, "Alpha").await;
        let beta = service(&pool, "Beta").await;
        let gamma = service(&pool, "Gamma").await;

        FolderRepository::add_services(&pool, folder, &[gamma, alpha, beta])
            .await
            .unwrap();

        assert_eq!(ordered_names(&pool).await, vec!["Gamma", "Alpha", "Beta"]);
    }

    async fn find_position(pool: &DbPool, service_id: i64) -> i64 {
        sqlx::query_scalar("SELECT position FROM services WHERE id = ?")
            .bind(service_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn removing_a_service_keeps_the_setting_it_had() {
        let pool = test_pool().await;
        let g1 = group(&pool, "G1").await;
        let folder = folder_with(&pool, "Guarded", Visibility::Groups, &[g1]).await;
        let other = FolderRepository::create(&pool, "Other").await.unwrap();
        let wiki = service(&pool, "Wiki").await;
        FolderRepository::add_services(&pool, folder, &[wiki])
            .await
            .unwrap();

        assert!(
            !FolderRepository::remove_service(&pool, other, wiki)
                .await
                .unwrap()
        );
        assert!(find_service(&pool, wiki).await.follows_folder);
        assert!(
            FolderRepository::remove_service(&pool, folder, wiki)
                .await
                .unwrap()
        );

        let wiki = find_service(&pool, wiki).await;
        assert_eq!((wiki.folder_id, wiki.follows_folder), (None, false));
        assert_eq!(wiki.visibility, Visibility::Groups);
        assert_eq!(
            ServiceRepository::group_ids_of(&pool, wiki.id)
                .await
                .unwrap(),
            vec![g1]
        );
    }

    #[tokio::test]
    async fn deleting_a_folder_with_followers_keeps_the_services_and_their_setting() {
        let pool = test_pool().await;
        let g1 = group(&pool, "G1").await;
        let folder = folder_with(&pool, "Guarded", Visibility::Groups, &[g1]).await;
        let wiki = service(&pool, "Wiki").await;
        FolderRepository::add_services(&pool, folder, &[wiki])
            .await
            .unwrap();

        assert!(FolderRepository::delete(&pool, folder).await.unwrap());

        let wiki = find_service(&pool, wiki).await;
        assert_eq!((wiki.folder_id, wiki.follows_folder), (None, false));
        assert_eq!(wiki.visibility, Visibility::Groups);
        assert_eq!(
            ServiceRepository::group_ids_of(&pool, wiki.id)
                .await
                .unwrap(),
            vec![g1]
        );
        assert!(
            FolderRepository::find(&pool, folder)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            FolderRepository::group_ids_of(&pool, folder).await.unwrap(),
            Vec::<i64>::new()
        );
    }

    #[tokio::test]
    async fn deleting_a_group_takes_it_out_of_folders_and_their_followers() {
        let pool = test_pool().await;
        let (g1, g2) = (group(&pool, "G1").await, group(&pool, "G2").await);
        let folder = folder_with(&pool, "Guarded", Visibility::Groups, &[g1, g2]).await;
        let wiki = service(&pool, "Wiki").await;
        FolderRepository::add_services(&pool, folder, &[wiki])
            .await
            .unwrap();

        GroupRepository::delete(&pool, g1).await.unwrap();

        assert_eq!(
            FolderRepository::group_ids_of(&pool, folder).await.unwrap(),
            vec![g2]
        );
        assert_eq!(
            ServiceRepository::group_ids_of(&pool, wiki).await.unwrap(),
            vec![g2]
        );
        assert_followers_match(&pool).await;
    }

    #[tokio::test]
    async fn a_seeded_service_has_no_folder_and_the_schema_guards_followers() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO services (name, slug) VALUES ('Seeded', 'seeded')")
            .execute(&pool)
            .await
            .unwrap();
        let seeded: i64 = sqlx::query_scalar("SELECT id FROM services WHERE slug = 'seeded'")
            .fetch_one(&pool)
            .await
            .unwrap();

        let seeded_service = find_service(&pool, seeded).await;
        assert_eq!(
            (seeded_service.folder_id, seeded_service.follows_folder),
            (None, false)
        );
        let refused = sqlx::query("UPDATE services SET follows_folder = 1 WHERE id = ?")
            .bind(seeded)
            .execute(&pool)
            .await;
        assert!(refused.is_err());
    }

    #[tokio::test]
    async fn services_list_without_folder_first_then_folder_by_folder() {
        let pool = test_pool().await;
        let (a, b) = (
            FolderRepository::create(&pool, "A").await.unwrap(),
            FolderRepository::create(&pool, "B").await.unwrap(),
        );
        let ids = [
            service(&pool, "In-b-1").await,
            service(&pool, "Loose-1").await,
            service(&pool, "In-a-1").await,
            service(&pool, "In-b-2").await,
            service(&pool, "Loose-2").await,
        ];
        FolderRepository::add_services(&pool, b, &[ids[3]])
            .await
            .unwrap();
        FolderRepository::add_services(&pool, b, &[ids[0]])
            .await
            .unwrap();
        FolderRepository::add_services(&pool, a, &[ids[2]])
            .await
            .unwrap();

        assert_eq!(
            ordered_names(&pool).await,
            vec!["Loose-1", "Loose-2", "In-a-1", "In-b-2", "In-b-1"]
        );
        assert!(FolderRepository::reorder(&pool, &[b, a]).await.unwrap());
        assert_eq!(
            ordered_names(&pool).await,
            vec!["Loose-1", "Loose-2", "In-b-2", "In-b-1", "In-a-1"]
        );
        for ids in [vec![a], vec![a, b, 999], vec![a, a]] {
            assert!(!FolderRepository::reorder(&pool, &ids).await.unwrap());
        }
    }

    #[tokio::test]
    async fn the_read_rule_does_not_look_at_folders() {
        let pool = test_pool().await;
        let g1 = group(&pool, "G1").await;
        let folder = folder_with(&pool, "Guarded", Visibility::Groups, &[g1]).await;
        let wiki = service(&pool, "Wiki").await;
        FolderRepository::add_services(&pool, folder, &[wiki])
            .await
            .unwrap();

        sqlx::query("UPDATE service_folders SET visibility = 'everyone'")
            .execute(&pool)
            .await
            .unwrap();

        let scope = crate::services::scope_of(&pool, None).await.unwrap();
        assert!(!scope.sees_service(wiki));
    }
}
