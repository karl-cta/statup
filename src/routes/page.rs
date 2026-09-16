//! What every page needs around its content: the masthead, the footer and
//! the token htmx sends back with its requests.

use askama::Template;
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect, Response};
use chrono::Utc;

use crate::clock;
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::User;
use crate::repositories::EventRepository;
use crate::services::EventService;
use crate::state::AppState;

/// Anonymous visitors read only while the pages are open to everyone; the
/// others are sent to the sign-in page. A background request is told to
/// leave the page, rather than handed the sign-in form to swap in.
pub fn members_only(
    state: &AppState,
    user: Option<&User>,
    headers: &HeaderMap,
) -> Option<Response> {
    if user.is_some() || state.is_public_mode() {
        return None;
    }
    let response = if headers.contains_key("hx-request") {
        [("hx-redirect", "/login")].into_response()
    } else {
        Redirect::to("/login").into_response()
    };
    Some(response)
}

pub struct Frame {
    pub csrf_token: String,
    pub user_name: String,
    pub is_authenticated: bool,
    pub can_publish: bool,
    pub is_admin: bool,
    pub unread_count: i64,
    pub unread_label: String,
    pub dateline: String,
    pub clock: String,
    pub clock_offset: i32,
    pub last_update_iso: Option<String>,
    pub last_update_label: Option<String>,
}

impl Frame {
    /// Masthead tabs: the dashboard and the events for everyone, then the
    /// services and the settings by role.
    pub fn tab_count(&self) -> usize {
        2 + usize::from(self.can_publish) + usize::from(self.is_admin)
    }

    pub async fn load(
        pool: &DbPool,
        user: Option<&User>,
        csrf_token: String,
        i18n: &I18n,
    ) -> Result<Self, AppError> {
        let now = Utc::now();
        let unread_count = match user {
            Some(u) => EventService::unread_count(pool, u.last_seen_at).await?,
            None => 0,
        };
        let last_update = EventRepository::last_admin_action(pool).await?;
        Ok(Self {
            csrf_token,
            user_name: user.map(|u| u.display_name.clone()).unwrap_or_default(),
            is_authenticated: user.is_some(),
            can_publish: user.is_some_and(|u| u.role.can_publish()),
            is_admin: user.is_some_and(|u| u.role.can_admin()),
            unread_count,
            unread_label: i18n.plural("nav.unread", usize::try_from(unread_count).unwrap_or(0)),
            dateline: i18n.format_dateline(&clock::today()),
            clock: i18n.format_clock(&now),
            clock_offset: clock::offset_minutes(&now),
            last_update_iso: last_update.map(|t| t.to_rfc3339()),
            last_update_label: last_update.map(|t| i18n.format_datetime_long(&t)),
        })
    }
}

pub fn render(template: &impl Template) -> Result<Response, AppError> {
    let html = template
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

/// A run of a title, marked when it matches the search.
pub struct TitlePart {
    pub text: String,
    pub hit: bool,
}

/// Splits `title` into runs, marking the words that start with one of the
/// searched words. Case and common accents are ignored, like the search
/// index does, and positions are counted in characters.
pub fn title_parts(title: &str, query: Option<&str>) -> Vec<TitlePart> {
    let chars: Vec<char> = title.chars().collect();
    let folded: Vec<char> = chars.iter().copied().map(fold).collect();
    let mut hits = vec![false; chars.len()];
    for term in query.map(search_terms).unwrap_or_default() {
        mark_prefix_matches(&folded, &term, &mut hits);
    }
    let mut parts: Vec<TitlePart> = Vec::new();
    for (ch, hit) in chars.into_iter().zip(hits) {
        match parts.last_mut() {
            Some(last) if last.hit == hit => last.text.push(ch),
            _ => parts.push(TitlePart {
                text: ch.to_string(),
                hit,
            }),
        }
    }
    parts
}

fn search_terms(query: &str) -> Vec<Vec<char>> {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .take(10)
        .map(|term| term.chars().map(fold).collect())
        .collect()
}

fn mark_prefix_matches(text: &[char], term: &[char], hits: &mut [bool]) {
    if term.is_empty() || term.len() > text.len() {
        return;
    }
    for start in 0..=text.len() - term.len() {
        let at_word_start = start == 0 || !text[start - 1].is_alphanumeric();
        if at_word_start && text[start..start + term.len()] == *term {
            hits[start..start + term.len()].fill(true);
        }
    }
}

/// Lowercase form without the accent, one character for one character.
fn fold(c: char) -> char {
    let lower = c.to_lowercase().next().unwrap_or(c);
    match lower {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked(parts: &[TitlePart]) -> Vec<(&str, bool)> {
        parts.iter().map(|p| (p.text.as_str(), p.hit)).collect()
    }

    #[test]
    fn words_starting_with_a_term_are_marked() {
        let parts = title_parts("Lenteurs sur le stockage", Some("stock"));
        assert_eq!(
            marked(&parts),
            vec![("Lenteurs sur le ", false), ("stock", true), ("age", false)]
        );
    }

    #[test]
    fn accents_and_case_are_ignored() {
        let parts = title_parts("Panne RÉSEAU", Some("reseau"));
        assert_eq!(marked(&parts), vec![("Panne ", false), ("RÉSEAU", true)]);
    }

    #[test]
    fn letters_that_lengthen_when_lowercased_never_panic() {
        let parts = title_parts("İstanbul outage", Some("outage"));
        assert_eq!(marked(&parts), vec![("İstanbul ", false), ("outage", true)]);
        let parts = title_parts("İİİ", Some("i"));
        let text: String = parts.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(text, "İİİ");
    }

    #[test]
    fn mid_word_matches_are_not_marked() {
        let parts = title_parts("stockage", Some("age"));
        assert_eq!(marked(&parts), vec![("stockage", false)]);
    }

    #[test]
    fn no_query_gives_one_plain_run() {
        assert_eq!(
            marked(&title_parts("Outage", None)),
            vec![("Outage", false)]
        );
        assert!(title_parts("", Some("x")).is_empty());
    }
}
