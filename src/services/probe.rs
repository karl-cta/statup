//! Network checks: whether a web address answers or a port accepts a
//! connection. The body of a checked page is never read.

use std::time::{Duration, Instant};

use super::monitoring::Outcome;
use crate::models::{CheckKind, CheckedService, ServiceCheck};

/// Longest wait for one check.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 5;
/// From this status on, the server is there but not working.
const FIRST_SERVER_ERROR: u16 = 500;

/// The body of an answer, or none as soon as it passes `limit` bytes: the
/// pages Statup reads are small, and a server must not fill its memory.
pub(crate) async fn read_within(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Option<Vec<u8>>, reqwest::Error> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Ok(None);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > limit {
            return Ok(None);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Some(body))
}

/// Why a check answered or failed, in words an editor can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finding {
    /// The server answered: an HTTP status below 500, or `None` for a port.
    Answered { status: Option<u16> },
    /// Nothing listens there: the connection was refused.
    Refused,
    /// No answer within the delay.
    TimedOut,
    /// The name matches no server.
    NameNotFound,
    /// No name server answered: Statup's own resolver, or the name's.
    NameServerSilent,
    /// Statup has no network route at all.
    NoRoute,
    /// The certificate was refused.
    Certificate,
    /// The server answered with an error status, 500 or above.
    ServerError(u16),
    /// The address redirects too many times.
    TooManyRedirects,
    /// Any other connection failure.
    Unreachable,
}

/// What one check found, how long it took, and the raw error behind a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub finding: Finding,
    /// The error chain, never the address checked.
    pub detail: Option<String>,
    pub elapsed: Duration,
}

impl Finding {
    /// Why a check failed, in English, for the journal.
    pub fn words(self) -> String {
        match self {
            Self::Answered { .. } => "answered".into(),
            Self::Refused => "connection refused".into(),
            Self::TimedOut => "no answer in time".into(),
            Self::NameNotFound => "name not found".into(),
            Self::NameServerSilent => "name server not answering".into(),
            Self::NoRoute => "no network route".into(),
            Self::Certificate => "certificate refused".into(),
            Self::ServerError(status) => format!("server error {status}"),
            Self::TooManyRedirects => "too many redirects".into(),
            Self::Unreachable => "unreachable".into(),
        }
    }
}

impl Finding {
    /// Whether the failure says Statup itself is off the network, rather
    /// than anything about the service.
    pub fn is_network_loss(self) -> bool {
        matches!(self, Self::NameServerSilent | Self::NoRoute)
    }
}

impl Report {
    /// Whether the service answered, whatever the reason it did not.
    pub fn outcome(&self) -> Outcome {
        match self.finding {
            Finding::Answered { .. } => Outcome::Answered,
            _ => Outcome::Failed,
        }
    }
}

/// The HTTP clients the checks share, built once.
pub struct Probes {
    strict: reqwest::Client,
    /// Accepts a certificate the instance cannot verify, for internal tools.
    lenient: reqwest::Client,
    timeout: Duration,
}

impl Probes {
    /// Builds the shared clients, each giving up after `timeout`.
    pub fn new(timeout: Duration) -> Result<Self, reqwest::Error> {
        Ok(Self {
            strict: client_builder(timeout).build()?,
            lenient: client_builder(timeout)
                .danger_accept_invalid_certs(true)
                .build()?,
            timeout,
        })
    }

    /// The client for a web address, lenient for an internal certificate.
    pub fn http(&self, internal_cert: bool) -> &reqwest::Client {
        if internal_cert {
            &self.lenient
        } else {
            &self.strict
        }
    }

    /// Runs a check and says what it found.
    pub async fn examine(&self, check: &ServiceCheck) -> Report {
        match check.kind {
            CheckKind::Http => probe_http(self.http(check.internal_cert), &check.target).await,
            CheckKind::Tcp => probe_tcp(&check.target, self.timeout).await,
        }
    }
}

