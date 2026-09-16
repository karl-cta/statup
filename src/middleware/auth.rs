//! Authentication: the signed-in user, loaded once per request by
//! [`load_session_user`], and the extractors that require it or a role.

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
use crate::session::{
    USER_ID_KEY, apply_signed_in_expiry, credential_matches, read_value, refresh_signed_in,
};

/// The user [`load_session_user`] found for this request, `None` for a
/// visitor. Its presence tells the extractors the lookup already happened.
#[derive(Clone)]
struct SessionUser(Option<User>);

/// Extractor that provides the authenticated user.
///
/// Redirects to `/login` when nobody is signed in, the account is disabled
/// or its password changed since the session was opened.
pub struct AuthUser(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        signed_in_user(parts, state)
            .await
            .map(AuthUser)
            .ok_or_else(|| Redirect::to("/login"))
    }
}

/// Optional user extractor, returns `Some(User)` if authenticated, `None` otherwise.
///
/// Never rejects: used for read-only routes that are accessible in public mode
/// without authentication.
pub struct OptionalUser(pub Option<User>);

#[async_trait]
impl<S> FromRequestParts<S> for OptionalUser
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(OptionalUser(signed_in_user(parts, state).await))
    }
}

/// The user loaded by the middleware, or a lookup when a route runs
/// without it.
async fn signed_in_user<S>(parts: &mut Parts, state: &S) -> Option<User>
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    if let Some(SessionUser(user)) = parts.extensions.get::<SessionUser>() {
        return user.clone();
    }
    let session = Session::from_request_parts(parts, state).await.ok()?;
    session_user(&session, &DbPool::from_ref(state)).await
}

/// The signed-in user, or `None` when the session is empty, the account is
/// gone or disabled, or its password changed since the session was opened.
async fn session_user(session: &Session, pool: &DbPool) -> Option<User> {
    let user_id: i64 = read_value(session, USER_ID_KEY).await.ok().flatten()?;
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
const PASSWORD_CHANGE_PATHS: [&str; 4] = ["/password/new", "/login", "/logout", "/i18n"];

/// Loads the signed-in user once for the whole request, extends the session
/// on activity, and sends a person whose password was chosen by someone
/// else to the page that replaces it, whatever page they asked for.
pub async fn load_session_user(
    State(pool): State<DbPool>,
    session: Session,
    mut request: Request,
    next: Next,
) -> Response {
    let user = session_user(&session, &pool).await;
    if let Some(user) = &user {
        if user.must_change_password && !PASSWORD_CHANGE_PATHS.contains(&request.uri().path()) {
            return Redirect::to("/password/new").into_response();
        }
        if let Err(e) = refresh_signed_in(&session).await {
            tracing::warn!(error = %e, "Session lifetime was not extended");
        }
    }

    let signed_in = user.is_some();
    request.extensions_mut().insert(SessionUser(user));
    let response = next.run(request).await;

    if signed_in
        && session.is_modified()
        && let Err(e) = apply_signed_in_expiry(&session).await
    {
        tracing::warn!(error = %e, "Session lifetime was not kept");
    }
    response
}

/// Extractor that requires the authenticated user to have the `Publisher` or `Admin` role.
///
/// Returns `AppError::Unauthorized` (401) without a signed-in user and
/// `AppError::Forbidden` (403) if the role is insufficient.
pub struct RequirePublisher(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for RequirePublisher
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = signed_in_user(parts, state)
            .await
            .ok_or(AppError::Unauthorized)?;
        if !user.role.can_publish() {
            return Err(AppError::Forbidden);
        }
        Ok(RequirePublisher(user))
    }
}

/// Extractor that requires the authenticated user to have the `Admin` role.
///
/// Returns `AppError::Unauthorized` (401) without a signed-in user and
/// `AppError::Forbidden` (403) if the role is insufficient.
pub struct RequireAdmin(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for RequireAdmin
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = signed_in_user(parts, state)
            .await
            .ok_or(AppError::Unauthorized)?;
        if !user.role.can_admin() {
            return Err(AppError::Forbidden);
        }
        Ok(RequireAdmin(user))
    }
}
