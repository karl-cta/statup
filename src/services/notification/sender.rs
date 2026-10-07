//! Sending one message to one destination.

use std::time::Duration;

use anyhow::Context;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use reqwest::StatusCode;
use reqwest::header::{CONTENT_TYPE, RETRY_AFTER};

use super::mail::{email, recipients};
use super::{Facts, Notice, post};
use crate::config::{SmtpConfig, SmtpSecurity};
use crate::models::{Channel, ChannelKind};
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
    /// An address the tool's format cannot use, such as an ntfy address
    /// without its topic.
    Unsupported,
    /// The mail server refused the message (5xx), with its code if it gave
    /// one: a wrong password, an unknown recipient.
    MailRefused(Option<u16>),
    /// The mail server asked to try later (4xx).
    MailBusy(Option<u16>),
    /// The mail server could not be reached.
    MailUnreachable,
    /// An email destination, and no mail server set on this instance.
    NoMailServer,
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
            Self::MailRefused(status) => with_status("mail_refused", status),
            Self::MailBusy(status) => with_status("mail_busy", status),
            Self::MailUnreachable => "mail_unreachable".to_string(),
            Self::NoMailServer => "no_mail_server".to_string(),
        }
    }

    /// The failure a stored code stands for. A wait asked for by the tool is
    /// not stored.
    pub fn from_code(code: &str) -> Option<Self> {
        if let Some(status) = code.strip_prefix("status_") {
            return status.parse().ok().map(Self::Status);
        }
        if let Some(status) = code.strip_prefix("mail_refused") {
            return Some(Self::MailRefused(status_of(status)));
        }
        if let Some(status) = code.strip_prefix("mail_busy") {
            return Some(Self::MailBusy(status_of(status)));
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
            "mail_unreachable" => Self::MailUnreachable,
            "no_mail_server" => Self::NoMailServer,
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
            Self::Redirected
            | Self::Unsupported
            | Self::Gone
            | Self::MailRefused(_)
            | Self::NoMailServer => true,
            Self::TooManyRequests(_)
            | Self::Transport(_)
            | Self::MailBusy(_)
            | Self::MailUnreachable => false,
        }
    }
}

/// `mail_refused_535`, or `mail_refused` when the server gave no code.
fn with_status(code: &str, status: Option<u16>) -> String {
    status.map_or_else(|| code.to_string(), |status| format!("{code}_{status}"))
}

fn status_of(suffix: &str) -> Option<u16> {
    suffix
        .strip_prefix('_')
        .and_then(|status| status.parse().ok())
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

/// The clients of the notifications, shared by the background task and the
/// test button of the settings: HTTP for the tools, SMTP for email when a
/// mail server is set.
pub struct Notifier {
    client: reqwest::Client,
    mailer: Option<Mailer>,
}

struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl Notifier {
    pub fn new(timeout: Duration, smtp: Option<&SmtpConfig>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("statup")
            .build()?;
        let mailer = smtp
            .map(|config| Mailer::new(config, timeout))
            .transpose()?;
        Ok(Self { client, mailer })
    }

    /// Whether email destinations can be sent: a mail server is set.
    pub fn mail_ready(&self) -> bool {
        self.mailer.is_some()
    }

    async fn send_mail(
        &self,
        channel: &Channel,
        notice: &Notice,
        facts: &Facts,
    ) -> Result<(), Failure> {
        let mailer = self.mailer.as_ref().ok_or(Failure::NoMailServer)?;
        let recipients = recipients(&channel.target).ok_or(Failure::MailRefused(None))?;
        let message = email(&mailer.from, &recipients, notice, facts, &channel.locale)
            .map_err(|_| Failure::MailRefused(None))?;
        mailer
            .transport
            .send(message)
            .await
            .map(drop)
            .map_err(|e| classify_smtp(&e))
    }

    async fn post_json(
        &self,
        channel: &Channel,
        notice: &Notice,
        facts: &Facts,
    ) -> Result<(), Failure> {
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

impl Mailer {
    fn new(config: &SmtpConfig, timeout: Duration) -> anyhow::Result<Self> {
        let from = config
            .from
            .parse::<Mailbox>()
            .with_context(|| format!("SMTP_FROM is not an email address: {}", config.from))?;
        let builder = match config.security {
            SmtpSecurity::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)?
            }
            SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)?,
            SmtpSecurity::None => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
            }
        };
        let mut builder = builder.port(config.port).timeout(Some(timeout));
        if let Some((user, password)) = &config.credentials {
            builder = builder.credentials(Credentials::new(user.clone(), password.clone()));
        }
        Ok(Self {
            transport: builder.build(),
            from,
        })
    }
}

impl Sender for Notifier {
    async fn send(&self, channel: &Channel, notice: &Notice, facts: &Facts) -> Result<(), Failure> {
        if channel.kind == ChannelKind::Email {
            return self.send_mail(channel, notice, facts).await;
        }
        self.post_json(channel, notice, facts).await
    }
}

