//! Request rate limit for the dynamic routes: 100 requests a minute per
//! client address, and the page shown past it.

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

use super::client_ip::client_ip;
use crate::error::AppError;
use crate::i18n::I18n;

/// One request slot comes back every 600 ms, 100 a minute.
const REPLENISH_EVERY_MS: u64 = 600;
const BURST: u32 = 100;
const CLEANUP_PERIOD: Duration = Duration::from_secs(60);

type Config = GovernorConfig<ClientIpKeyExtractor, NoOpMiddleware>;

/// Rate limit keyed on the client address, see [`client_ip`].
#[derive(Clone, Copy, Debug)]
pub struct ClientIpKeyExtractor {
    trust_proxy: bool,
}

impl KeyExtractor for ClientIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        client_ip(req.headers(), peer, self.trust_proxy).ok_or(GovernorError::UnableToExtractKey)
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
    pub fn new(trust_proxy: bool) -> anyhow::Result<Self> {
        let config = GovernorConfigBuilder::default()
            .key_extractor(ClientIpKeyExtractor { trust_proxy })
            .per_millisecond(REPLENISH_EVERY_MS)
            .burst_size(BURST)
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

/// Self contained, so it renders even while the stylesheet is out of reach.
/// A slot frees in under a second, so the page reloads itself shortly.
const RATE_LIMITED_PAGE: &str = r#"<!doctype html>
<html lang="{lang}"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="{seconds}">
<title>{title}</title>
<style>
  :root { color-scheme: light dark; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center;
         background: #FFFFFF; color: #1C1B18; text-align: center; padding: 24px;
         font: 400 14px/1.6 -apple-system, BlinkMacSystemFont, system-ui, sans-serif; }
  h1 { font-size: 20px; font-weight: 600; letter-spacing: -0.01em; margin: 0 0 8px; }
  p { margin: 0; color: #5F5C52; max-width: 44ch; }
  @media (prefers-color-scheme: dark) {
    body { background: #1C1B18; color: #FAF8F2; }
    p { color: #9C9689; }
  }
</style></head>
<body><main>
  <h1>{title}</h1>
  <p>{body}</p>
</main></body></html>"#;

/// The 429 page in the language of the request.
pub fn rate_limited_page(i18n: &I18n, retry_after_secs: u64) -> Response {
    let html = RATE_LIMITED_PAGE
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
        assert!(RateLimit::new(false).is_ok());
    }
}
