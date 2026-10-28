//! Updates, state changes, their undoing and deletion.

use axum::extract::{Path, State};
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::detail::{Composer, ReviewForm, detail_page, drawer_page};
use super::{record_event, title_of};
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, HtmlForm, RequirePublisher, Viewer};
use crate::models::{JournalKind, Lifecycle, User};
use crate::repositories::NewJournalEntry;
use crate::routes::render;
use crate::services::{EventService, SeriesService, read_next_update};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct UpdateInput {
    #[serde(default)]
    message: String,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    lifecycle: Option<Lifecycle>,
    #[serde(default)]
    next_update: String,
}

impl UpdateInput {
    async fn post(&self, state: &AppState, id: i64, user: &User) -> Result<(), AppError> {
        let next_update = read_next_update(&self.next_update)?;
        EventService::post_update(
            &state.pool,
            id,
            &self.message,
            self.lifecycle,
            next_update,
            user,
            None,
        )
        .await?;
        self.record(state, id, user).await;
        Ok(())
    }

    /// The new state is the detail, when the update sets one.
    async fn record(&self, state: &AppState, id: i64, user: &User) {
        let detail = self.lifecycle.map(Lifecycle::as_str);
        record_event(
            &state.pool,
            JournalKind::EventUpdatePosted,
            user,
            id,
            detail,
        )
        .await;
    }

    /// The form again, with what the author wrote and why it was refused.
    fn refused(self, key: &str, i18n: &I18n) -> Composer {
        Composer {
            error: Some(i18n.t(key).to_string()),
            message: self.message,
            lifecycle: self.lifecycle,
            next_update: self.next_update,
        }
    }
}

pub async fn add_update(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<UpdateInput>,
) -> Result<Response, AppError> {
    match input.post(&state, id, &user).await {
        Ok(()) => Ok(Redirect::to(&format!("/events/{id}?published=1")).into_response()),
        Err(AppError::Validation(key)) => {
            let composer = input.refused(&key, &i18n);
            let review_form = ReviewForm::default();
            let viewer = Viewer::of(&state.pool, Some(user)).await?;
            let page = detail_page(
                &state,
                id,
                &viewer,
                csrf_token.0,
                i18n,
                composer,
                review_form,
            )
            .await?;
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
    let composer = match input.post(&state, id, &user).await {
        Ok(()) => Composer::default(),
        Err(AppError::Validation(key)) => input.refused(&key, &i18n),
        Err(e) => return Err(e),
    };
    let posted = composer.error.is_none();
    let viewer = Viewer::of(&state.pool, Some(user)).await?;
    let page = drawer_page(&state, id, &viewer, csrf_token.0, i18n, composer).await?;
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
    record_event(
        &state.pool,
        JournalKind::EventUpdateDeleted,
        &user,
        id,
        None,
    )
    .await;
    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

pub async fn revert_lifecycle(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    EventService::revert(&state.pool, id, user.role).await?;
    record_event(&state.pool, JournalKind::EventReverted, &user, id, None).await;
    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

pub async fn stop_repeat(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    SeriesService::stop(&state.pool, id).await?;
    record_event(&state.pool, JournalKind::RepeatStopped, &user, id, None).await;
    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

pub async fn delete(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let title = title_of(&state.pool, id).await;
    EventService::delete(&state.pool, id, user.role).await?;
    NewJournalEntry::new(JournalKind::EventDeleted)
        .by(&user)
        .about(title)
        .record(&state.pool)
        .await;
    Ok(Redirect::to("/events").into_response())
}
