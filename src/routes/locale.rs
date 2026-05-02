//! Locale switching, persists the active locale in a cookie and on the user profile.
//!
//! `POST /i18n` accepts `locale=fr|en`, sets the `lang` cookie for one year,
//! persists the choice on the authenticated user (if any), then either signals
//! HTMX to refresh or redirects back to the referring page.

use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::{HeaderMap, HeaderValue, LOCATION, REFERER, SET_COOKIE};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::error::AppError;
use crate::i18n::LOCALES;
use crate::middleware::OptionalUser;
use crate::repositories::UserRepository;
use crate::state::AppState;

/// Cookie max-age, one year in seconds.
const COOKIE_MAX_AGE: u32 = 60 * 60 * 24 * 365;

#[derive(Deserialize)]
pub struct SwitchInput {
    locale: String,
}

pub async fn switch(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    headers: HeaderMap,
    axum::extract::Form(input): axum::extract::Form<SwitchInput>,
) -> Result<Response, AppError> {
    if !LOCALES.contains(&input.locale.as_str()) {
        return Err(AppError::Validation("error.unsupported_locale".into()));
    }

    if let Some(u) = user.as_ref() {
        UserRepository::update_preferred_locale(&state.pool, u.id, Some(&input.locale)).await?;
    }

    let cookie = format!(
        "lang={}; Path=/; Max-Age={}; SameSite=Lax",
        input.locale, COOKIE_MAX_AGE
    );
    let cookie_value = HeaderValue::from_str(&cookie)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid cookie header: {e}")))?;

    let mut response_headers = HeaderMap::new();
    response_headers.insert(SET_COOKIE, cookie_value);

    if headers.contains_key("hx-request") {
        response_headers.insert("hx-refresh", HeaderValue::from_static("true"));
        return Ok((StatusCode::NO_CONTENT, response_headers).into_response());
    }

    let target = headers
        .get(REFERER)
        .and_then(|v| v.to_str().ok())
        .map_or_else(|| "/".to_string(), ToOwned::to_owned);
    let location = HeaderValue::from_str(&target).unwrap_or_else(|_| HeaderValue::from_static("/"));
    response_headers.insert(LOCATION, location);

    Ok((StatusCode::SEE_OTHER, response_headers).into_response())
}
