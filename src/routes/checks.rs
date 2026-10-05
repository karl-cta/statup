//! The monitoring part of a service form, shared by the service page and the
//! first launch: what the fields show, how they are read, and the Test
//! button.

use askama::Template;
use axum::response::Response;
use serde::Deserialize;

use super::render;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{HtmlForm, RequirePublisher};
use crate::models::{CheckKind, ServiceCheck, Tone};
use crate::services::{CHECK_TIMEOUT, Finding, Outcome, Probes, Report, target_allowed};

/// What the monitoring fields show.
#[derive(Debug, Clone, Default)]
pub struct CheckFields {
    pub kind: Option<CheckKind>,
    pub url: String,
    pub host: String,
    pub port: String,
    pub internal_cert: bool,
}

impl CheckFields {
    /// Whether the fields show this kind, `none` for no monitoring.
    pub fn is(&self, kind: &str) -> bool {
        self.kind.map_or("none", CheckKind::as_str) == kind
    }

    /// The fields of a saved check; a port target is split back into its
    /// host and its port.
    pub fn from_saved(kind: Option<CheckKind>, target: Option<&str>, internal_cert: bool) -> Self {
        let target = target.unwrap_or_default();
        match kind {
            Some(CheckKind::Http) => Self {
                kind,
                url: target.to_string(),
                internal_cert,
                ..Self::default()
            },
            Some(CheckKind::Tcp) => {
                let (host, port) = split_target(target);
                Self {
                    kind,
                    host,
                    port,
                    ..Self::default()
                }
            }
            None => Self::default(),
        }
    }
}

/// The monitoring fields as a form sends them.
#[derive(Debug, Default, Deserialize)]
pub struct CheckInput {
    #[serde(default, rename = "check_kind")]
    kind: String,
    #[serde(default, rename = "check_url")]
    url: String,
    #[serde(default, rename = "check_host")]
    host: String,
    #[serde(default, rename = "check_port")]
    port: String,
    #[serde(default, rename = "check_internal_cert")]
    internal_cert: Option<String>,
}

impl CheckInput {
    /// The fields as typed, to show them again after a refusal.
    pub fn fields(&self) -> CheckFields {
        CheckFields {
            kind: self.kind.parse().ok(),
            url: self.url.clone(),
            host: self.host.clone(),
            port: self.port.clone(),
            internal_cert: self.internal_cert.is_some(),
        }
    }

    pub fn check(&self) -> Result<Option<ServiceCheck>, AppError> {
        parse_check(
            &self.kind,
            &self.url,
            &self.host,
            &self.port,
            self.internal_cert.is_some(),
        )
    }
}

/// What to check, read from the fields of the chosen kind; those of the
/// other kind are ignored, so a page without script that sends them all
/// still works.
pub fn parse_check(
    kind: &str,
    url: &str,
    host: &str,
    port: &str,
    internal_cert: bool,
) -> Result<Option<ServiceCheck>, AppError> {
    let kind = match kind {
        "" | "none" => return Ok(None),
        other => other
            .parse::<CheckKind>()
            .map_err(|()| AppError::validation("error.invalid_data"))?,
    };
    let target = match kind {
        CheckKind::Http => url.trim().to_string(),
        CheckKind::Tcp => join_target(host.trim(), port.trim())?,
    };
    target_allowed(kind, &target).map_err(AppError::validation)?;
    Ok(Some(ServiceCheck {
        kind,
        target,
        internal_cert: kind == CheckKind::Http && internal_cert,
    }))
}

