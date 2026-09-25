//! Updates, state changes, their undoing and deletion.

use axum::extract::{Path, State};
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::detail::{Composer, detail_page, drawer_page};
use crate::error::AppError;
use crate::i18n::Locale;
use crate::middleware::{CsrfToken, HtmlForm, RequirePublisher};
use crate::models::Lifecycle;
use crate::routes::render;
use crate::services::EventService;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct UpdateInput {
    #[serde(default)]
    message: String,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    lifecycle: Option<Lifecycle>,
}

pub async fn add_update(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<UpdateInput>,
) -> Result<Response, AppError> {
    match EventService::post_update(&state.pool, id, &input.message, input.lifecycle, &user).await {
        Ok(()) => Ok(Redirect::to(&format!("/events/{id}?posted=1")).into_response()),
        Err(AppError::Validation(key)) => {
            let composer = Composer {
                error: Some(i18n.t(&key).to_string()),
                message: input.message,
                lifecycle: input.lifecycle,
            };
            let page = detail_page(&state, id, Some(&user), csrf_token.0, i18n, composer).await?;
            render(&page)
        }
        Err(e) => Err(e),
    }
}

/// An update posted from the side panel: the panel is drawn again in
/// place, and the page behind it learns that the event changed.
pub async fn add_update_in_panel(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<UpdateInput>,
) -> Result<Response, AppError> {
    let result =
        EventService::post_update(&state.pool, id, &input.message, input.lifecycle, &user).await;
    let composer = match result {
        Ok(()) => Composer::default(),
        Err(AppError::Validation(key)) => Composer {
            error: Some(i18n.t(&key).to_string()),
            message: input.message,
            lifecycle: input.lifecycle,
        },
        Err(e) => return Err(e),
    };
    let posted = composer.error.is_none();
    let page = drawer_page(&state, id, Some(&user), csrf_token.0, i18n, composer).await?;
    let mut response = render(&page)?;
    if posted {
        response
            .headers_mut()
            .insert("HX-Trigger", HeaderValue::from_static("event-updated"));
    }
    Ok(response)
}

pub async fn delete_update(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path((id, update_id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    EventService::delete_update(&state.pool, id, update_id, &user).await?;
    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

pub async fn revert_lifecycle(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    EventService::revert(&state.pool, id, user.role).await?;
    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

pub async fn delete(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    EventService::delete(&state.pool, id, user.role).await?;
    Ok(Redirect::to("/events").into_response())
}