/// Runs the check a service is set up with; tests swap in a scripted one.
pub trait Prober: Send + Sync + 'static {
    fn check(&self, service: &CheckedService) -> impl Future<Output = Report> + Send;
}

impl Prober for Probes {
    async fn check(&self, service: &CheckedService) -> Report {
        let check = ServiceCheck {
            kind: service.kind,
            target: service.target.clone(),
            internal_cert: service.internal_cert,
        };
        self.examine(&check).await
    }
}

fn client_builder(timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .user_agent("statup")
}

/// A response below 500 counts as an answer, even a refusal (401, 404).
async fn probe_http(client: &reqwest::Client, url: &str) -> Report {
    let started = Instant::now();
    let (finding, detail) = match client.get(url).send().await {
        Ok(response) => (classify_status(response.status().as_u16()), None),
        Err(e) => {
            // The address is removed from the error: it may carry a token.
            let e = e.without_url();
            (classify_http(&e), Some(error_chain(&e)))
        }
    };
    Report {
        finding,
        detail,
        elapsed: started.elapsed(),
    }
}

/// An accepted connection counts as an answer; nothing is sent over it.
async fn probe_tcp(target: &str, timeout: Duration) -> Report {
    let started = Instant::now();
    let (finding, detail) =
        match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(target)).await {
            Ok(Ok(_stream)) => (Finding::Answered { status: None }, None),
            Ok(Err(e)) => (classify_io(&e), Some(error_chain(&e))),
            Err(elapsed) => (Finding::TimedOut, Some(error_chain(&elapsed))),
        };
    Report {
        finding,
        detail,
        elapsed: started.elapsed(),
    }
}

fn classify_status(status: u16) -> Finding {
    if status >= FIRST_SERVER_ERROR {
        return Finding::ServerError(status);
    }
    Finding::Answered {
        status: Some(status),
    }
}

pub(crate) fn classify_http(error: &reqwest::Error) -> Finding {
    if error.is_timeout() {
        return Finding::TimedOut;
    }
    if error.is_redirect() {
        return Finding::TooManyRedirects;
    }
    classify_chain(error)
}

