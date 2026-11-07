//! Reading GitHub's `/releases/latest` answer and deciding which URLs the
//! app may open. Only this project's own GitHub pages count as allowed.

use serde::Deserialize;

/// The page to open when no specific release URL is available or allowed.
pub const RELEASES_PAGE_URL: &str = "https://github.com/EhsanKarimiRX/erx-tools-windows/releases";

/// The exact prefix a release URL must start with to be considered allowed.
const ALLOWED_URL_PREFIX: &str = "https://github.com/EhsanKarimiRX/erx-tools-windows/";

/// Maximum length of an allowed URL (defense against absurd answers).
const MAX_ALLOWED_URL_LENGTH: usize = 300;

/// The parts of a release the update check needs.
#[derive(Debug, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag: String,
    pub url: String,
}

/// The outcome of reading GitHub's answer.
#[derive(Debug, PartialEq, Eq)]
pub enum ReleaseParse {
    Usable(ReleaseInfo),
    /// A draft or pre-release: deliberately ignored by design (DECISIONS.md),
    /// so beta builds must be published as normal releases.
    NotUsable,
    /// Not JSON, wrong types, or a missing required field.
    Invalid,
}

/// Only the fields ERX Tools needs; every other field in the answer is
/// ignored. `tag_name` and `html_url` are required; the two flags default
/// to `false` when absent.
#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Parses the body of `GET .../releases/latest`. Empty bodies, non-JSON,
/// wrong field types or missing required fields are `Invalid`; a draft or
/// pre-release answer is `NotUsable`.
pub fn parse_release_json(body: &[u8]) -> ReleaseParse {
    let release: RawRelease = match serde_json::from_slice(body) {
        Ok(release) => release,
        Err(_) => return ReleaseParse::Invalid,
    };
    if release.draft || release.prerelease {
        return ReleaseParse::NotUsable;
    }
    ReleaseParse::Usable(ReleaseInfo {
        tag: release.tag_name,
        url: release.html_url,
    })
}

