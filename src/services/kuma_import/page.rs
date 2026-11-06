//! Reading a public status page of Uptime Kuma: its groups and the names of
//! their monitors, the ones its webhook sends.

use reqwest::Url;
use serde::Deserialize;

use crate::models::CheckKind;
use crate::services::probe::{classify_http, read_within};
use crate::services::{Finding, Probes, target_allowed};

/// Largest page read, in bytes: a page of a few hundred monitors takes a
/// few dozen kilobytes.
const MAX_PAGE_BYTES: usize = 1024 * 1024;
/// The slug Kuma serves at `/status` and `/status-page`.
const DEFAULT_SLUG: &str = "default";

/// A status page as Kuma shows it to anyone, without an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KumaPage {
    pub title: Option<String>,
    /// In the order of the page.
    pub groups: Vec<KumaGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KumaGroup {
    #[serde(default)]
    pub name: String,
    /// In the order of the group.
    #[serde(rename = "monitorList", default)]
    pub monitors: Vec<KumaMonitor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KumaMonitor {
    #[serde(default)]
    pub name: String,
}

/// Why a page could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KumaError {
    /// The address is not that of a status page, and why (a translation key).
    Address(&'static str),
    /// Kuma has no status page under this name.
    NotFound,
    /// Kuma answered with another error status.
    Status(u16),
    /// The server could not be reached, and why.
    Network(Finding),
    /// The answer is larger than any status page.
    TooLarge,
    /// The answer is not a status page of Uptime Kuma.
    NotKuma,
}

/// The `config` and `publicGroupList` of `/api/status-page/<slug>`, the same
/// in Kuma 1.23 and 2.x; the rest is left unread.
#[derive(Deserialize)]
struct StatusPageData {
    #[serde(default)]
    config: Option<PageConfig>,
    #[serde(rename = "publicGroupList")]
    groups: Vec<KumaGroup>,
}

#[derive(Deserialize)]
struct PageConfig {
    #[serde(default)]
    title: Option<String>,
}

/// Where Kuma gives the data of the page an editor pasted: its address in
/// a browser (`/status/<slug>`, `/status` for the default page) or that of
/// its data (`/api/status-page/<slug>`). The URL passes the gate of the
/// checks first, so a hosted instance limits both in one place.
fn status_page_api(raw: &str) -> Result<Url, &'static str> {
    let raw = raw.trim();
    target_allowed(CheckKind::Http, raw).map_err(|key| match key {
        "validation.check_url_credentials" => key,
        "validation.check_url_required" => "validation.kuma_url_required",
        _ => "validation.kuma_url_invalid",
    })?;
    let mut url = Url::parse(raw).map_err(|_| "validation.kuma_url_invalid")?;
    let segments: Vec<&str> = url
        .path_segments()
        .map(|segments| segments.filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let (prefix, slug) = page_of(&segments).ok_or("validation.kuma_url_invalid")?;
    let path = format!(
        "/{}",
        [prefix, &["api", "status-page", &slug]].concat().join("/")
    );
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

/// The path before the page, and the page's slug, lowercased as Kuma does.
fn page_of<'a>(segments: &'a [&'a str]) -> Option<(&'a [&'a str], String)> {
    let (prefix, slug) = match segments {
        [prefix @ .., "api", "status-page", slug] | [prefix @ .., "status", slug] => {
            (prefix, slug.to_lowercase())
        }
        [prefix @ .., "status" | "status-page"] => (prefix, DEFAULT_SLUG.to_string()),
        _ => return None,
    };
    // Kuma's slugs are letters, digits and dashes: anything else is not one.
    let valid = slug
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then_some((prefix, slug))
}

/// Reads the page at the address an editor pasted, with the client of the
/// checks, lenient for an internal certificate.
pub async fn read_status_page(
    probes: &Probes,
    address: &str,
    internal_cert: bool,
) -> Result<KumaPage, KumaError> {
    let api = status_page_api(address).map_err(KumaError::Address)?;
    let response = probes
        .http(internal_cert)
        .get(api)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(network)?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(KumaError::NotFound);
    }
    if !status.is_success() {
        return Err(KumaError::Status(status.as_u16()));
    }
    let body = body_within(response, MAX_PAGE_BYTES).await?;
    parse(&body)
}

/// The groups and monitors of a page, from the data Kuma sends.
fn parse(body: &[u8]) -> Result<KumaPage, KumaError> {
    let data: StatusPageData = serde_json::from_slice(body).map_err(|_| KumaError::NotKuma)?;
    Ok(KumaPage {
        title: data
            .config
            .and_then(|config| config.title)
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty()),
        groups: merge_same_names(data.groups),
    })
}

/// Groups of the same name, whatever the case, become one: their monitors
/// go in one folder anyway, and a ticked name means them all.
fn merge_same_names(groups: Vec<KumaGroup>) -> Vec<KumaGroup> {
    let mut merged: Vec<KumaGroup> = Vec::with_capacity(groups.len());
    for group in groups {
        let name = group.name.trim().to_lowercase();
        match merged
            .iter_mut()
            .find(|kept| kept.name.trim().to_lowercase() == name)
        {
            Some(kept) => kept.monitors.extend(group.monitors),
            None => merged.push(group),
        }
    }
    merged
}

async fn body_within(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, KumaError> {
    read_within(response, limit)
        .await
        .map_err(network)?
        .ok_or(KumaError::TooLarge)
}

/// The address is removed from the error: it may carry a token.
fn network(error: reqwest::Error) -> KumaError {
    KumaError::Network(classify_http(&error.without_url()))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// The data of a page in Kuma 2.5.7, from `getStatusPageData`.
    const KUMA_2: &str = r#"{
        "config": {"slug": "infra", "title": "Infra", "description": null, "icon": "/icon.svg",
            "autoRefreshInterval": 300, "theme": "auto", "published": true, "showTags": false,
            "customCSS": "", "footerText": null, "showPoweredBy": true, "analyticsId": null,
            "showCertificateExpiry": false, "showOnlyLastHeartbeat": false, "rssTitle": null},
        "incidents": [],
        "publicGroupList": [
            {"id": 1, "name": "Infrastructure", "weight": 1, "monitorList": [
                {"id": 3, "name": "Mail", "sendUrl": 0, "type": "http"},
                {"id": 7, "name": "VPN", "sendUrl": 1, "type": "port", "url": "https://vpn.example.com"}
            ]},
            {"id": 2, "name": "Réseau", "weight": 2, "monitorList": []}
        ],
        "maintenanceList": []
    }"#;

    /// The same page in Kuma 1.23.17: one `incident`, otherwise alike.
    const KUMA_1: &str = r#"{
        "config": {"slug": "infra", "title": "Infra", "description": null, "icon": "/icon.svg",
            "theme": "light", "published": true, "showTags": true, "customCSS": "",
            "footerText": null, "showPoweredBy": true, "googleAnalyticsId": null,
            "showCertificateExpiry": false},
        "incident": null,
        "publicGroupList": [
            {"id": 1, "name": "Infrastructure", "weight": 1, "monitorList": [
                {"id": 3, "name": "Mail", "sendUrl": 0, "type": "http", "tags": []},
                {"id": 7, "name": "VPN", "sendUrl": 0, "type": "port", "tags": []}
            ]},
            {"id": 2, "name": "Réseau", "weight": 2, "monitorList": []}
        ],
        "maintenanceList": []
    }"#;

    fn api(raw: &str) -> Result<String, &'static str> {
        status_page_api(raw).map(String::from)
    }

    #[test]
    fn the_address_of_a_page_leads_to_its_data() {
        let cases = [
            (
                "https://kuma.example.com/status/infra",
                "https://kuma.example.com/api/status-page/infra",
            ),
            (
                "https://kuma.example.com/status/Infra/",
                "https://kuma.example.com/api/status-page/infra",
            ),
            (
                "http://10.0.0.5:3001/status/infra?x=1#top",
                "http://10.0.0.5:3001/api/status-page/infra",
            ),
            (
                "https://example.com/kuma/status/infra",
                "https://example.com/kuma/api/status-page/infra",
            ),
            (
                "https://kuma.example.com/status",
                "https://kuma.example.com/api/status-page/default",
            ),
            (
                "https://kuma.example.com/status-page",
                "https://kuma.example.com/api/status-page/default",
            ),
            (
                "https://kuma.example.com/api/status-page/infra",
                "https://kuma.example.com/api/status-page/infra",
            ),
            (
                "  https://kuma.example.com/status/infra  ",
                "https://kuma.example.com/api/status-page/infra",
            ),
        ];
        for (pasted, data) in cases {
            assert_eq!(api(pasted).as_deref(), Ok(data), "{pasted}");
        }
    }

    #[test]
    fn another_address_is_refused_with_why() {
        assert_eq!(api(""), Err("validation.kuma_url_required"));
        for refused in [
            "kuma.example.com/status/infra",
            "ftp://kuma.example.com/status/infra",
            "https://kuma.example.com",
            "https://kuma.example.com/dashboard",
            "https://kuma.example.com/status/infra/extra",
            "https://kuma.example.com/status/in%20fra",
        ] {
            assert_eq!(
                api(refused),
                Err("validation.kuma_url_invalid"),
                "{refused}"
            );
        }
        assert_eq!(
            api("https://admin:secret@kuma.example.com/status/infra"),
            Err("validation.check_url_credentials")
        );
    }

    #[test]
    fn both_versions_give_the_groups_in_order() {
        for body in [KUMA_2, KUMA_1] {
            let page = parse(body.as_bytes()).unwrap();
            assert_eq!(page.title.as_deref(), Some("Infra"));
            let names: Vec<(&str, Vec<&str>)> = page
                .groups
                .iter()
                .map(|g| {
                    (
                        g.name.as_str(),
                        g.monitors.iter().map(|m| m.name.as_str()).collect(),
                    )
                })
                .collect();
            assert_eq!(
                names,
                [("Infrastructure", vec!["Mail", "VPN"]), ("Réseau", vec![])]
            );
        }
    }

    #[test]
    fn groups_of_the_same_name_become_one() {
        let body = r#"{"publicGroupList": [
            {"name": "Services", "monitorList": [{"name": "Mail"}]},
            {"name": "Réseau", "monitorList": [{"name": "VPN"}]},
            {"name": " services", "monitorList": [{"name": "Wiki"}]}
        ]}"#;
        let page = parse(body.as_bytes()).unwrap();
        let names: Vec<(&str, Vec<&str>)> = page
            .groups
            .iter()
            .map(|g| {
                (
                    g.name.as_str(),
                    g.monitors.iter().map(|m| m.name.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [("Services", vec!["Mail", "Wiki"]), ("Réseau", vec!["VPN"])]
        );
    }

    #[test]
    fn another_answer_is_not_a_kuma_page() {
        for body in [
            "<!doctype html><title>Kuma</title>",
            "{}",
            r#"{"status":"ok"}"#,
            "[]",
        ] {
            assert_eq!(parse(body.as_bytes()), Err(KumaError::NotKuma), "{body}");
        }
    }

    /// Answers every request with the same raw response, then closes.
    async fn serve(response: String) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let response = response.clone();
                tokio::spawn(async move {
                    let mut request = [0u8; 2048];
                    let _ = stream.read(&mut request).await;
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        addr
    }

    async fn read_from(response: String, timeout: Duration) -> Result<KumaPage, KumaError> {
        let addr = serve(response).await;
        let probes = Probes::new(timeout).unwrap();
        read_status_page(&probes, &format!("http://{addr}/status/infra"), false).await
    }

    fn ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn a_page_is_read_over_the_network() {
        let page = read_from(ok(KUMA_2), Duration::from_secs(5)).await.unwrap();
        assert_eq!(page.groups.len(), 2);
    }

    #[tokio::test]
    async fn a_missing_page_or_an_error_says_so() {
        let missing = r#"{"status":"fail","msg":"Status Page Not Found"}"#;
        let not_found = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{missing}",
            missing.len()
        );
        assert_eq!(
            read_from(not_found, Duration::from_secs(5)).await,
            Err(KumaError::NotFound)
        );
        let forbidden = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        assert_eq!(
            read_from(forbidden.to_string(), Duration::from_secs(5)).await,
            Err(KumaError::Status(403))
        );
        let html = ok("<!doctype html><title>Login</title>");
        assert_eq!(
            read_from(html, Duration::from_secs(5)).await,
            Err(KumaError::NotKuma)
        );
    }

    #[tokio::test]
    async fn a_page_too_large_is_refused_even_without_its_length() {
        let declared = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_PAGE_BYTES + 1
        );
        assert_eq!(
            read_from(declared, Duration::from_secs(5)).await,
            Err(KumaError::TooLarge)
        );
        let streamed = format!(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}",
            "x".repeat(MAX_PAGE_BYTES + 1)
        );
        assert_eq!(
            read_from(streamed, Duration::from_secs(5)).await,
            Err(KumaError::TooLarge)
        );
    }

    #[tokio::test]
    async fn a_silent_server_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
            }
        });
        let probes = Probes::new(Duration::from_millis(200)).unwrap();
        assert_eq!(
            read_status_page(&probes, &format!("http://{addr}/status/infra"), false).await,
            Err(KumaError::Network(Finding::TimedOut))
        );
    }
}
