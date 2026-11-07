//! The result of an update check: the HTTP outcome types, the decision
//! (`decide`) and the exact user-facing messages (R5: every state shows a
//! plain-English message).

use serde::Serialize;

use super::release::{RELEASES_PAGE_URL, ReleaseParse, is_allowed_release_url, parse_release_json};
use super::version::parse_version;

/// How the HTTP layer answered. Pure data, so `decide` is unit-testable
/// without any Windows calls.
#[derive(Debug, PartialEq, Eq)]
pub enum HttpOutcome {
    Response { status: u32, body: Vec<u8> },
    Error(HttpErrorKind),
}

/// The classification of an HTTP-layer failure.
#[derive(Debug, PartialEq, Eq)]
pub enum HttpErrorKind {
    CannotReach,
    TimedOut,
    Tls,
    TooLarge,
    Other,
}

/// What the frontend is shown. Serialized as an object with a camelCase
/// `kind` tag, e.g. `{"kind": "updateAvailable", "current": "0.1.0",
/// "latest": "0.2.0"}`.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateCheckResult {
    UpToDate { current: String, latest: String },
    UpdateAvailable { current: String, latest: String },
    NoRelease { current: String },
    Failed { message: String },
}

/// Decides what the UI should show, from the app's current version and the
/// HTTP outcome. Returns the result plus the page URL to remember (set only
/// for `UpdateAvailable`).
pub fn decide(current: &str, outcome: &HttpOutcome) -> (UpdateCheckResult, Option<String>) {
    match outcome {
        HttpOutcome::Error(kind) => (
            UpdateCheckResult::Failed {
                message: error_message(kind),
            },
            None,
        ),
        HttpOutcome::Response { status, body } => decide_response(current, *status, body),
    }
}

fn error_message(kind: &HttpErrorKind) -> String {
    match kind {
        HttpErrorKind::CannotReach | HttpErrorKind::TimedOut => {
            "Couldn't reach GitHub. Check your internet connection and try again.".to_string()
        }
        HttpErrorKind::Tls => {
            "Couldn't make a secure connection to GitHub. Check your PC's date and time and any security software, then try again.".to_string()
        }
        HttpErrorKind::TooLarge | HttpErrorKind::Other => {
            "GitHub's answer could not be understood.".to_string()
        }
    }
}

fn decide_response(current: &str, status: u32, body: &[u8]) -> (UpdateCheckResult, Option<String>) {
    match status {
        404 => (
            UpdateCheckResult::NoRelease {
                current: display_version(current),
            },
            None,
        ),
        403 | 429 => (
            UpdateCheckResult::Failed {
                message:
                    "GitHub is limiting requests right now. Please try again in a few minutes."
                        .to_string(),
            },
            None,
        ),
        200 => decide_ok_body(current, body),
        other => (
            UpdateCheckResult::Failed {
                message: format!("GitHub returned an unexpected answer (HTTP {other})."),
            },
            None,
        ),
    }
}

/// Normalized display form (no leading `v`); falls back to the raw text
/// when it cannot be parsed.
fn display_version(text: &str) -> String {
    match parse_version(text) {
        Some(version) => version.to_string(),
        None => text.to_string(),
    }
}