/// Looks through an error and its causes for the first one that says why.
/// A failed lookup is judged on the whole chain: its outer layer only says
/// "dns error", the cause underneath tells which kind.
fn classify_chain(error: &(dyn std::error::Error + 'static)) -> Finding {
    let mut link = Some(error);
    while let Some(current) = link {
        if let Some(finding) = current
            .downcast_ref::<std::io::Error>()
            .and_then(|io_error| classify_io_kind(io_error.kind()))
        {
            return finding;
        }
        if current.to_string().to_lowercase().contains("certificate") {
            return Finding::Certificate;
        }
        link = current.source();
    }
    classify_lookup(&error_chain(error).to_lowercase()).unwrap_or(Finding::Unreachable)
}

fn classify_io(error: &std::io::Error) -> Finding {
    classify_io_kind(error.kind())
        .or_else(|| classify_lookup(&error.to_string().to_lowercase()))
        .unwrap_or(Finding::Unreachable)
}

fn classify_io_kind(kind: std::io::ErrorKind) -> Option<Finding> {
    use std::io::ErrorKind;
    match kind {
        ErrorKind::ConnectionRefused => Some(Finding::Refused),
        ErrorKind::TimedOut => Some(Finding::TimedOut),
        ErrorKind::NetworkUnreachable | ErrorKind::NetworkDown => Some(Finding::NoRoute),
        _ => None,
    }
}

/// A failed name lookup, from the system's own words: musl says "Try
/// again" and glibc "Temporary failure" when no name server answers.
fn classify_lookup(text: &str) -> Option<Finding> {
    let silent = ["try again", "temporary failure in name resolution"];
    let unknown = [
        "dns error",
        "failed to lookup address",
        "nodename nor servname",
        "name or service not known",
    ];
    if !unknown.iter().any(|phrase| text.contains(phrase)) {
        return None;
    }
    if silent.iter().any(|phrase| text.contains(phrase)) {
        return Some(Finding::NameServerSilent);
    }
    Some(Finding::NameNotFound)
}

/// An error and its causes, joined from the outermost to the innermost.
fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::time::Instant;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const UNAUTHORIZED: &str =
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const SERVER_ERROR: &str =
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const REDIRECT_LOOP: &str =
        "HTTP/1.1 302 Found\r\nLocation: /\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    /// Answers every request with the same raw response, then closes.
    async fn serve(response: &'static str) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer).await;
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        addr
    }

    /// Accepts connections and keeps them open without ever answering.
    async fn serve_silence() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
            }
        });
        addr
    }

    fn probes() -> Probes {
        Probes::new(Duration::from_secs(5)).unwrap()
    }

    fn checked(kind: CheckKind, target: String) -> CheckedService {
        CheckedService {
            id: 1,
            name: "Intranet".into(),
            kind,
            target,
            internal_cert: false,
            detected_status: None,
            outage_after_minutes: crate::services::DEFAULT_OUTAGE_AFTER,
        }
    }

    fn check_of(service: &CheckedService) -> ServiceCheck {
        ServiceCheck {
            kind: service.kind,
            target: service.target.clone(),
            internal_cert: service.internal_cert,
        }
    }

    /// An address nothing listens on: bound, then released.
    async fn closed_addr() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        addr
    }

    async fn http_report(response: &'static str) -> Report {
        let addr = serve(response).await;
        probe_http(&probes().strict, &format!("http://{addr}/")).await
    }

    #[tokio::test]
    async fn open_port_answers() {
        let addr = serve(OK).await;
        let report = probe_tcp(&addr.to_string(), CHECK_TIMEOUT).await;
        assert_eq!(report.finding, Finding::Answered { status: None });
        assert_eq!(report.outcome(), Outcome::Answered);
    }

    #[tokio::test]
    async fn closed_port_is_refused() {
        let addr = closed_addr().await;
        let report = probe_tcp(&addr.to_string(), CHECK_TIMEOUT).await;
        assert_eq!(report.finding, Finding::Refused);
        assert_eq!(report.outcome(), Outcome::Failed);
        assert!(report.detail.is_some());
    }

    #[tokio::test]
    async fn closed_port_is_refused_over_http() {
        let addr = closed_addr().await;
        let report = probe_http(&probes().strict, &format!("http://{addr}/")).await;
        assert_eq!(report.finding, Finding::Refused);
    }

    #[tokio::test]
    async fn http_error_detail_leaves_the_address_out() {
        let addr = closed_addr().await;
        let report = probe_http(&probes().strict, &format!("http://{addr}/")).await;
        let detail = report.detail.unwrap();
        assert!(!detail.contains("127.0.0.1"), "{detail}");
    }

    #[tokio::test]
    async fn status_200_answers() {
        let report = http_report(OK).await;
        assert_eq!(report.finding, Finding::Answered { status: Some(200) });
        assert_eq!(report.detail, None);
    }

    #[tokio::test]
    async fn status_401_answers() {
        let report = http_report(UNAUTHORIZED).await;
        assert_eq!(report.finding, Finding::Answered { status: Some(401) });
    }

    #[tokio::test]
    async fn status_500_is_a_server_error() {
        let report = http_report(SERVER_ERROR).await;
        assert_eq!(report.finding, Finding::ServerError(500));
        assert_eq!(report.outcome(), Outcome::Failed);
    }

    #[tokio::test]
    async fn endless_redirect_is_reported() {
        let report = http_report(REDIRECT_LOOP).await;
        assert_eq!(report.finding, Finding::TooManyRedirects);
    }

    #[tokio::test]
    async fn silent_server_times_out() {
        let addr = serve_silence().await;
        let probes = Probes::new(Duration::from_millis(300)).unwrap();
        let started = Instant::now();
        let report = probe_http(&probes.strict, &format!("http://{addr}/")).await;
        assert_eq!(report.finding, Finding::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn invalid_targets_fail_without_panicking() {
        let probes = probes();
        let web = probe_http(&probes.strict, "not a url").await;
        assert_eq!(web.outcome(), Outcome::Failed);
        let port = probe_tcp("nowhere", Duration::from_secs(1)).await;
        assert_eq!(port.outcome(), Outcome::Failed);
    }

    /// An outer error that only names its layer, as hyper's "dns error" does.
    #[derive(Debug)]
    struct Layer(&'static str, std::io::Error);

    impl std::fmt::Display for Layer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for Layer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.1)
        }
    }

    fn lookup_failure(words: &str) -> std::io::Error {
        std::io::Error::other(format!("failed to lookup address information: {words}"))
    }

    #[test]
    fn a_silent_name_server_is_told_apart_from_an_unknown_name() {
        assert_eq!(
            classify_io(&lookup_failure("Try again")),
            Finding::NameServerSilent
        );
        assert_eq!(
            classify_io(&lookup_failure("Temporary failure in name resolution")),
            Finding::NameServerSilent
        );
        assert_eq!(
            classify_io(&lookup_failure("Name does not resolve")),
            Finding::NameNotFound
        );
        assert_eq!(
            classify_io(&lookup_failure("Name or service not known")),
            Finding::NameNotFound
        );
    }

    #[test]
    fn a_lookup_wrapped_in_a_dns_layer_is_judged_on_its_cause() {
        let silent = Layer("dns error", lookup_failure("Try again"));
        assert_eq!(classify_chain(&silent), Finding::NameServerSilent);
        let unknown = Layer("dns error", lookup_failure("Name does not resolve"));
        assert_eq!(classify_chain(&unknown), Finding::NameNotFound);
    }

    #[test]
    fn no_route_is_a_network_loss() {
        let unreachable = std::io::Error::from(std::io::ErrorKind::NetworkUnreachable);
        assert_eq!(classify_io(&unreachable), Finding::NoRoute);
        let wrapped = Layer("client error (Connect)", unreachable);
        assert_eq!(classify_chain(&wrapped), Finding::NoRoute);
        let down = std::io::Error::from(std::io::ErrorKind::NetworkDown);
        assert_eq!(classify_io(&down), Finding::NoRoute);
        assert!(Finding::NoRoute.is_network_loss());
        assert!(Finding::NameServerSilent.is_network_loss());
        assert!(!Finding::NameNotFound.is_network_loss());
        assert!(!Finding::Refused.is_network_loss());
    }

    #[tokio::test]
    async fn examine_dispatches_on_the_kind_and_measures_the_time() {
        let probes = probes();
        let web = serve(OK).await;
        let port = serve(OK).await;
        let http = checked(CheckKind::Http, format!("http://{web}/"));
        let tcp = checked(CheckKind::Tcp, port.to_string());
        let http_report = probes.examine(&check_of(&http)).await;
        let tcp_report = probes.examine(&check_of(&tcp)).await;
        assert_eq!(http_report.finding, Finding::Answered { status: Some(200) });
        assert_eq!(tcp_report.finding, Finding::Answered { status: None });
        assert!(http_report.elapsed > Duration::ZERO);
        assert!(tcp_report.elapsed > Duration::ZERO);
    }

    #[tokio::test]
    async fn check_rests_on_the_report() {
        let probes = probes();
        let web = serve(OK).await;
        let closed = closed_addr().await;
        let up = checked(CheckKind::Http, format!("http://{web}/"));
        let down = checked(CheckKind::Tcp, closed.to_string());
        assert_eq!(probes.check(&up).await.outcome(), Outcome::Answered);
        assert_eq!(probes.check(&down).await.outcome(), Outcome::Failed);
    }
}