/// What the mail server said, as a failure: refused, busy, or not reached.
fn classify_smtp(error: &lettre::transport::smtp::Error) -> Failure {
    let status = error
        .status()
        .and_then(|code| code.to_string().parse().ok());
    if error.is_permanent() {
        return Failure::MailRefused(status);
    }
    if error.is_transient() {
        return Failure::MailBusy(status);
    }
    if error.is_timeout() {
        return Failure::Transport(Finding::TimedOut);
    }
    Failure::MailUnreachable
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
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{TcpListener, TcpStream};

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
        let notifier = Notifier::new(Duration::from_secs(5), None).unwrap();
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

        let notifier = Notifier::new(Duration::from_secs(5), None).unwrap();
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
    async fn an_email_destination_needs_a_mail_server() {
        let failure = send_to(ChannelKind::Email, "ops@example.com".to_string())
            .await
            .unwrap_err();
        assert_eq!(failure, Failure::NoMailServer);
        assert!(failure.is_permanent());
    }

    /// A mail server that accepts every command but `RCPT`, which it answers
    /// with `rcpt_reply`, and keeps every line it reads.
    async fn mail_server(rcpt_reply: &'static str) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&lines);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(converse(stream, rcpt_reply, Arc::clone(&log)));
            }
        });
        (addr, lines)
    }

    async fn converse(stream: TcpStream, rcpt_reply: &'static str, log: Arc<Mutex<Vec<String>>>) {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let _ = writer.write_all(b"220 mail.example.com ESMTP\r\n").await;
        let mut in_data = false;
        let mut line = String::new();
        while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            log.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(line.trim_end().to_string());
            let command = line.to_ascii_uppercase();
            let reply: &[u8] = if in_data {
                in_data = line != ".\r\n";
                if in_data { b"" } else { b"250 queued\r\n" }
            } else if command.starts_with("EHLO") {
                b"250-mail.example.com\r\n250 OK\r\n"
            } else if command.starts_with("RCPT") {
                rcpt_reply.as_bytes()
            } else if command.starts_with("DATA") {
                in_data = true;
                b"354 go ahead\r\n"
            } else if command.starts_with("QUIT") {
                b"221 bye\r\n"
            } else {
                b"250 OK\r\n"
            };
            let _ = writer.write_all(reply).await;
            line.clear();
        }
    }

    async fn mail_to(addr: SocketAddr, target: &str) -> Result<(), Failure> {
        let smtp = SmtpConfig {
            host: addr.ip().to_string(),
            port: addr.port(),
            security: SmtpSecurity::None,
            credentials: None,
            from: "Statup <status@example.com>".to_string(),
        };
        let notifier = Notifier::new(Duration::from_secs(5), Some(&smtp)).unwrap();
        assert!(notifier.mail_ready());
        let channel = channel(ChannelKind::Email, target.to_string());
        let (notice, facts) = test_message(&channel);
        notifier.send(&channel, &notice, &facts).await
    }

    #[tokio::test]
    async fn an_email_reaches_every_address_in_blind_copy() {
        let (addr, lines) = mail_server("250 OK\r\n").await;

        mail_to(addr, "it@example.com, board@example.com")
            .await
            .unwrap();

        let lines = lines.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert!(lines.contains(&"MAIL FROM:<status@example.com>".to_string()));
        assert!(lines.contains(&"RCPT TO:<it@example.com>".to_string()));
        assert!(lines.contains(&"RCPT TO:<board@example.com>".to_string()));
        assert!(lines.iter().any(|line| line.starts_with("Subject: ")));
        assert!(!lines.iter().any(|line| line.starts_with("Bcc")));
    }

    #[tokio::test]
    async fn a_refused_recipient_is_a_permanent_failure() {
        let (addr, _) = mail_server("550 5.1.1 unknown user\r\n").await;

        let failure = mail_to(addr, "nobody@example.com").await.unwrap_err();

        assert_eq!(failure, Failure::MailRefused(Some(550)));
        assert!(failure.is_permanent());
    }

    #[tokio::test]
    async fn a_busy_mail_server_is_tried_again() {
        let (addr, _) = mail_server("451 4.7.1 try again later\r\n").await;

        let failure = mail_to(addr, "it@example.com").await.unwrap_err();

        assert_eq!(failure, Failure::MailBusy(Some(451)));
        assert!(!failure.is_permanent());
    }

    #[test]
    fn a_sender_that_is_no_address_stops_the_start() {
        let smtp = SmtpConfig {
            host: "mail.example.com".to_string(),
            port: 587,
            security: SmtpSecurity::StartTls,
            credentials: None,
            from: "Statup".to_string(),
        };
        assert!(Notifier::new(Duration::from_secs(5), Some(&smtp)).is_err());
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
            Failure::MailRefused(Some(535)),
            Failure::MailRefused(None),
            Failure::MailBusy(Some(451)),
            Failure::MailUnreachable,
            Failure::NoMailServer,
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
