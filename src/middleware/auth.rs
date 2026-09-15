//! Authentication middleware - extractors for `AuthUser`, `OptionalUser`,
//! `RequirePublisher`, `RequireAdmin`.

use std::convert::Infallible;

use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use tower_sessions::Session;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::User;
use crate::repositories::UserRepository;
use crate::session::{USER_ID_KEY, credential_matches};

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
async fn load_user_from_session<S>(parts: &mut Parts, state: &S) -> Option<User>
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    let session = Session::from_request_parts(parts, state).await.ok()?;
    let pool = DbPool::from_ref(state);
    session_user(&session, &pool).await
}

/// The signed-in user, or `None` when the session is empty, the account is
/// gone or disabled, or its password changed since the session was opened.
async fn session_user(session: &Session, pool: &DbPool) -> Option<User> {
    let user_id: i64 = session.get(USER_ID_KEY).await.ok().flatten()?;
    let user = UserRepository::find_by_id(pool, user_id)
        .await
        .ok()
        .flatten()?;

    if !user.is_active || !credential_matches(session, &user.password_hash).await {
        session.flush().await.ok();
        return None;
    }

    Some(user)
}

/// Where a person with a temporary password can still go: the page that
/// replaces it, and the ways out.
const PASSWORD_CHANGE_PATHS: [&str; 5] = ["/password/new", "/login", "/logout", "/i18n", "/health"];

/// Sends a signed-in person whose password was chosen by someone else to the
/// page that replaces it, whatever page they asked for.
pub async fn require_password_change(
    State(pool): State<DbPool>,
    session: Session,
    request: Request,
    next: Next,
) -> Response {
    if !PASSWORD_CHANGE_PATHS.contains(&request.uri().path())
        && must_change_password(&session, &pool).await
    {
        return Redirect::to("/password/new").into_response();
    }
    next.run(request).await
}

async fn must_change_password(session: &Session, pool: &DbPool) -> bool {
    session_user(session, pool)
        .await
        .is_some_and(|user| user.must_change_password)
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
