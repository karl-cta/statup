//! Sending one message to one destination.

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{CONTENT_TYPE, RETRY_AFTER};

use super::{Facts, Notice, post};
use crate::models::Channel;
use crate::services::Finding;
use crate::services::probe::classify_http;

/// Longest wait for a destination to answer.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// Why a message did not go out, stored as a short code that the settings
/// page words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The tool answered with this status: refused (4xx) or broken (5xx).
    Status(u16),
    /// 429, with the delay the tool asked for, if any.
    TooManyRequests(Option<Duration>),
    /// A redirection: a webhook address never moves, so it is followed
    /// nowhere.
    Redirected,
    /// The request did not get an answer.
    Transport(Finding),
    /// A kind that is not posted over HTTP: email.
    Unsupported,
    /// The event or service the message is about no longer exists.
    Gone,
}

impl Failure {
    pub fn code(self) -> String {
        match self {
            Self::Status(status) => format!("status_{status}"),
            Self::TooManyRequests(_) => "too_many_requests".to_string(),
            Self::Redirected => "redirected".to_string(),
            Self::Transport(finding) => transport_code(finding).to_string(),
            Self::Unsupported => "unsupported".to_string(),
            Self::Gone => "gone".to_string(),
        }
    }

    /// The failure a stored code stands for. A wait asked for by the tool is
    /// not stored.
    pub fn from_code(code: &str) -> Option<Self> {
        if let Some(status) = code.strip_prefix("status_") {
            return status.parse().ok().map(Self::Status);
        }
        let failure = match code {
            "too_many_requests" => Self::TooManyRequests(None),
            "redirected" => Self::Redirected,
            "refused" => Self::Transport(Finding::Refused),
            "timed_out" => Self::Transport(Finding::TimedOut),
            "name_not_found" => Self::Transport(Finding::NameNotFound),
            "certificate" => Self::Transport(Finding::Certificate),
            "unreachable" => Self::Transport(Finding::Unreachable),
            "unsupported" => Self::Unsupported,
            "gone" => Self::Gone,
            _ => return None,
        };
        Some(failure)
    }

    /// Waiting cannot fix it: an address refused (4xx other than 408 and
    /// 429), a redirection, an unsupported kind, a vanished subject.
    pub fn is_permanent(self) -> bool {
        match self {
            Self::Status(status) => {
                (400..500).contains(&status)
                    && status != StatusCode::REQUEST_TIMEOUT.as_u16()
                    && status != StatusCode::TOO_MANY_REQUESTS.as_u16()
            }
            Self::Redirected | Self::Unsupported | Self::Gone => true,
            Self::TooManyRequests(_) | Self::Transport(_) => false,
        }
    }
}

fn transport_code(finding: Finding) -> &'static str {
    match finding {
        Finding::Refused => "refused",
        Finding::TimedOut => "timed_out",
        Finding::NameNotFound => "name_not_found",
        Finding::Certificate => "certificate",
        _ => "unreachable",
    }
}

/// Sends a message to a destination. The real one posts over HTTP; tests use
/// fakes.
pub trait Sender {
    fn send(
        &self,
        channel: &Channel,
        notice: &Notice,
        facts: &Facts,
    ) -> impl Future<Output = Result<(), Failure>> + Send;
}

/// The HTTP client of the notifications, shared by the background task and
/// the test button of the settings.
pub struct Notifier {
    client: reqwest::Client,
}

impl Notifier {
    pub fn new(timeout: Duration) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("statup")
            .build()?;
        Ok(Self { client })
    }
}

impl Sender for Notifier {
    async fn send(&self, channel: &Channel, notice: &Notice, facts: &Facts) -> Result<(), Failure> {
        let post =
            post(channel.kind, &channel.target, notice, facts).ok_or(Failure::Unsupported)?;
        let response = self
            .client
            .post(post.url)
            .header(CONTENT_TYPE, "application/json")
            .body(post.body.to_string())
            .send()
            .await
            // The address is a secret: anyone holding it can post.
            .map_err(|e| Failure::Transport(classify_http(&e.without_url())))?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        if status.is_redirection() {
            return Err(Failure::Redirected);
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(Failure::TooManyRequests(retry_after(&response)));
        }
        Err(Failure::Status(status.as_u16()))
    }
}

