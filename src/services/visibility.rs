//! Which services a viewer may see.

use crate::db::DbPool;
use crate::models::{Scope, User};
use crate::repositories::ServiceRepository;

/// The scope of a viewer: `None` is a visitor who is not signed in.
/// Publishers and admins see everything. When nothing is hidden from the
/// viewer the scope is [`Scope::All`], so an instance without restricted
/// services keeps running unfiltered queries.
pub async fn scope_of(pool: &DbPool, user: Option<&User>) -> Result<Scope, sqlx::Error> {
    if user.is_some_and(|user| user.role.can_publish()) {
        return Ok(Scope::All);
    }
    let (ids, total) = ServiceRepository::visible_ids(pool, user.map(|user| user.id)).await?;
    Ok(narrowed(ids, total))
}

/// What every signed-in reader sees, whatever their groups: the services
/// open to everyone and to members. The status embed reads the page this
/// way when it holds its private link.
pub async fn member_scope(pool: &DbPool) -> Result<Scope, sqlx::Error> {
    let (ids, total) = ServiceRepository::member_visible_ids(pool).await?;
    Ok(narrowed(ids, total))
}

fn narrowed(ids: Vec<i64>, total: usize) -> Scope {
    if ids.len() == total {
        return Scope::All;
    }
    Scope::visible(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Grants, Role, Visibility};
    use crate::repositories::{GroupRepository, UserRepository};
    use crate::test_helpers::test_pool;

    async fn user(pool: &DbPool, email: &str, role: Role) -> User {
        UserRepository::create(pool, email, "hash", "Someone", role)
            .await
            .unwrap()
    }

    async fn service(pool: &DbPool, name: &str, visibility: Visibility, groups: &[i64]) -> i64 {
        let id = ServiceRepository::create(pool, name, &name.to_lowercase(), None, None, None)
            .await
            .unwrap()
            .id;
        ServiceRepository::set_visibility(
            pool,
            id,
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

    #[tokio::test]
    async fn an_instance_without_restricted_services_hides_nothing() {
        let pool = test_pool().await;
        service(&pool, "Wiki", Visibility::Everyone, &[]).await;
        let reader = user(&pool, "r@x.com", Role::Reader).await;

        assert_eq!(scope_of(&pool, None).await.unwrap(), Scope::All);
        assert_eq!(scope_of(&pool, Some(&reader)).await.unwrap(), Scope::All);
    }

    #[tokio::test]
    async fn a_service_inserted_like_the_demo_seed_is_seen_by_everyone() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO services (name, slug) VALUES ('Wiki', 'wiki')")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(scope_of(&pool, None).await.unwrap(), Scope::All);
    }

    #[tokio::test]
    async fn publishers_and_admins_see_everything() {
        let pool = test_pool().await;
        service(&pool, "Secret", Visibility::Groups, &[]).await;
        for (email, role) in [("p@x.com", Role::Publisher), ("a@x.com", Role::Admin)] {
            let who = user(&pool, email, role).await;
            assert_eq!(scope_of(&pool, Some(&who)).await.unwrap(), Scope::All);
        }
    }

    #[tokio::test]
    async fn readers_and_visitors_see_what_their_access_opens() {
        let pool = test_pool().await;
        let group = GroupRepository::create(&pool, "Support").await.unwrap();
        let open = service(&pool, "Open", Visibility::Everyone, &[]).await;
        let members = service(&pool, "Members", Visibility::Members, &[]).await;
        service(&pool, "Grouped", Visibility::Groups, &[group]).await;
        let inside = user(&pool, "in@x.com", Role::Reader).await;
        let outside = user(&pool, "out@x.com", Role::Reader).await;
        GroupRepository::add_members(&pool, group, &[inside.id])
            .await
            .unwrap();

        assert_eq!(
            scope_of(&pool, None).await.unwrap(),
            Scope::visible(vec![open])
        );
        // Nothing is hidden from a member of the group: no filter at all.
        assert_eq!(scope_of(&pool, Some(&inside)).await.unwrap(), Scope::All);
        assert_eq!(
            scope_of(&pool, Some(&outside)).await.unwrap(),
            Scope::visible(vec![open, members])
        );
    }

    #[tokio::test]
    async fn the_member_scope_never_opens_a_group() {
        let pool = test_pool().await;
        let group = GroupRepository::create(&pool, "Support").await.unwrap();
        let open = service(&pool, "Open", Visibility::Everyone, &[]).await;
        let members = service(&pool, "Members", Visibility::Members, &[]).await;
        assert_eq!(member_scope(&pool).await.unwrap(), Scope::All);

        service(&pool, "Grouped", Visibility::Groups, &[group]).await;
        assert_eq!(
            member_scope(&pool).await.unwrap(),
            Scope::visible(vec![open, members])
        );
    }

    #[tokio::test]
    async fn a_service_left_without_a_group_is_for_publishers_and_admins_only() {
        let pool = test_pool().await;
        let group = GroupRepository::create(&pool, "Support").await.unwrap();
        let grouped = service(&pool, "Grouped", Visibility::Groups, &[group]).await;
        let reader = user(&pool, "r@x.com", Role::Reader).await;
        let publisher = user(&pool, "p@x.com", Role::Publisher).await;
        GroupRepository::add_members(&pool, group, &[reader.id])
            .await
            .unwrap();
        GroupRepository::delete(&pool, group).await.unwrap();

        let scope = scope_of(&pool, Some(&reader)).await.unwrap();
        assert!(!scope.sees_service(grouped));
        assert_eq!(scope_of(&pool, Some(&publisher)).await.unwrap(), Scope::All);
    }
}
