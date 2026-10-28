//! Whether a newer version of Statup is published. The server asks GitHub
//! once a day, never a visitor's browser, and only administrators see the
//! answer. `UPDATE_CHECK=false` turns it off.

use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use serde::Deserialize;
use tokio::task::AbortHandle;

/// The version this binary was built as.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

const LATEST_RELEASE: &str = "https://api.github.com/repos/karl-cta/statup/releases/latest";
/// Every published version, where the Version tile leads when nothing newer
/// is known.
pub const RELEASES_PAGE: &str = "https://github.com/karl-cta/statup/releases";
const RELEASES: &str = "https://github.com/karl-cta/statup/releases/tag";
const PERIOD: Duration = Duration::from_secs(24 * 60 * 60);
/// A restart loop must not turn into a request loop.
const FIRST_DELAY: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(10);

/// A published version newer than this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewerRelease {
    pub version: String,
    /// Built here from the version, not taken from the answer.
    pub url: String,
}

/// What the last check found. Empty until a check finds a newer version,
/// and when checks are off or failing.
#[derive(Default)]
pub struct UpdateStatus {
    newer: RwLock<Option<NewerRelease>>,
}

impl UpdateStatus {
    pub fn newer(&self) -> Option<NewerRelease> {
        self.newer
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Records the latest published version, keeping it only if newer. A tag
    /// that cannot be read changes nothing.
    pub fn record_latest(&self, tag: &str) {
        if parse(version_of(tag)).is_none() {
            return;
        }
        *self.newer.write().unwrap_or_else(PoisonError::into_inner) = newer_than_current(tag);
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

/// Checks after a minute, then once a day. A failure, such as a proxy that
/// blocks GitHub, is logged quietly and changes nothing.
pub fn spawn_update_check(status: Arc<UpdateStatus>) -> AbortHandle {
    let task = tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(TIMEOUT)
            // GitHub requires one; the version stays out of it.
            .user_agent("statup")
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                tracing::warn!(error = %e, "Update check disabled: no HTTP client");
                return;
            }
        };
        tokio::time::sleep(FIRST_DELAY).await;
        let mut interval = tokio::time::interval(PERIOD);
        loop {
            interval.tick().await;
            if let Some(tag) = latest_tag(&client).await {
                status.record_latest(&tag);
            }
        }
    });
    task.abort_handle()
}

/// The tag of the latest release, or none when GitHub cannot be reached or
/// answers something else, so a failed check keeps what the last one found.
async fn latest_tag(client: &reqwest::Client) -> Option<String> {
    let answer = client
        .get(LATEST_RELEASE)
        .header("accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status);
    let body = match answer {
        Ok(response) => response.bytes().await.ok()?,
        Err(e) => {
            tracing::debug!(error = %e, "Update check failed");
            return None;
        }
    };
    serde_json::from_slice::<Release>(&body)
        .ok()
        .map(|release| release.tag_name)
}

/// The release a tag names, when it is newer than this binary.
fn newer_than_current(tag: &str) -> Option<NewerRelease> {
    let version = version_of(tag);
    (parse(version)? > parse(CURRENT_VERSION)?).then(|| NewerRelease {
        version: version.to_string(),
        url: format!("{RELEASES}/v{version}"),
    })
}

fn version_of(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// `major.minor.patch`; a pre-release or anything else is none.
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
    let parsed = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bump(version: &str, index: usize) -> String {
        let mut parts: Vec<u64> = version.split('.').map(|p| p.parse().unwrap()).collect();
        parts[index] += 1;
        for part in parts.iter_mut().skip(index + 1) {
            *part = 0;
        }
        format!("{}.{}.{}", parts[0], parts[1], parts[2])
    }

    #[test]
    fn only_a_newer_release_is_kept() {
        let patch = bump(CURRENT_VERSION, 2);
        let found = newer_than_current(&format!("v{patch}")).expect("newer");
        assert_eq!(found.version, patch);
        assert_eq!(
            found.url,
            format!("https://github.com/karl-cta/statup/releases/tag/v{patch}")
        );
        assert!(newer_than_current(&format!("v{}", bump(CURRENT_VERSION, 0))).is_some());
        assert!(newer_than_current(CURRENT_VERSION).is_none(), "the same");
        assert!(newer_than_current("v0.0.1").is_none(), "older");
    }

    #[test]
    fn unreadable_tags_and_pre_releases_are_ignored() {
        for tag in [
            "",
            "latest",
            "v9",
            "v9.0",
            "v9.0.0-rc.1",
            "v9.0.0.1",
            "v9.x.0",
        ] {
            assert!(newer_than_current(tag).is_none(), "{tag}");
        }
    }

    #[test]
    fn a_later_check_can_clear_the_notice() {
        let status = UpdateStatus::default();
        status.record_latest(&format!("v{}", bump(CURRENT_VERSION, 1)));
        assert!(status.newer().is_some());
        status.record_latest("garbage");
        assert!(
            status.newer().is_some(),
            "an unreadable answer changes nothing"
        );
        status.record_latest(CURRENT_VERSION);
        assert!(status.newer().is_none());
    }
}
