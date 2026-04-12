//! Authentication middleware - extractors for `AuthUser`, `OptionalUser`,
//! `RequirePublisher`, `RequireAdmin`.

use std::convert::Infallible;

use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum::response::Redirect;
use tower_sessions::Session;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::User;
use crate::repositories::UserRepository;
use crate::session::USER_ID_KEY;

/// Extractor that provides the authenticated user.
///
/// Reads `user_id` from the session, loads the user from the DB,
/// and redirects to `/login` if anything fails (no session, user not found,
/// or user inactive).
pub struct AuthUser(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        load_user_from_session(parts, state)
            .await
            .map(AuthUser)
            .ok_or_else(|| Redirect::to("/login"))
    }
}

/// Optional user extractor, returns `Some(User)` if authenticated, `None` otherwise.
///
/// Never rejects: used for read-only routes that are accessible in public mode
/// without authentication (REQ-16).
pub struct OptionalUser(pub Option<User>);

#[async_trait]
impl<S> FromRequestParts<S> for OptionalUser
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = load_user_from_session(parts, state).await;
        Ok(OptionalUser(user))
    }
}

/// Try to load the authenticated user from the session.
///
/// Returns `None` if anything fails (no session, no `user_id`, user not found,
/// or user inactive).
async fn load_user_from_session<S>(parts: &mut Parts, state: &S) -> Option<User>
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    let session = Session::from_request_parts(parts, state).await.ok()?;
    let user_id: i64 = session.get(USER_ID_KEY).await.ok().flatten()?;
    let pool = DbPool::from_ref(state);
    let user = UserRepository::find_by_id(&pool, user_id)
        .await
        .ok()
        .flatten()?;

    if !user.is_active {
        session.flush().await.ok();
        return None;
    }

    Some(user)
}

/// Extractor that requires the authenticated user to have the `Publisher` or `Admin` role.
///
/// Delegates authentication to [`AuthUser`], then checks `role.can_publish()`.
/// Returns `AppError::Forbidden` (403) if the role is insufficient.
pub struct RequirePublisher(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for RequirePublisher
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_user = AuthUser::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::Unauthorized)?;

        if !auth_user.0.role.can_publish() {
            return Err(AppError::Forbidden);
        }

        Ok(RequirePublisher(auth_user.0))
    }
}

/// Extractor that requires the authenticated user to have the `Admin` role.
///
/// Delegates authentication to [`AuthUser`], then checks `role.can_admin()`.
/// Returns `AppError::Forbidden` (403) if the role is insufficient.
pub struct RequireAdmin(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for RequireAdmin
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_user = AuthUser::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::Unauthorized)?;

        if !auth_user.0.role.can_admin() {
            return Err(AppError::Forbidden);
        }

        Ok(RequireAdmin(auth_user.0))
    }
}
