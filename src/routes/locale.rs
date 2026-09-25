//! Language switch: `GET /i18n?locale=fr|en` keeps the choice in a cookie,
//! and on the account of a signed-in person, then sends the visitor back to
//! the page they came from. A link needs no session and no CSRF token.

use axum::extract::{Query, State};
use axum::http::header::{HOST, LOCATION, REFERER, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
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
pub struct SwitchQuery {
    #[serde(default)]
    locale: String,
}

pub async fn switch(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    headers: HeaderMap,
    Query(query): Query<SwitchQuery>,
) -> Result<Response, AppError> {
    let Some(locale) = LOCALES.iter().copied().find(|l| *l == query.locale) else {
        return Err(AppError::Validation("error.unsupported_locale".into()));
    };
    if let Some(user) = &user {
        UserRepository::update_preferred_locale(&state.pool, user.id, Some(locale)).await?;
    }

    let cookie = HeaderValue::from_str(&locale_cookie(locale, state.serves_https()))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid cookie header: {e}")))?;
    let location = HeaderValue::from_str(&return_target(&headers))
        .unwrap_or_else(|_| HeaderValue::from_static("/"));
    Ok((
        StatusCode::SEE_OTHER,
        [(LOCATION, location), (SET_COOKIE, cookie)],
    )
        .into_response())
}

/// The `lang` cookie, `Secure` when the instance is served over HTTPS.
pub(super) fn locale_cookie(locale: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("lang={locale}; Path=/; Max-Age={COOKIE_MAX_AGE}; SameSite=Lax{secure}")
}

/// The page to go back to: the path and query of a referring page of this
/// same site, "/" for anything else.
fn return_target(headers: &HeaderMap) -> String {
    let header = |name| {
        headers
            .get(name)
            .and_then(|v: &HeaderValue| v.to_str().ok())
    };
    match (header(REFERER), header(HOST)) {
        (Some(referer), Some(host)) => same_site_page(referer, host).unwrap_or_else(|| "/".into()),
        _ => "/".into(),
    }
}

fn same_site_page(referer: &str, host: &str) -> Option<String> {
    let uri: Uri = referer.parse().ok()?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return None;
    }
    if !uri.authority()?.as_str().eq_ignore_ascii_case(host) {
        return None;
    }
    let path = uri.path();
    // "//host" and "/\host" would be read by browsers as another site.
    if !path.starts_with('/') || path.starts_with("//") || path.starts_with("/\\") {
        return None;
    }
    let page = canonical_page(path);
    match uri.query() {
        Some(query) if page == path => Some(format!("{path}?{query}")),
        _ => Some(page),
    }
}

/// The page a visitor sees for a path. A form posted to an action path
/// (`/admin/users/7/role`) leaves that path as the referrer of the next
/// page, and following it would ask for a page that only takes a POST: an
/// action leads back to the page of its form, anything else home.
fn canonical_page(path: &str) -> String {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if is_page(&segments) {
        return path.to_string();
    }
    let page = match segments.as_slice() {
        ["admin", "users", ..] => "/admin/users",
        ["admin", "settings", ..] => "/admin/settings",
        ["profile", ..] => "/profile",
        ["icons", ..] => "/icons",
        ["events", "templates", ..] => "/events/new",
        ["events", id, action, ..] if is_id(id) && *action != "delete" => {
            return format!("/events/{id}");
        }
        ["events", ..] => "/events",
        ["services", ..] => "/services",
        ["logout"] => "/login",
        _ => "/",
    };
    page.to_string()
}

/// The paths that answer a GET with a page of their own.
fn is_page(segments: &[&str]) -> bool {
    match segments {
        ["" | "events" | "services" | "icons" | "subscribe" | "login" | "register" | "profile"]
        | ["events" | "services" | "password", "new"]
        | ["admin", "settings" | "users"]
        | ["setup", "page" | "services" | "done"] => true,
        ["events", id] | ["events" | "services", id, "edit"] => is_id(id),
        _ => false,
    }
}

