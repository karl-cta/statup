//! Network checks: whether a web address answers or a port accepts a
//! connection. The body of a page is never read.

use std::time::Duration;

use super::monitoring::Outcome;
use crate::models::{CheckKind, CheckedService};

/// Longest wait for one check.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 5;
/// From this status on, the server is there but not working.
const FIRST_SERVER_ERROR: u16 = 500;

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
}

/// Runs the check a service is set up with; tests swap in a scripted one.
pub trait Prober: Send + Sync + 'static {
    fn check(&self, service: &CheckedService) -> impl Future<Output = Outcome> + Send;
}

impl Prober for Probes {
    async fn check(&self, service: &CheckedService) -> Outcome {
        match service.kind {
            CheckKind::Http => {
                let client = if service.internal_cert {
                    &self.lenient
                } else {
                    &self.strict
                };
                probe_http(client, &service.target).await
            }
            CheckKind::Tcp => probe_tcp(&service.target, self.timeout).await,
        }
    }
}

fn client_builder(timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .user_agent("statup")
}

/// A response below 500 counts as an answer, even a refusal (401, 404).
async fn probe_http(client: &reqwest::Client, url: &str) -> Outcome {
    // The URL stays out of the logs: it may carry a token.
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(e) => {
            tracing::debug!(error = %e, "HTTP check failed");
            return Outcome::Failed;
        }
    };
    let status = response.status();
    if status.as_u16() >= FIRST_SERVER_ERROR {
        tracing::debug!(%status, "HTTP check failed");
        return Outcome::Failed;
    }
    Outcome::Answered
}

/// An accepted connection counts as an answer; nothing is sent over it.
async fn probe_tcp(target: &str, timeout: Duration) -> Outcome {
    let error = match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(target)).await {
        Ok(Ok(_stream)) => return Outcome::Answered,
        Ok(Err(e)) => e.to_string(),
        Err(elapsed) => elapsed.to_string(),
    };
    tracing::debug!(%error, "Port check failed");
    Outcome::Failed
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
            kind,
            target,
            internal_cert: false,
            detected_status: None,
        }
    }

    #[tokio::test]
    async fn open_port_answers() {
        let addr = serve(OK).await;
        let outcome = probe_tcp(&addr.to_string(), CHECK_TIMEOUT).await;
        assert_eq!(outcome, Outcome::Answered);
    }

    #[tokio::test]
    async fn closed_port_fails() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let outcome = probe_tcp(&addr.to_string(), CHECK_TIMEOUT).await;
        assert_eq!(outcome, Outcome::Failed);
    }

    #[tokio::test]
    async fn status_200_answers() {
        let addr = serve(OK).await;
        let outcome = probe_http(&probes().strict, &format!("http://{addr}/")).await;
        assert_eq!(outcome, Outcome::Answered);
    }

    #[tokio::test]
    async fn status_401_answers() {
        let addr = serve(UNAUTHORIZED).await;
        let outcome = probe_http(&probes().strict, &format!("http://{addr}/")).await;
        assert_eq!(outcome, Outcome::Answered);
    }

    #[tokio::test]
    async fn status_500_fails() {
        let addr = serve(SERVER_ERROR).await;
        let outcome = probe_http(&probes().strict, &format!("http://{addr}/")).await;
        assert_eq!(outcome, Outcome::Failed);
    }

    #[tokio::test]
    async fn endless_redirect_fails() {
        let addr = serve(REDIRECT_LOOP).await;
        let outcome = probe_http(&probes().strict, &format!("http://{addr}/")).await;
        assert_eq!(outcome, Outcome::Failed);
    }

    #[tokio::test]
    async fn silent_server_fails_after_the_timeout() {
        let addr = serve_silence().await;
        let probes = Probes::new(Duration::from_millis(300)).unwrap();
        let started = Instant::now();
        let outcome = probe_http(&probes.strict, &format!("http://{addr}/")).await;
        assert_eq!(outcome, Outcome::Failed);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn invalid_targets_fail_without_panicking() {
        let probes = probes();
        assert_eq!(
            probe_http(&probes.strict, "not a url").await,
            Outcome::Failed
        );
        assert_eq!(
            probe_tcp("nowhere", Duration::from_secs(1)).await,
            Outcome::Failed
        );
    }

    #[tokio::test]
    async fn check_dispatches_on_the_kind() {
        let probes = probes();
        let web = serve(OK).await;
        let port = serve(OK).await;
        let http = checked(CheckKind::Http, format!("http://{web}/"));
        let tcp = checked(CheckKind::Tcp, port.to_string());
        assert_eq!(probes.check(&http).await, Outcome::Answered);
        assert_eq!(probes.check(&tcp).await, Outcome::Answered);
    }
}