fn decide_ok_body(current: &str, body: &[u8]) -> (UpdateCheckResult, Option<String>) {
    let release = match parse_release_json(body) {
        ReleaseParse::Usable(info) => info,
        ReleaseParse::NotUsable => {
            return (
                UpdateCheckResult::NoRelease {
                    current: display_version(current),
                },
                None,
            );
        }
        ReleaseParse::Invalid => {
            return (
                UpdateCheckResult::Failed {
                    message: "GitHub's answer could not be understood.".to_string(),
                },
                None,
            );
        }
    };

    let latest = match parse_version(&release.tag) {
        Some(version) => version,
        None => {
            return (
                UpdateCheckResult::Failed {
                    message: format!(
                        "The newest release has a version name ERX Tools cannot read ({}).",
                        release.tag
                    ),
                },
                None,
            );
        }
    };
    let current = match parse_version(current) {
        Some(version) => version,
        None => {
            return (
                UpdateCheckResult::Failed {
                    message: "This app's own version number could not be read.".to_string(),
                },
                None,
            );
        }
    };

    if latest > current {
        // Only open URLs on the project's own GitHub pages; anything else
        // falls back to the plain releases page.
        let page = if is_allowed_release_url(&release.url) {
            release.url
        } else {
            RELEASES_PAGE_URL.to_string()
        };
        (
            UpdateCheckResult::UpdateAvailable {
                current: current.to_string(),
                latest: latest.to_string(),
            },
            Some(page),
        )
    } else {
        (
            UpdateCheckResult::UpToDate {
                current: current.to_string(),
                latest: latest.to_string(),
            },
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(tag: &str, url: &str) -> Vec<u8> {
        // serde_json builds the same shape GitHub answers with.
        serde_json::json!({
            "tag_name": tag,
            "html_url": url,
            "draft": false,
            "prerelease": false,
        })
        .to_string()
        .into_bytes()
    }

    fn response(status: u32, body: Vec<u8>) -> HttpOutcome {
        HttpOutcome::Response { status, body }
    }

    fn failed_message(result: &UpdateCheckResult) -> String {
        match result {
            UpdateCheckResult::Failed { message } => message.clone(),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn connection_errors_fail_with_a_reach_message() {
        for kind in [HttpErrorKind::CannotReach, HttpErrorKind::TimedOut] {
            let (result, page) = decide("0.1.0", &HttpOutcome::Error(kind));
            assert_eq!(
                failed_message(&result),
                "Couldn't reach GitHub. Check your internet connection and try again."
            );
            assert_eq!(page, None);
        }
    }

    #[test]
    fn tls_errors_fail_with_a_tls_message() {
        let (result, page) = decide("0.1.0", &HttpOutcome::Error(HttpErrorKind::Tls));
        assert_eq!(
            failed_message(&result),
            "Couldn't make a secure connection to GitHub. Check your PC's date and time and any security software, then try again."
        );
        assert_eq!(page, None);
    }

    #[test]
    fn too_large_and_other_errors_fail_with_the_same_message() {
        for kind in [HttpErrorKind::TooLarge, HttpErrorKind::Other] {
            let (result, page) = decide("0.1.0", &HttpOutcome::Error(kind));
            assert_eq!(
                failed_message(&result),
                "GitHub's answer could not be understood."
            );
            assert_eq!(page, None);
        }
    }

    #[test]
    fn http_404_means_no_release() {
        let (result, page) = decide("0.1.0", &response(404, b"{}".to_vec()));
        assert_eq!(
            result,
            UpdateCheckResult::NoRelease {
                current: "0.1.0".to_string()
            }
        );
        assert_eq!(page, None);
    }

    #[test]
    fn rate_limiting_statuses_fail_with_a_limit_message() {
        for status in [403u32, 429] {
            let (result, page) = decide("0.1.0", &response(status, b"{}".to_vec()));
            assert_eq!(
                failed_message(&result),
                "GitHub is limiting requests right now. Please try again in a few minutes."
            );
            assert_eq!(page, None);
        }
    }

    #[test]
    fn unexpected_statuses_are_reported_with_the_number() {
        for status in [301u32, 500, 204] {
            let (result, page) = decide("0.1.0", &response(status, b"{}".to_vec()));
            assert_eq!(
                failed_message(&result),
                format!("GitHub returned an unexpected answer (HTTP {status}).")
            );
            assert_eq!(page, None);
        }
    }

    #[test]
    fn garbage_and_draft_bodies_at_200() {
        let (result, page) = decide("0.1.0", &response(200, b"garbage".to_vec()));
        assert_eq!(
            failed_message(&result),
            "GitHub's answer could not be understood."
        );
        assert_eq!(page, None);

        let draft = serde_json::json!({
            "tag_name": "v9.9.9",
            "html_url": "https://github.com/EhsanKarimiRX/erx-tools-windows/releases/tag/v9.9.9",
            "draft": true,
        })
        .to_string()
        .into_bytes();
        let (result, page) = decide("0.1.0", &response(200, draft));
        assert_eq!(
            result,
            UpdateCheckResult::NoRelease {
                current: "0.1.0".to_string()
            }
        );
        assert_eq!(page, None);
    }

    const OWN_TAG_URL: &str =
        "https://github.com/EhsanKarimiRX/erx-tools-windows/releases/tag/v0.2.0";

    #[test]
    fn newer_tag_is_an_update_and_remembers_the_page() {
        let (result, page) = decide("0.1.0", &response(200, body("v0.2.0", OWN_TAG_URL)));
        assert_eq!(
            result,
            UpdateCheckResult::UpdateAvailable {
                current: "0.1.0".to_string(),
                latest: "0.2.0".to_string(),
            }
        );
        assert_eq!(page, Some(OWN_TAG_URL.to_string()));
    }

    #[test]
    fn equal_lower_and_prerelease_tags_are_up_to_date() {
        // None of these tags count as an update over the given current
        // version; `latest` shows the release's own (normalized) version.
        let cases = [
            ("0.1.0", "0.1.0", "0.1.0"),
            ("0.1.0", "0.0.9", "0.0.9"),
            // A pre-release of the same version is never an update.
            ("1.0.0", "v1.0.0-beta.1", "1.0.0-beta.1"),
        ];
        for (current, tag, latest) in cases {
            let (result, page) = decide(current, &response(200, body(tag, OWN_TAG_URL)));
            assert_eq!(
                result,
                UpdateCheckResult::UpToDate {
                    current: current.to_string(),
                    latest: latest.to_string(),
                },
                "tag {tag} must not count as an update over {current}"
            );
            assert_eq!(page, None);
        }
    }

    #[test]
    fn unreadable_tag_and_unreadable_current_fail() {
        let (result, page) = decide("0.1.0", &response(200, body("banana", OWN_TAG_URL)));
        assert_eq!(
            failed_message(&result),
            "The newest release has a version name ERX Tools cannot read (banana)."
        );
        assert_eq!(page, None);

        let (result, page) = decide("not-a-version", &response(200, body("0.2.0", OWN_TAG_URL)));
        assert_eq!(
            failed_message(&result),
            "This app's own version number could not be read."
        );
        assert_eq!(page, None);
    }

    #[test]
    fn disallowed_release_url_falls_back_to_the_releases_page() {
        let (result, page) = decide(
            "0.1.0",
            &response(200, body("v0.2.0", "https://github.com.evil.com/x")),
        );
        assert!(matches!(result, UpdateCheckResult::UpdateAvailable { .. }));
        assert_eq!(page, Some(RELEASES_PAGE_URL.to_string()));
    }

    #[test]
    fn versions_are_shown_normalized_without_leading_v() {
        let (result, page) = decide(" v0.1.0 ", &response(200, body("v9.9.9-rc.1", OWN_TAG_URL)));
        assert_eq!(
            result,
            UpdateCheckResult::UpdateAvailable {
                current: "0.1.0".to_string(),
                latest: "9.9.9-rc.1".to_string(),
            }
        );
        assert!(page.is_some());
    }

    #[test]
    fn serialization_uses_the_camel_case_kind_tag() {
        let cases = vec![
            (
                UpdateCheckResult::UpdateAvailable {
                    current: "0.1.0".to_string(),
                    latest: "0.2.0".to_string(),
                },
                serde_json::json!({
                    "kind": "updateAvailable", "current": "0.1.0", "latest": "0.2.0"
                }),
            ),
            (
                UpdateCheckResult::UpToDate {
                    current: "0.1.0".to_string(),
                    latest: "0.1.0".to_string(),
                },
                serde_json::json!({
                    "kind": "upToDate", "current": "0.1.0", "latest": "0.1.0"
                }),
            ),
            (
                UpdateCheckResult::NoRelease {
                    current: "0.1.0".to_string(),
                },
                serde_json::json!({ "kind": "noRelease", "current": "0.1.0" }),
            ),
            (
                UpdateCheckResult::Failed {
                    message: "boom".to_string(),
                },
                serde_json::json!({ "kind": "failed", "message": "boom" }),
            ),
        ];
        for (result, expected) in cases {
            assert_eq!(serde_json::to_value(&result).expect("serialize"), expected);
        }
    }
}