/// `Retry-After` as a number of seconds; a date is not read.
fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let seconds = response
        .headers()
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex, PoisonError};

    use chrono::{TimeZone, Utc};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::i18n::I18n;
    use crate::models::{ChannelKind, Happening};
    use crate::services::{Origin, Subject};

    /// Answers every request with the same raw response and keeps what it
    /// received.
    async fn serve(response: &'static str) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&received);
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let request = read_request(&mut stream).await;
                    log.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(request);
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (addr, received)
    }

    /// Reads the head, then as many body bytes as `content-length` says.
    async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
        let mut data = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(read) = stream.read(&mut chunk).await {
            if read == 0 {
                break;
            }
            data.extend_from_slice(&chunk[..read]);
            let text = String::from_utf8_lossy(&data);
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let length = head
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")?
                            .trim()
                            .parse()
                            .ok()
                    })
                    .unwrap_or(0usize);
                if body.len() >= length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&data).into_owned()
    }

    async fn closed_addr() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        addr
    }

    fn channel(kind: ChannelKind, target: String) -> Channel {
        let at = Utc.with_ymd_and_hms(2026, 10, 7, 9, 0, 0).unwrap();
        Channel {
            id: 1,
            name: "Team".to_string(),
            kind,
            target,
            locale: "en".to_string(),
            on_incidents: true,
            on_maintenances: true,
            on_publications: true,
            on_detected: true,
            created_at: at,
            updated_at: at,
        }
    }

    /// The test message, written for `channel`.
    fn test_message(channel: &Channel) -> (Notice, Facts) {
        let origin = Origin {
            instance: "Statup".to_string(),
            page: None,
        };
        let at = Utc.with_ymd_and_hms(2026, 10, 7, 9, 0, 0).unwrap();
        let notice = Notice::write(
            Happening::Test,
            None,
            &Subject::Test,
            &origin,
            &I18n::new(&channel.locale),
            at,
        );
        let facts = Facts::of(Happening::Test, None, &Subject::Test, &origin, at);
        (notice, facts)
    }

    async fn send_to(kind: ChannelKind, target: String) -> Result<(), Failure> {
        let channel = channel(kind, target);
        let (notice, facts) = test_message(&channel);
        let notifier = Notifier::new(Duration::from_secs(5)).unwrap();
        notifier.send(&channel, &notice, &facts).await
    }

    async fn answer_of(response: &'static str) -> Result<(), Failure> {
        let (addr, _) = serve(response).await;
        send_to(ChannelKind::Slack, format!("http://{addr}/hook")).await
    }

    #[tokio::test]
    async fn the_body_posted_is_the_json_of_the_format() {
        let (addr, received) =
            serve("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        let channel = channel(ChannelKind::Slack, format!("http://{addr}/hook"));
        let (notice, facts) = test_message(&channel);
        let expected = post(channel.kind, &channel.target, &notice, &facts).unwrap();

        let notifier = Notifier::new(Duration::from_secs(5)).unwrap();
        notifier.send(&channel, &notice, &facts).await.unwrap();

        let requests = received.lock().unwrap_or_else(PoisonError::into_inner);
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(request.starts_with("POST /hook HTTP/1.1"), "{request}");
        assert!(
            request
                .to_lowercase()
                .contains("content-type: application/json"),
            "{request}"
        );
        let (_, body) = request.split_once("\r\n\r\n").unwrap();
        assert_eq!(body, expected.body.to_string());
    }

    #[tokio::test]
    async fn any_success_status_is_a_success() {
        let accepted =
            answer_of("HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert_eq!(accepted.await, Ok(()));
        let empty = answer_of("HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
        assert_eq!(empty.await, Ok(()));
    }

    #[tokio::test]
    async fn a_refused_address_is_permanent() {
        let failure =
            answer_of("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap_err();
        assert_eq!(failure, Failure::Status(404));
        assert!(failure.is_permanent());
    }

    #[tokio::test]
    async fn a_broken_tool_is_worth_another_try() {
        let failure = answer_of(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap_err();
        assert_eq!(failure, Failure::Status(503));
        assert!(!failure.is_permanent());
    }

    #[tokio::test]
    async fn too_many_requests_carries_the_delay_asked_for() {
        let failure = answer_of(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap_err();
        assert_eq!(
            failure,
            Failure::TooManyRequests(Some(Duration::from_secs(120)))
        );
        assert!(!failure.is_permanent());
    }

    #[tokio::test]
    async fn a_retry_after_date_is_not_read() {
        let failure = answer_of(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: Wed, 21 Oct 2026 07:28:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap_err();
        assert_eq!(failure, Failure::TooManyRequests(None));
    }

    #[tokio::test]
    async fn a_redirection_is_not_followed() {
        let failure = answer_of(
            "HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap_err();
        assert_eq!(failure, Failure::Redirected);
        assert!(failure.is_permanent());
    }

    #[tokio::test]
    async fn a_closed_port_is_refused() {
        let addr = closed_addr().await;
        let failure = send_to(ChannelKind::Slack, format!("http://{addr}/hook"))
            .await
            .unwrap_err();
        assert_eq!(failure, Failure::Transport(Finding::Refused));
        assert!(!failure.is_permanent());
    }

    #[tokio::test]
    async fn an_email_destination_cannot_be_sent_yet() {
        let failure = send_to(ChannelKind::Email, "ops@example.com".to_string())
            .await
            .unwrap_err();
        assert_eq!(failure, Failure::Unsupported);
        assert!(failure.is_permanent());
    }

    #[test]
    fn a_failure_code_gives_the_failure_back() {
        let failures = [
            Failure::Status(404),
            Failure::Status(503),
            Failure::TooManyRequests(None),
            Failure::Redirected,
            Failure::Transport(Finding::Refused),
            Failure::Transport(Finding::TimedOut),
            Failure::Transport(Finding::NameNotFound),
            Failure::Transport(Finding::Certificate),
            Failure::Transport(Finding::Unreachable),
            Failure::Unsupported,
            Failure::Gone,
        ];
        for failure in failures {
            assert_eq!(Failure::from_code(&failure.code()), Some(failure));
        }
        assert_eq!(Failure::from_code("unknown"), None);
        assert_eq!(Failure::from_code("status_x"), None);
    }

    #[test]
    fn the_codes_leave_out_what_the_page_does_not_word() {
        let asked = Failure::TooManyRequests(Some(Duration::from_secs(5)));
        assert_eq!(asked.code(), "too_many_requests");
        let loop_ = Failure::Transport(Finding::TooManyRedirects);
        assert_eq!(loop_.code(), "unreachable");
    }

    #[test]
    fn only_a_refused_address_is_permanent_among_statuses() {
        assert!(Failure::Status(401).is_permanent());
        assert!(!Failure::Status(408).is_permanent());
        assert!(!Failure::Status(429).is_permanent());
        assert!(!Failure::Status(500).is_permanent());
    }
}