fn is_id(segment: &str) -> bool {
    !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_paths_map_to_the_page_that_shows_them() {
        let cases = [
            ("/admin/users/new", "/admin/users"),
            ("/admin/users/7/role", "/admin/users"),
            ("/admin/users/7/disable", "/admin/users"),
            ("/admin/users/7/reset-password", "/admin/users"),
            ("/admin/settings/instance-name", "/admin/settings"),
            ("/admin/settings/public-mode", "/admin/settings"),
            ("/admin/settings/time-zone", "/admin/settings"),
            ("/admin/settings/logo", "/admin/settings"),
            ("/admin/settings/logo/remove", "/admin/settings"),
            ("/profile/password", "/profile"),
            ("/icons/upload", "/icons"),
            ("/icons/upload-picker", "/icons"),
            ("/icons/3/delete", "/icons"),
            ("/events/12/updates", "/events/12"),
            ("/events/12/panel-updates", "/events/12"),
            ("/events/12/updates/3/delete", "/events/12"),
            ("/events/12/revert-lifecycle", "/events/12"),
            ("/events/12/drawer", "/events/12"),
            ("/events/12/delete", "/events"),
            ("/events/templates/4/delete", "/events/new"),
            ("/services/2/status", "/services"),
            ("/services/2/delete", "/services"),
            ("/services/2/drawer", "/services"),
            ("/logout", "/login"),
        ];
        for (path, page) in cases {
            assert_eq!(canonical_page(path), page, "{path}");
        }
    }

    #[test]
    fn a_path_that_is_no_page_leads_home() {
        for path in [
            "/i18n",
            "/admin/dashboard/layout/order",
            "/admin/dashboard/layout/services/width",
            "/search",
            "/feed",
            "/nowhere",
        ] {
            assert_eq!(canonical_page(path), "/", "{path}");
        }
        assert_eq!(canonical_page("/events/templates"), "/events/new");
        assert_eq!(canonical_page("/events/abc"), "/events");
    }

    #[test]
    fn pages_that_answer_a_get_are_kept() {
        for path in [
            "/",
            "/events",
            "/events/12",
            "/events/new",
            "/events/12/edit",
            "/services",
            "/services/new",
            "/services/2/edit",
            "/icons",
            "/subscribe",
            "/admin/users",
            "/admin/settings",
            "/setup/page",
            "/setup/services",
            "/setup/done",
            "/password/new",
            "/profile",
            "/login",
            "/register",
        ] {
            assert_eq!(canonical_page(path), path);
        }
    }

    #[test]
    fn only_a_page_of_this_site_is_a_return_target() {
        let host = "status.example.com";
        assert_eq!(
            same_site_page("https://status.example.com/events?kind=incident", host).as_deref(),
            Some("/events?kind=incident")
        );
        assert_eq!(
            same_site_page("http://STATUS.example.com/admin/users/3/role?x=1", host).as_deref(),
            Some("/admin/users")
        );
        assert_eq!(same_site_page("https://evil.example/events", host), None);
        assert_eq!(
            same_site_page("https://status.example.com.evil/", host),
            None
        );
        assert_eq!(
            same_site_page("https://user@status.example.com/", host),
            None
        );
        assert_eq!(same_site_page("javascript:alert(1)", host), None);
        assert_eq!(same_site_page("/events", host), None);
        assert_eq!(
            same_site_page("https://status.example.com//evil.example/", host),
            None
        );
        assert_eq!(
            same_site_page("http://localhost:3000/events", "localhost:3000").as_deref(),
            Some("/events")
        );
        assert_eq!(
            same_site_page("http://localhost:3001/events", "localhost:3000"),
            None
        );
    }

    #[test]
    fn the_cookie_is_secure_behind_https() {
        assert!(locale_cookie("en", true).ends_with("; Secure"));
        assert!(!locale_cookie("en", false).contains("Secure"));
        assert!(locale_cookie("fr", false).starts_with("lang=fr; Path=/;"));
    }
}
