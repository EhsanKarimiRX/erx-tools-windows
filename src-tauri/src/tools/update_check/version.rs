//! Semantic-version parsing and comparison for the update check.
//!
//! Deliberately hand-written (no new crates allowed): it only needs the
//! subset of SemVer that GitHub release tags use. `+build` metadata is
//! accepted but ignored, matching SemVer precedence rules.

use std::cmp::Ordering;
use std::fmt;

/// One dot-separated pre-release identifier: numeric (`3`) or text (`beta`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreId {
    Num(u64),
    Text(String),
}

impl Ord for PreId {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (PreId::Num(a), PreId::Num(b)) => a.cmp(b),
            // SemVer: numeric identifiers always compare lower than text ones.
            (PreId::Num(_), PreId::Text(_)) => Ordering::Less,
            (PreId::Text(_), PreId::Num(_)) => Ordering::Greater,
            (PreId::Text(a), PreId::Text(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for PreId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A parsed version: `major.minor.patch` with an optional pre-release list.
/// Build metadata (`+...`) is dropped at parse time, so it never takes part
/// in comparison or display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Empty = a normal release.
    pub pre: Vec<PreId>,
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let core = self
            .major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch));
        if core != Ordering::Equal {
            return core;
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            // A version with a pre-release is lower than the same one without.
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                // Left to right; if all shared identifiers are equal, the
                // longer list is the higher version.
                for (a, b) in self.pre.iter().zip(other.pre.iter()) {
                    let ord = a.cmp(b);
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    /// Normalized form without a leading `v`: `major.minor.patch` plus
    /// `-pre.release.ids` when a pre-release is present. Build metadata was
    /// already dropped at parse time.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-")?;
            for (i, id) in self.pre.iter().enumerate() {
                if i > 0 {
                    write!(f, ".")?;
                }
                match id {
                    PreId::Num(n) => write!(f, "{n}")?,
                    PreId::Text(text) => write!(f, "{text}")?,
                }
            }
        }
        Ok(())
    }
}

fn is_all_ascii_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn parse_core_part(text: &str) -> Option<u64> {
    // Digits only: rejects the signs (`+1`, `-1`) and non-ASCII digits that
    // `u64::from_str` alone would treat differently. Leading zeros accepted.
    if !is_all_ascii_digits(text) {
        return None;
    }
    text.parse::<u64>().ok()
}

fn parse_pre_identifier(text: &str) -> Option<PreId> {
    let valid = !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !valid {
        return None;
    }
    if is_all_ascii_digits(text) {
        // A numeric identifier that does not fit in a u64 is not supported
        // (the type is fixed); reject instead of silently mis-ordering it.
        Some(PreId::Num(text.parse::<u64>().ok()?))
    } else {
        Some(PreId::Text(text.to_string()))
    }
}

/// Parses `major.minor.patch[-pre.release.ids][+build]`, optionally with one
/// leading `v`/`V` and surrounding whitespace. Returns `None` for anything
/// else (wrong part count, non-numeric parts, empty or invalid identifiers).
pub fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim();
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    // Build metadata is accepted but ignored. The first `+` starts it:
    // pre-release identifiers cannot contain `+`, so everything after the
    // first one is build info.
    let text = match text.split_once('+') {
        Some((core, _build)) => core,
        None => text,
    };
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };

    let mut parts = core.split('.');
    let (major, minor, patch) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(major), Some(minor), Some(patch), None) => {
            let major = parse_core_part(major)?;
            let minor = parse_core_part(minor)?;
            let patch = parse_core_part(patch)?;
            (major, minor, patch)
        }
        _ => return None,
    };

    let pre = match pre {
        None => Vec::new(),
        Some(ids) => {
            let mut parsed = Vec::new();
            for id in ids.split('.') {
                parsed.push(parse_pre_identifier(id)?);
            }
            parsed
        }
    };

    Some(Version {
        major,
        minor,
        patch,
        pre,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Version {
        match parse_version(text) {
            Some(version) => version,
            None => panic!("expected {text:?} to parse"),
        }
    }

    fn parse_fail(text: &str) {
        assert!(
            parse_version(text).is_none(),
            "expected {text:?} to be rejected"
        );
    }

    #[test]
    fn numeric_parts_compare_numerically() {
        assert!(parse("0.2.0") > parse("0.1.0"));
        assert!(parse("0.10.0") > parse("0.9.0"));
    }

    #[test]
    fn pre_release_is_lower_than_the_same_release() {
        assert!(parse("1.0.0") > parse("1.0.0-beta.1"));
    }

    #[test]
    fn pre_release_identifiers_compare_left_to_right() {
        assert!(parse("1.0.0-beta.2") > parse("1.0.0-beta.1"));
        // Numeric identifiers compare numerically, not as text.
        assert!(parse("1.0.0-beta.11") > parse("1.0.0-beta.2"));
        // Text identifiers compare in ASCII order.
        assert!(parse("1.0.0-alpha") < parse("1.0.0-beta"));
        // Numeric identifiers compare lower than text ones.
        assert!(parse("1.0.0-1") < parse("1.0.0-alpha"));
        // A longer identifier list wins when all shared ones are equal.
        assert!(parse("1.0.0-alpha") < parse("1.0.0-alpha.1"));
    }

    #[test]
    fn equal_versions_compare_equal() {
        assert_eq!(parse("1.2.3").cmp(&parse("1.2.3")), Ordering::Equal);
        assert_eq!(parse("1.2.3-rc.1"), parse("1.2.3-rc.1"));
    }

    #[test]
    fn leading_v_and_build_metadata_are_ignored() {
        assert_eq!(parse("v0.2.0"), parse("0.2.0"));
        assert_eq!(parse("0.2.0+build5"), parse("0.2.0"));
        assert_eq!(parse("v1.2.3-rc.1+b.2"), parse("1.2.3-rc.1"));
    }

    #[test]
    fn leading_zeros_are_accepted() {
        assert_eq!(parse("01.02.03"), parse("1.2.3"));
    }

    #[test]
    fn whitespace_and_display_normalization() {
        assert_eq!(parse(" v1.2.3 ").to_string(), "1.2.3");
        assert_eq!(parse("0.2.0+build5").to_string(), "0.2.0");
        assert_eq!(parse("1.2.3-beta.11.2").to_string(), "1.2.3-beta.11.2");
        assert_eq!(parse("2.0.0").to_string(), "2.0.0");
    }

    #[test]
    fn invalid_inputs_return_none() {
        for bad in [
            "",
            "v",
            "V",
            "  ",
            "1",
            "1.2",
            "1.2.3.4",
            "a.b.c",
            "banana",
            "-1.2.3",
            "1.-2.3",
            "1.2.-3",
            "1.2.3-",
            "1.2.3-a..b",
            "1.2.3-a.",
            "1.2.3-!",
            "1.2.3-a b",
            "1.2.3-99999999999999999999999",
            "+",
            "1.2.3.4.5",
        ] {
            parse_fail(bad);
        }
    }
}
