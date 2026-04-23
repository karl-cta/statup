//! Icon routes - upload, library browsing, delete.

use askama::Template;
use axum::extract::{Multipart, Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, RequirePublisher};
use crate::models::{Icon, MAX_ICON_SIZE, User};
use crate::repositories::{EventRepository, IconRepository};
use crate::services::{EventService, IconService};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "icons/list.html")]
struct IconListTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    icons: Vec<Icon>,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "components/icon_grid.html")]
struct IconGridTemplate {
    custom_icons: Vec<Icon>,
    selected_icon_id: Option<i64>,
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
) -> Result<Response, AppError> {
    let (user_display_name, is_admin, is_authenticated) = layout_fields(&user);
    let unread_count = unread(&state.pool, &user).await?;
    let icons = IconRepository::list_all(&state.pool).await?;
    let last_admin_action = EventRepository::last_admin_action(&state.pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt));
    let tpl = IconListTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        icons,
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
    Locale(i18n): Locale,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let (original_name, data) = extract_upload(&mut multipart, &i18n).await?;

    IconService::upload(
        &state.pool,
        &state.upload_dir,
        &data,
        &original_name,
        user.id,
    )
    .await?;

    Ok(Redirect::to("/icons").into_response())
}

pub async fn upload_picker(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Locale(i18n): Locale,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let (original_name, data) = extract_upload(&mut multipart, &i18n).await?;

    let icon = IconService::upload(
        &state.pool,
        &state.upload_dir,
        &data,
        &original_name,
        user.id,
    )
    .await?;

    let icons = IconRepository::list_all(&state.pool).await?;
    let tpl = IconGridTemplate {
        custom_icons: icons,
        selected_icon_id: Some(icon.id),
        i18n,
    };
    render(&tpl)
}

pub async fn delete(
    RequirePublisher(_user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Locale(_i18n): Locale,
) -> Result<Response, AppError> {
    IconService::delete(&state.pool, &state.upload_dir, id).await?;
    Ok(Redirect::to("/icons").into_response())
}
