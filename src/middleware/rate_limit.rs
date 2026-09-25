//! Request rate limit for the dynamic routes, per client address, and the
//! page shown past it. The budget is sized for an office behind one
//! address opening the page together during an incident, not for one
//! person: brute force on sign-in has its own, stricter limiter.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::header::{CONTENT_TYPE, RETRY_AFTER};
use axum::http::{HeaderValue, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use governor::middleware::NoOpMiddleware;
use tokio::task::AbortHandle;
use tower_governor::GovernorError;
use tower_governor::GovernorLayer;
use tower_governor::governor::{GovernorConfig, GovernorConfigBuilder};
use tower_governor::key_extractor::KeyExtractor;

use super::client_ip::{ClientIpSource, client_ip, limit_key};
use crate::error::AppError;
use crate::i18n::I18n;

/// Requests a minute per client address, also the size of the burst
/// allowed before the limit bites.
pub const DEFAULT_PER_MINUTE: u32 = 600;
const CLEANUP_PERIOD: Duration = Duration::from_secs(60);

type Config = GovernorConfig<ClientIpKeyExtractor, NoOpMiddleware>;

/// Rate limit keyed on the client address, see [`client_ip`].
#[derive(Clone, Debug)]
pub struct ClientIpKeyExtractor {
    source: ClientIpSource,
}

impl KeyExtractor for ClientIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        client_ip(req.headers(), peer, &self.source)
            .map(limit_key)
            .ok_or(GovernorError::UnableToExtractKey)
    }
}

/// The request rate limiter and its per-address state.
#[derive(Clone)]
pub struct RateLimit {
    config: Arc<Config>,
}

impl RateLimit {
    /// # Errors
    ///
    /// Returns an error if the quota constants are invalid.
    pub fn new(source: ClientIpSource) -> anyhow::Result<Self> {
        Self::with_quota(DEFAULT_PER_MINUTE, source)
    }

    /// A limiter allowing `per_minute` requests a minute per address.
    ///
    /// # Errors
    ///
    /// Returns an error when `per_minute` is zero.
    pub fn with_quota(per_minute: u32, source: ClientIpSource) -> anyhow::Result<Self> {
        let replenish_every_ms = 60_000 / u64::from(per_minute.max(1));
        let config = GovernorConfigBuilder::default()
            .key_extractor(ClientIpKeyExtractor { source })
            .per_millisecond(replenish_every_ms)
            .burst_size(per_minute)
            .error_handler(|error| limit_response(&error))
            .finish()
            .ok_or_else(|| anyhow::anyhow!("invalid rate limit quota"))?;
        Ok(Self {
            config: Arc::new(config),
        })
    }

    pub fn layer(&self) -> GovernorLayer<ClientIpKeyExtractor, NoOpMiddleware> {
        GovernorLayer {
            config: Arc::clone(&self.config),
        }
    }

    /// Forgets the addresses whose budget is full again, every minute, so
    /// the state does not grow with every address ever seen.
    pub fn spawn_cleanup_task(&self) -> AbortHandle {
        let limiter = Arc::clone(self.config.limiter());
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(CLEANUP_PERIOD);
            loop {
                interval.tick().await;
                limiter.retain_recent();
                limiter.shrink_to_fit();
            }
        });
        task.abort_handle()
    }
}

/// Marks a rate limited response for `render_error_pages`, which knows the
/// language of the request.
#[derive(Clone, Copy, Debug)]
pub struct RateLimited {
    pub retry_after_secs: u64,
}

fn limit_response(error: &GovernorError) -> Response<Body> {
    match error {
        GovernorError::TooManyRequests { wait_time, .. } => {
            let mut response = StatusCode::TOO_MANY_REQUESTS.into_response();
            response.extensions_mut().insert(RateLimited {
                retry_after_secs: (*wait_time).max(1),
            });
            response
        }
        GovernorError::UnableToExtractKey => {
            AppError::Internal(anyhow::anyhow!("no client address to rate limit")).into_response()
        }
        GovernorError::Other { code, .. } => code.into_response(),
    }
}

/// Files are served outside the rate limit, so the page keeps the app's
/// stylesheet and theme. A slot frees in under a second, so the page
/// reloads itself shortly.
const RATE_LIMITED_PAGE: &str = r#"<!doctype html>
<html lang="{lang}"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="light dark">
<meta http-equiv="refresh" content="{seconds}">
<title>{title}</title>
<link rel="stylesheet" href="{stylesheet}">
<script src="{theme}"></script>
</head>
<body><div class="auth-page"><main class="auth-panel">
  <h1 class="auth-title">{title}</h1>
  <p class="auth-intro">{body}</p>
</main></div></body></html>"#;

/// The 429 page in the language of the request.
pub fn rate_limited_page(i18n: &I18n, retry_after_secs: u64) -> Response {
    let html = RATE_LIMITED_PAGE
        .replace("{stylesheet}", &crate::asset("css/style.css"))
        .replace("{theme}", &crate::asset("js/theme.js"))
        .replace("{lang}", i18n.locale())
        .replace("{seconds}", &retry_after_secs.to_string())
        .replace("{title}", &escape_html(i18n.t("error.rate_limited_title")))
        .replace("{body}", &escape_html(i18n.t("error.rate_limited_body")));
    let mut response = (StatusCode::TOO_MANY_REQUESTS, html).into_response();
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(RETRY_AFTER, HeaderValue::from(retry_after_secs));
    response
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_speaks_the_language_of_the_request() {
        let response = rate_limited_page(&I18n::new("en"), 2);
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
            Some("2")
        );
    }

    #[test]
    fn page_text_is_escaped() {
        assert_eq!(
            escape_html(r#"<a href="x">'&'</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn the_quota_is_valid() {
        assert!(RateLimit::new(ClientIpSource::Peer).is_ok());
        assert!(RateLimit::with_quota(1, ClientIpSource::Peer).is_ok());
        assert!(RateLimit::with_quota(0, ClientIpSource::Peer).is_err());
    }
}
