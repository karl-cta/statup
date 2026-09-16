//! Icon routes: upload, library browsing and deletion.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;

use super::{Frame, render};
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, RequirePublisher};
use crate::models::{Icon, MAX_ICON_SIZE, User};
use crate::repositories::IconRepository;
use crate::services::{IconService, file_exists, icon_path};
use crate::state::AppState;

/// Longest original file name kept, in characters.
const ORIGINAL_NAME_MAX_CHARS: usize = 255;

/// One icon of the library with what the page has to say about it.
struct IconCard {
    icon: Icon,
    /// Services wearing this icon, by name.
    services: Vec<String>,
    /// Events and templates wearing it, counted.
    other_uses: i64,
    /// The row exists but the file does not: the image is broken wherever
    /// the icon is used, and deleting the row is the cleanup.
    file_missing: bool,
}

impl IconCard {
    fn in_use(&self) -> bool {
        !self.services.is_empty() || self.other_uses > 0
    }

    fn deletable(&self) -> bool {
        self.file_missing || !self.in_use()
    }
}

#[derive(Template)]
#[template(path = "icons/list.html")]
struct IconListTemplate {
    frame: Frame,
    cards: Vec<IconCard>,
    /// Name of the icon the previous action added or removed.
    added: Option<String>,
    removed: Option<String>,
    /// A refused upload or deletion, said in the page rather than on the
    /// bare error page.
    error: Option<String>,
    i18n: I18n,
}

/// What the library page says on top of the cards.
#[derive(Default)]
struct LibraryNotice {
    added: Option<String>,
    removed: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
pub struct IconListQuery {
    added: Option<i64>,
    removed: Option<String>,
}

#[derive(Template)]
#[template(path = "components/icon_grid.html")]
struct IconGridTemplate {
    custom_icons: Vec<Icon>,
    selected_icon_id: Option<i64>,
    /// Refusals come back with a 200 so htmx swaps the message in.
    upload_error: Option<String>,
    i18n: I18n,
}

pub async fn list(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    Query(query): Query<IconListQuery>,
) -> Result<Response, AppError> {
    let cards = load_cards(&state).await?;
    let notice = LibraryNotice {
        added: query
            .added
            .and_then(|id| cards.iter().find(|c| c.icon.id == id))
            .map(|c| c.icon.original_name.clone()),
        removed: query.removed,
        error: None,
    };
    render_list(&state, &user, csrf_token, i18n, cards, notice).await
}

async fn load_cards(state: &AppState) -> Result<Vec<IconCard>, AppError> {
    let icons = IconRepository::list_all(&state.pool).await?;
    let mut services: HashMap<i64, Vec<String>> = HashMap::new();
    for (icon_id, name) in IconRepository::service_names_by_icon(&state.pool).await? {
        services.entry(icon_id).or_default().push(name);
    }
    let other_uses: HashMap<i64, i64> = IconRepository::other_use_counts(&state.pool)
        .await?
        .into_iter()
        .collect();

    let mut cards = Vec::with_capacity(icons.len());
    for icon in icons {
        let file_missing = !file_exists(&icon_path(&state.upload_dir, &icon.filename)).await;
        cards.push(IconCard {
            services: services.remove(&icon.id).unwrap_or_default(),
            other_uses: other_uses.get(&icon.id).copied().unwrap_or(0),
            file_missing,
            icon,
        });
    }
    Ok(cards)
}

async fn render_list(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    cards: Vec<IconCard>,
    notice: LibraryNotice,
) -> Result<Response, AppError> {
    let frame = Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?;
    render(&IconListTemplate {
        frame,
        cards,
        added: notice.added,
        removed: notice.removed,
        error: notice.error,
        i18n,
    })
}

/// The uploaded file of a multipart request, with its original name.
/// CSRF is validated upstream by middleware.
async fn extract_upload(multipart: &mut Multipart) -> Result<(String, Vec<u8>), AppError> {
    let unreadable = |e: axum::extract::multipart::MultipartError| {
        tracing::debug!(error = %e, "Unreadable upload");
        AppError::Validation("validation.invalid_form_data".to_string())
    };
    let mut file_data = None;
    while let Some(field) = multipart.next_field().await.map_err(unreadable)? {
        if field.name() != Some("file") {
            continue;
        }
        let original_name: String = field
            .file_name()
            .unwrap_or("icon")
            .chars()
            .take(ORIGINAL_NAME_MAX_CHARS)
            .collect();
        let data = field.bytes().await.map_err(unreadable)?;
        if data.len() > MAX_ICON_SIZE {
            return Err(AppError::Validation(
                "validation.file_too_large".to_string(),
            ));
        }
        file_data = Some((original_name, data.to_vec()));
    }
    file_data.ok_or_else(|| AppError::Validation("validation.no_file".to_string()))
}

async fn store_upload(
    state: &AppState,
    user: &User,
    multipart: &mut Multipart,
) -> Result<Icon, AppError> {
    let (original_name, data) = extract_upload(multipart).await?;
    IconService::upload(
        &state.pool,
        &state.upload_dir,
        data,
        &original_name,
        user.id,
    )
    .await
}

pub async fn upload(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    match store_upload(&state, &user, &mut multipart).await {
        Ok(icon) => Ok(Redirect::to(&format!("/icons?added={}", icon.id)).into_response()),
        Err(AppError::Validation(key)) => {
            let cards = load_cards(&state).await?;
            let notice = LibraryNotice {
                error: Some(i18n.t(&key).to_string()),
                ..LibraryNotice::default()
            };
            render_list(&state, &user, csrf_token, i18n, cards, notice).await
        }
        Err(e) => Err(e),
    }
}

pub async fn upload_picker(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Locale(i18n): Locale,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let (selected_icon_id, upload_error) = match store_upload(&state, &user, &mut multipart).await {
        Ok(icon) => (Some(icon.id), None),
        Err(AppError::Validation(key)) => (None, Some(i18n.t(&key).to_string())),
        Err(e) => return Err(e),
    };

    render(&IconGridTemplate {
        custom_icons: IconService::choosable(&state.pool, &state.upload_dir).await?,
        selected_icon_id,
        upload_error,
        i18n,
    })
}

pub async fn delete(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    match IconService::delete(&state.pool, &state.upload_dir, id).await {
        Ok(icon) => {
            let removed = utf8_percent_encode(&icon.original_name, NON_ALPHANUMERIC);
            Ok(Redirect::to(&format!("/icons?removed={removed}")).into_response())
        }
        Err(AppError::Validation(key)) => {
            let cards = load_cards(&state).await?;
            let notice = LibraryNotice {
                error: Some(i18n.t(&key).to_string()),
                ..LibraryNotice::default()
            };
            render_list(&state, &user, csrf_token, i18n, cards, notice).await
        }
        Err(e) => Err(e),
    }
}