/// `host:port`, an IPv6 address in brackets.
fn join_target(host: &str, port: &str) -> Result<String, AppError> {
    if host.is_empty() {
        return Err(AppError::validation("validation.check_address_required"));
    }
    if !port.parse::<u16>().is_ok_and(|port| port > 0) {
        return Err(AppError::validation("validation.check_port_invalid"));
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.contains(':') {
        return Ok(format!("[{host}]:{port}"));
    }
    Ok(format!("{host}:{port}"))
}

fn split_target(target: &str) -> (String, String) {
    match target.rsplit_once(':') {
        Some((host, port)) => (
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .to_string(),
            port.to_string(),
        ),
        None => (target.to_string(), String::new()),
    }
}

#[derive(Template)]
#[template(path = "services/_check_result.html")]
struct CheckResultFragment {
    /// The tone of the verdict; none when the address itself was refused.
    tone: Option<&'static str>,
    message: String,
    /// What the monitoring will do, said once the address answers.
    rule: Option<String>,
    detail: Option<String>,
    i18n: I18n,
}

/// Runs the check the form describes, without saving anything, and says
/// what the monitoring would make of it.
pub async fn test_check(
    _publisher: RequirePublisher,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<CheckInput>,
) -> Result<Response, AppError> {
    let check = match input.check() {
        Ok(Some(check)) => check,
        Ok(None) => return Err(AppError::validation("error.invalid_data")),
        Err(AppError::Validation(key)) => {
            let message = i18n.t(&key).to_string();
            return render(&CheckResultFragment {
                tone: None,
                message,
                rule: None,
                detail: None,
                i18n,
            });
        }
        Err(e) => return Err(e),
    };
    let probes = Probes::new(CHECK_TIMEOUT).map_err(anyhow::Error::from)?;
    let report = probes.examine(&check).await;
    let (tone, message) = verdict(&report, &i18n);
    let rule = (report.outcome() == Outcome::Answered)
        .then(|| i18n.t("services.check_result_rule").to_string());
    render(&CheckResultFragment {
        tone: Some(tone.as_str()),
        message,
        rule,
        detail: report.detail,
        i18n,
    })
}

/// What a test found, in the words of the form: green for what the
/// monitoring counts as an answer, red for a failure.
fn verdict(report: &Report, i18n: &I18n) -> (Tone, String) {
    let ms = report.elapsed.as_millis().to_string();
    let failure = |key: &str| (Tone::Crit, i18n.t(key).to_string());
    match report.finding {
        Finding::Answered {
            status: Some(status),
        } => (
            Tone::Ok,
            i18n.tf(
                "services.check_result_web_ok",
                &[("status", &status.to_string()), ("ms", &ms)],
            ),
        ),
        Finding::Answered { status: None } => (
            Tone::Ok,
            i18n.tf("services.check_result_port_ok", &[("ms", &ms)]),
        ),
        Finding::ServerError(status) => (
            Tone::Crit,
            i18n.tf(
                "services.check_result_server_error",
                &[("status", &status.to_string())],
            ),
        ),
        Finding::Refused => failure("services.check_result_refused"),
        Finding::TimedOut => failure("services.check_result_timeout"),
        Finding::NameNotFound => failure("services.check_result_name"),
        Finding::Certificate => failure("services.check_result_certificate"),
        Finding::TooManyRedirects => failure("services.check_result_redirects"),
        Finding::Unreachable => failure("services.check_result_unreachable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port_check(host: &str, port: &str) -> Result<Option<ServiceCheck>, AppError> {
        parse_check("tcp", "", host, port, false)
    }

    #[test]
    fn a_host_and_its_port_are_joined() {
        let target = |host, port| port_check(host, port).unwrap().unwrap().target;
        assert_eq!(target(" 192.168.1.1 ", " 443 "), "192.168.1.1:443");
        assert_eq!(target("fd00::1", "22"), "[fd00::1]:22");
        assert_eq!(target("[fd00::1]", "22"), "[fd00::1]:22");
    }

    #[test]
    fn a_port_outside_its_range_is_refused() {
        for port in ["", "0", "65536", "https"] {
            assert!(
                matches!(port_check("nas.local", port), Err(AppError::Validation(key)) if key == "validation.check_port_invalid"),
                "{port}"
            );
        }
        assert!(matches!(
            port_check("", "443"),
            Err(AppError::Validation(key)) if key == "validation.check_address_required"
        ));
        assert!(matches!(
            port_check("https://nas.local", "443"),
            Err(AppError::Validation(key)) if key == "validation.check_address_invalid"
        ));
    }

    #[test]
    fn a_saved_port_target_is_split_back() {
        let fields = CheckFields::from_saved(Some(CheckKind::Tcp), Some("[fd00::1]:22"), false);
        assert_eq!(
            (fields.host.as_str(), fields.port.as_str()),
            ("fd00::1", "22")
        );
        let fields = CheckFields::from_saved(Some(CheckKind::Tcp), Some("nas.local:5000"), false);
        assert_eq!(
            (fields.host.as_str(), fields.port.as_str()),
            ("nas.local", "5000")
        );
        assert!(fields.is("tcp"));
    }

    #[test]
    fn no_monitoring_reads_as_none() {
        assert!(parse_check("none", "", "", "", false).unwrap().is_none());
        assert!(parse_check("", "", "", "", false).unwrap().is_none());
        assert!(parse_check("ping", "", "", "", false).is_err());
    }
}
