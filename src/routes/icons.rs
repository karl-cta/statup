//! Icon routes - upload, library browsing, delete.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, RequirePublisher};
use crate::models::{Icon, MAX_ICON_SIZE, User};
use crate::repositories::{EventRepository, IconRepository};
use crate::services::{EventService, IconService};
use crate::state::AppState;

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
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    cards: Vec<IconCard>,
    /// Name of the icon the previous action added or removed.
    added: Option<String>,
    removed: Option<String>,
    /// A refused upload or deletion, said in the page rather than on the
    /// bare error page.
    error: Option<String>,
    i18n: I18n,
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
    /// A refused upload comes back as a normal swap carrying this message.
    /// htmx does not swap error statuses, so a 400 left the author with a
    /// picker that did nothing and said nothing.
    upload_error: Option<String>,
    i18n: I18n,
}

fn render(tpl: &impl Template) -> Result<Response, AppError> {
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

fn layout_fields(user: &User) -> (String, bool, bool) {
    (user.display_name.clone(), user.role.can_admin(), true)
}

async fn unread(pool: &crate::db::DbPool, user: &User) -> Result<i64, AppError> {
    EventService::unread_count(pool, user.last_seen_at).await
}

pub async fn list(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    Query(query): Query<IconListQuery>,
) -> Result<Response, AppError> {
    let cards = load_cards(&state).await?;
    let added = query
        .added
        .and_then(|id| cards.iter().find(|c| c.icon.id == id))
        .map(|c| c.icon.original_name.clone());
    render_list(
        &state,
        &user,
        csrf_token.0,
        i18n,
        cards,
        added,
        query.removed,
        None,
    )
    .await
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
    let cards = icons
        .into_iter()
        .map(|icon| {
            let path = format!("{}/icons/{}", state.upload_dir, icon.filename);
            IconCard {
                services: services.remove(&icon.id).unwrap_or_default(),
                other_uses: other_uses.get(&icon.id).copied().unwrap_or(0),
                file_missing: !std::path::Path::new(&path).exists(),
                icon,
            }
        })
        .collect();
    Ok(cards)
}

#[allow(clippy::too_many_arguments)]
async fn render_list(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    cards: Vec<IconCard>,
    added: Option<String>,
    removed: Option<String>,
    error: Option<String>,
) -> Result<Response, AppError> {
    let (user_display_name, is_admin, is_authenticated) = layout_fields(user);
    let unread_count = unread(&state.pool, user).await?;
    let last_admin_action = EventRepository::last_admin_action(&state.pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt));
    let tpl = IconListTemplate {
        csrf_token,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        cards,
        added,
        removed,
        error,
        i18n,
    };
    render(&tpl)
}

/// Extract the uploaded file from a multipart request, enforcing size limit.
/// CSRF is validated upstream by middleware.
async fn extract_upload(
    multipart: &mut Multipart,
    i18n: &I18n,
) -> Result<(String, Vec<u8>), AppError> {
    let mut file_data: Option<(String, Vec<u8>)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(format!("{}: {e}", i18n.t("validation.generic_error"))))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            let original_name = field.file_name().unwrap_or("icon").to_string();
            let data = field.bytes().await.map_err(|e| {
                AppError::Validation(format!("{}: {e}", i18n.t("validation.generic_error")))
            })?;

            if data.len() > MAX_ICON_SIZE {
                return Err(AppError::Validation(
                    i18n.t("validation.file_too_large").to_string(),
                ));
            }

            file_data = Some((original_name, data.to_vec()));
        } else {
            // CSRF is validated by middleware; drain any other field.
            let _ = field.bytes().await;
        }
    }

    file_data.ok_or_else(|| AppError::Validation(i18n.t("validation.no_file").to_string()))
}

pub async fn upload(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let uploaded = async {
        let (original_name, data) = extract_upload(&mut multipart, &i18n).await?;
        IconService::upload(
            &state.pool,
            &state.upload_dir,
            &data,
            &original_name,
            user.id,
        )
        .await
    }
    .await;

    match uploaded {
        Ok(icon) => Ok(Redirect::to(&format!("/icons?added={}", icon.id)).into_response()),
        Err(AppError::Validation(msg)) => {
            let cards = load_cards(&state).await?;
            let error = Some(i18n.t(&msg).to_string());
            render_list(&state, &user, csrf_token.0, i18n, cards, None, None, error).await
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
    let uploaded = async {
        let (original_name, data) = extract_upload(&mut multipart, &i18n).await?;
        IconService::upload(
            &state.pool,
            &state.upload_dir,
            &data,
            &original_name,
            user.id,
        )
        .await
    }
    .await;

    let (selected_icon_id, upload_error) = match uploaded {
        Ok(icon) => (Some(icon.id), None),
        Err(AppError::Validation(msg)) => (None, Some(i18n.t(&msg).to_string())),
        Err(e) => return Err(e),
    };

    let icons = IconRepository::list_all(&state.pool).await?;
    let tpl = IconGridTemplate {
        custom_icons: icons,
        selected_icon_id,
        upload_error,
        i18n,
    };
    render(&tpl)
}

pub async fn delete(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let name = IconRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?
        .original_name;
    match IconService::delete(&state.pool, &state.upload_dir, id).await {
        Ok(()) => {
            let removed = utf8_percent_encode(&name, NON_ALPHANUMERIC);
            Ok(Redirect::to(&format!("/icons?removed={removed}")).into_response())
        }
        Err(AppError::Validation(msg)) => {
            let cards = load_cards(&state).await?;
            let error = Some(i18n.t(&msg).to_string());
            render_list(&state, &user, csrf_token.0, i18n, cards, None, None, error).await
        }
        Err(e) => Err(e),
    }
}
