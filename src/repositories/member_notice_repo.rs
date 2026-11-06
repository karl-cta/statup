//! Member notices: written for one member or for every member of some roles,
//! listed, counted and marked read.

use chrono::{DateTime, Utc};

use crate::clock;
use crate::db::DbPool;
use crate::models::{MemberNotice, NoticeKind, NoticeSubject, Role};

const NOTICE_COLUMNS: &str = "n.kind, n.event_id, n.service_id, n.due_at, \
     n.others, n.created_at, n.read_at, e.title AS event_title, s.name AS service_name, \
     c.name AS channel_name";

pub struct MemberNoticeRepository;

impl MemberNoticeRepository {
    pub async fn add_for_member(
        pool: &DbPool,
        user_id: i64,
        kind: NoticeKind,
        subject: NoticeSubject,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO member_notices (user_id, kind, event_id, service_id, channel_id, due_at, \
             others) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(kind)
        .bind(subject.event_id)
        .bind(subject.service_id)
        .bind(subject.channel_id)
        .bind(subject.due_at.map(clock::db))
        .bind(subject.others)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// One notice for each active member holding one of `roles`, unless the
    /// same one still waits unread: a service that keeps falling, or a
    /// destination that keeps refusing, tells each member once, dated from
    /// the latest time.
    pub async fn add_for_roles(
        pool: &DbPool,
        roles: &[Role],
        kind: NoticeKind,
        subject: NoticeSubject,
    ) -> Result<(), sqlx::Error> {
        let roles: Vec<&str> = roles.iter().map(|role| role.as_str()).collect();
        let roles = serde_json::to_string(&roles).unwrap_or_default();
        let mut tx = pool.begin().await?;
        // The same news, unread, is brought up to now rather than doubled.
        sqlx::query(
            "UPDATE member_notices SET created_at = datetime('now'), others = ?6 \
             WHERE kind = ?1 AND read_at IS NULL AND event_id IS ?2 AND service_id IS ?3 \
             AND channel_id IS ?4 AND user_id IN (SELECT id FROM users \
             WHERE is_active = 1 AND role IN (SELECT value FROM json_each(?5)))",
        )
        .bind(kind)
        .bind(subject.event_id)
        .bind(subject.service_id)
        .bind(subject.channel_id)
        .bind(&roles)
        .bind(subject.others)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO member_notices (user_id, kind, event_id, service_id, channel_id, due_at, \
             others) SELECT u.id, ?1, ?2, ?3, ?4, ?5, ?7 FROM users u \
             WHERE u.is_active = 1 AND u.role IN (SELECT value FROM json_each(?6)) \
             AND NOT EXISTS (SELECT 1 FROM member_notices n WHERE n.user_id = u.id \
             AND n.kind = ?1 AND n.read_at IS NULL AND n.event_id IS ?2 \
             AND n.service_id IS ?3 AND n.channel_id IS ?4)",
        )
        .bind(kind)
        .bind(subject.event_id)
        .bind(subject.service_id)
        .bind(subject.channel_id)
        .bind(subject.due_at.map(clock::db))
        .bind(&roles)
        .bind(subject.others)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    /// The latest notices of a member, newest first.
    pub async fn list_for(
        pool: &DbPool,
        user_id: i64,
        limit: i64,
    ) -> Result<Vec<MemberNotice>, sqlx::Error> {
        sqlx::query_as::<_, MemberNotice>(&format!(
            "SELECT {NOTICE_COLUMNS} FROM member_notices n \
             LEFT JOIN events e ON e.id = n.event_id \
             LEFT JOIN services s ON s.id = n.service_id \
             LEFT JOIN notification_channels c ON c.id = n.channel_id \
             WHERE n.user_id = ? ORDER BY n.created_at DESC, n.id DESC LIMIT ?"
        ))
        .bind(user_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    pub async fn unread_count(pool: &DbPool, user_id: i64) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM member_notices WHERE user_id = ? AND read_at IS NULL",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
    }

    pub async fn mark_all_read(
        pool: &DbPool,
        user_id: i64,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE member_notices SET read_at = ? WHERE user_id = ? AND read_at IS NULL")
            .bind(clock::db(now))
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Forgets the notices written before `before`.
    pub async fn purge(pool: &DbPool, before: DateTime<Utc>) -> Result<u64, sqlx::Error> {
        let done = sqlx::query("DELETE FROM member_notices WHERE created_at < ?")
            .bind(clock::db(before))
            .execute(pool)
            .await?;
        Ok(done.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::{ServiceRepository, UserRepository};
    use crate::test_helpers::test_pool;

    async fn member(pool: &DbPool, email: &str, role: Role) -> i64 {
        UserRepository::create(pool, email, "hash", "Member", role)
            .await
            .unwrap()
            .id
    }

    fn outage(service_id: i64) -> NoticeSubject {
        NoticeSubject {
            service_id: Some(service_id),
            ..NoticeSubject::default()
        }
    }

    #[tokio::test]
    async fn a_notice_for_roles_reaches_their_active_members_only() {
        let pool = test_pool().await;
        let editor = member(&pool, "editor@example.com", Role::Publisher).await;
        let admin = member(&pool, "admin@example.com", Role::Admin).await;
        let reader = member(&pool, "reader@example.com", Role::Reader).await;
        let gone = member(&pool, "gone@example.com", Role::Publisher).await;
        sqlx::query("UPDATE users SET is_active = 0 WHERE id = ?")
            .bind(gone)
            .execute(&pool)
            .await
            .unwrap();
        let vpn = ServiceRepository::create(&pool, "VPN", "vpn", None, None, None)
            .await
            .unwrap()
            .id;

        let roles = [Role::Publisher, Role::Admin];
        MemberNoticeRepository::add_for_roles(
            &pool,
            &roles,
            NoticeKind::OutageDetected,
            outage(vpn),
        )
        .await
        .unwrap();

        for (user, expected) in [(editor, 1), (admin, 1), (reader, 0), (gone, 0)] {
            let count = MemberNoticeRepository::unread_count(&pool, user)
                .await
                .unwrap();
            assert_eq!(count, expected, "user {user}");
        }
        let listed = MemberNoticeRepository::list_for(&pool, editor, 10)
            .await
            .unwrap();
        assert_eq!(listed[0].service_name.as_deref(), Some("VPN"));
        assert_eq!(listed[0].kind, NoticeKind::OutageDetected);
    }

    #[tokio::test]
    async fn the_same_news_waits_once_until_it_is_read() {
        let pool = test_pool().await;
        let editor = member(&pool, "editor@example.com", Role::Publisher).await;
        let vpn = ServiceRepository::create(&pool, "VPN", "vpn", None, None, None)
            .await
            .unwrap()
            .id;
        let roles = [Role::Publisher];
        let kind = NoticeKind::OutageDetected;

        for _ in 0..2 {
            MemberNoticeRepository::add_for_roles(&pool, &roles, kind, outage(vpn))
                .await
                .unwrap();
        }
        assert_eq!(
            MemberNoticeRepository::unread_count(&pool, editor)
                .await
                .unwrap(),
            1
        );

        MemberNoticeRepository::mark_all_read(&pool, editor, Utc::now())
            .await
            .unwrap();
        MemberNoticeRepository::add_for_roles(&pool, &roles, kind, outage(vpn))
            .await
            .unwrap();
        assert_eq!(
            MemberNoticeRepository::unread_count(&pool, editor)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn reading_clears_the_count_and_keeps_the_list() {
        let pool = test_pool().await;
        let editor = member(&pool, "editor@example.com", Role::Publisher).await;
        let vpn = ServiceRepository::create(&pool, "VPN", "vpn", None, None, None)
            .await
            .unwrap()
            .id;
        MemberNoticeRepository::add_for_member(
            &pool,
            editor,
            NoticeKind::OutageDetected,
            outage(vpn),
        )
        .await
        .unwrap();

        MemberNoticeRepository::mark_all_read(&pool, editor, Utc::now())
            .await
            .unwrap();

        assert_eq!(
            MemberNoticeRepository::unread_count(&pool, editor)
                .await
                .unwrap(),
            0
        );
        let listed = MemberNoticeRepository::list_for(&pool, editor, 10)
            .await
            .unwrap();
        assert!(!listed[0].is_unread());
    }

    #[tokio::test]
    async fn old_notices_are_forgotten() {
        let pool = test_pool().await;
        let editor = member(&pool, "editor@example.com", Role::Publisher).await;
        let vpn = ServiceRepository::create(&pool, "VPN", "vpn", None, None, None)
            .await
            .unwrap()
            .id;
        MemberNoticeRepository::add_for_member(
            &pool,
            editor,
            NoticeKind::OutageDetected,
            outage(vpn),
        )
        .await
        .unwrap();

        let kept = MemberNoticeRepository::purge(&pool, Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();
        assert_eq!(kept, 0);
        let gone = MemberNoticeRepository::purge(&pool, Utc::now() + chrono::Duration::minutes(1))
            .await
            .unwrap();
        assert_eq!(gone, 1);
    }
}