/// True only for URLs that start with exactly this project's own GitHub
/// prefix, are at most 300 bytes, and contain no whitespace or control
/// characters. Everything else is false.
pub fn is_allowed_release_url(url: &str) -> bool {
    if !url.starts_with(ALLOWED_URL_PREFIX) {
        return false;
    }
    if url.len() > MAX_ALLOWED_URL_LENGTH {
        return false;
    }
    url.bytes()
        .all(|b| !b.is_ascii_whitespace() && b >= 0x20 && b != 0x7f)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A typical `/releases/latest` body, with extra unknown fields that
    /// must be ignored.
    const TYPICAL_BODY: &str = r#"{
        "url": "https://api.github.com/repos/EhsanKarimiRX/erx-tools-windows/releases/1",
        "tag_name": "v0.2.0",
        "html_url": "https://github.com/EhsanKarimiRX/erx-tools-windows/releases/tag/v0.2.0",
        "draft": false,
        "prerelease": false,
        "name": "ERX Tools 0.2.0",
        "assets": [ { "name": "setup.exe", "size": 123 } ]
    }"#;

    fn usable_tag(body: &[u8]) -> String {
        match parse_release_json(body) {
            ReleaseParse::Usable(info) => info.tag,
            other => panic!("expected Usable, got {other:?}"),
        }
    }

    #[test]
    fn typical_body_is_usable_with_unknown_fields_ignored() {
        assert_eq!(usable_tag(TYPICAL_BODY.as_bytes()), "v0.2.0");
    }

    #[test]
    fn missing_required_fields_are_invalid() {
        assert_eq!(
            parse_release_json(br#"{"html_url": "https://github.com/x"}"#),
            ReleaseParse::Invalid
        );
        // html_url is required too: a usable release without a URL is
        // malformed, not silently openable.
        assert_eq!(
            parse_release_json(br#"{"tag_name": "v1.0.0"}"#),
            ReleaseParse::Invalid
        );
    }

    #[test]
    fn wrong_field_types_are_invalid() {
        assert_eq!(
            parse_release_json(br#"{"tag_name": 5, "html_url": "https://github.com/x"}"#),
            ReleaseParse::Invalid
        );
        assert_eq!(
            parse_release_json(
                br#"{"tag_name": "v1.0.0", "html_url": "https://github.com/x", "draft": "yes"}"#
            ),
            ReleaseParse::Invalid
        );
    }

    #[test]
    fn draft_and_prerelease_are_not_usable() {
        assert_eq!(
            parse_release_json(
                br#"{"tag_name": "v1.0.0", "html_url": "https://github.com/x", "draft": true}"#
            ),
            ReleaseParse::NotUsable
        );
        assert_eq!(
            parse_release_json(
                br#"{"tag_name": "v1.0.0", "html_url": "https://github.com/x", "prerelease": true}"#
            ),
            ReleaseParse::NotUsable
        );
        // Missing flags default to false, so this is usable.
        assert_eq!(
            parse_release_json(br#"{"tag_name": "v1.0.0", "html_url": "https://github.com/x"}"#),
            ReleaseParse::Usable(ReleaseInfo {
                tag: "v1.0.0".to_string(),
                url: "https://github.com/x".to_string(),
            })
        );
    }

    #[test]
    fn empty_garbage_and_wrong_shapes_are_invalid() {
        let bodies: Vec<&[u8]> = vec![
            b"",
            b"not json",
            &[0xC3, 0x28], // invalid UTF-8
            b"[]",
            b"null",
            b"\"hello\"",
            b"{\"tag_name\": [[[1]]], \"html_url\": \"x\"}",
        ];
        for body in bodies {
            assert_eq!(
                parse_release_json(body),
                ReleaseParse::Invalid,
                "expected {body:?} to be Invalid"
            );
        }
    }

    #[test]
    fn huge_nested_json_does_not_panic() {
        let mut body = String::from(
            "{\"tag_name\": \"v1.0.0\", \"html_url\": \"https://github.com/x\", \"extra\": ",
        );
        for _ in 0..1000 {
            body.push('[');
        }
        for _ in 0..1000 {
            body.push(']');
        }
        body.push('}');
        // The deep nesting sits in an ignored extra field: parsing must not
        // panic, and the release itself still comes through.
        assert!(matches!(
            parse_release_json(body.as_bytes()),
            ReleaseParse::Usable(_)
        ));
    }

    fn allowed(url: &str) -> bool {
        assert!(is_allowed_release_url(url), "expected {url:?} allowed");
        true
    }

    fn not_allowed(url: &str) -> bool {
        assert!(!is_allowed_release_url(url), "expected {url:?} rejected");
        false
    }

    #[test]
    fn own_github_release_urls_are_allowed() {
        allowed(RELEASES_PAGE_URL);
        allowed("https://github.com/EhsanKarimiRX/erx-tools-windows/releases/tag/v0.2.0");
    }

    #[test]
    fn lookalike_and_foreign_urls_are_rejected() {
        not_allowed("http://github.com/EhsanKarimiRX/erx-tools-windows/releases");
        not_allowed("https://github.com.evil.com/EhsanKarimiRX/erx-tools-windows/releases");
        not_allowed("https://github.com/EhsanKarimiRX/erx-tools-windows-evil/releases");
        not_allowed("https://github.com/OtherUser/erx-tools-windows/releases");
        not_allowed("https://github.com/ehsankarimirx/erx-tools-windows/releases");
        not_allowed("https://github.com/EhsanKarimiRX/erx-tools-windows/releases?page=1 x");
    }

    #[test]
    fn whitespace_control_and_length_limits_are_rejected() {
        not_allowed("https://github.com/EhsanKarimiRX/erx-tools-windows/ x");
        not_allowed("https://github.com/EhsanKarimiRX/erx-tools-windows/\nx");
        not_allowed("https://github.com/EhsanKarimiRX/erx-tools-windows/\u{0}");
        // Exactly 300 bytes is still allowed.
        let pad = "a".repeat(300 - ALLOWED_URL_PREFIX.len());
        allowed(&format!("{ALLOWED_URL_PREFIX}{pad}"));
        // One byte more is not.
        let pad = "a".repeat(301 - ALLOWED_URL_PREFIX.len());
        not_allowed(&format!("{ALLOWED_URL_PREFIX}{pad}"));
        not_allowed("");
    }
}
