//! Pure logic for the Organize modes (Prompt 5h): the `Mode` enum, size
//! buckets, date folder names, the EXIF ASCII date parser, the protected
//! runtime types, and the local-time calendar math. No file system and no
//! Windows API in the tested core; the one Windows-dependent conversion
//! (`filetime_to_local_ym`, for DST-correct LOCAL time) is isolated below.

pub use super::core::extension_of;
use super::core::{Destination, OrganizeEntry, OrganizeTarget, classify, skip_reason};

/// How the Folder tab groups files. The Desktop tab is always Format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Format,
    Date {
        source: DateSource,
        granularity: Granularity,
    },
    Size,
}

/// Which date the Date mode uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSource {
    /// The photo's own camera data (EXIF); everything without one goes to
    /// the "Date not found" folder.
    PhotoTaken,
    Created,
    Modified,
}

/// Year folders ("2023") or year-and-month folders ("2023-05") — flat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Year,
    YearMonth,
}

/// What the mode needs to know about one file (plain data, no I/O).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModeMetadata {
    pub size: u64,
    /// Local year + month (1–12) the file's date resolves to; None = the
    /// date could not be determined (→ "Date not found").
    pub date_ym: Option<(i32, u8)>,
}

pub const MB: u64 = 1_048_576;
pub const GB: u64 = 1_073_741_824;

/// The bucket folder for a file size, with the exact spec names.
pub fn size_bucket(bytes: u64) -> &'static str {
    if bytes < MB {
        "Under 1 MB"
    } else if bytes < 10 * MB {
        "1-10 MB"
    } else if bytes < 100 * MB {
        "10-100 MB"
    } else if bytes < GB {
        "100 MB-1 GB"
    } else {
        "Over 1 GB"
    }
}

/// The folder name for a date: "2023", "2023-05" (zero-padded), or
/// "Date not found" when no usable date was found. A month outside 1–12
/// counts as no usable date.
pub fn date_folder_name(date: Option<(i32, u8)>, granularity: Granularity) -> String {
    match date {
        Some((year, month)) if (1..=12).contains(&month) => match granularity {
            Granularity::Year => format!("{year}"),
            Granularity::YearMonth => format!("{year}-{month:02}"),
        },
        _ => "Date not found".to_string(),
    }
}

/// Program/runtime types that STAY IN PLACE in Date and Size modes when the
/// folder is NOT the user's Downloads folder (moving them by date or size
/// could break a program if the user picked the wrong folder).
const PROTECTED_RUNTIME_TYPES: &[&str] = &[
    "exe", "dll", "sys", "ini", "msi", "msix", "appx", "bat", "cmd", "ps1", "vbs", "com", "scr",
    "drv", "cpl", "ocx", "lnk", "url",
];

pub fn is_protected_runtime_type(extension: &str) -> bool {
    PROTECTED_RUNTIME_TYPES.contains(&extension.to_ascii_lowercase().as_str())
}

/// Maps one file to its destination for the given target and mode. The
/// safety skip rules (hidden/system, reparse, partial downloads, OneDrive
/// placeholders, …) always take precedence — exactly as in Format mode.
/// The Desktop target always uses the Format-style 3-category logic (the
/// Desktop tab never sends Date/Size).
pub fn classify_entry(
    file_name: &str,
    target: OrganizeTarget,
    mode: &Mode,
    metadata: &ModeMetadata,
) -> Destination {
    let entry = OrganizeEntry {
        name: file_name.to_string(),
        flags: super::core::EntryFlags::default(),
        is_dir: false,
    };
    if let Some(reason) = skip_reason(&entry) {
        return Destination::Skip(reason);
    }
    match (target, mode) {
        // The Desktop tab keeps its exact Format behavior.
        (OrganizeTarget::Desktop, _) => classify(file_name, target),
        (OrganizeTarget::Downloads | OrganizeTarget::Custom, Mode::Format) => {
            classify(file_name, target)
        }
        (_, Mode::Date { granularity, .. }) => {
            // Runtime/program types stay in place outside Downloads.
            if target != OrganizeTarget::Downloads
                && is_protected_runtime_type(&extension_of(file_name).unwrap_or_default())
            {
                return Destination::LeaveInPlace;
            }
            Destination::Category(date_folder_name(metadata.date_ym, *granularity))
        }
        (_, Mode::Size) => {
            if target != OrganizeTarget::Downloads
                && is_protected_runtime_type(&extension_of(file_name).unwrap_or_default())
            {
                return Destination::LeaveInPlace;
            }
            Destination::Category(size_bucket(metadata.size).to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Calendar math — pure UTC part (fully tested) + the one Windows call that
// applies the user's time zone and DST rules.
// ---------------------------------------------------------------------------

/// Days since 1970-01-01 → (year, month, day), Howard Hinnant's
/// civil-from-days algorithm. Correct across leap years and era boundaries.
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    ((y + i64::from(m <= 2)) as i32, m as u32, d as u32)
}

/// Unix seconds (UTC) → the UTC (year, month). Used by tests to cross-check
/// the Windows conversion.
pub fn unix_utc_to_utc_ym(unix_seconds: i64) -> (i32, u8) {
    let days = unix_seconds.div_euclid(86_400);
    let (year, month, _) = civil_from_days(days);
    (year, month as u8)
}

/// Windows FILETIME (100 ns ticks since 1601-01-01) from Unix seconds+nanos.
/// Pure arithmetic; tested against known values.
pub const FILETIME_EPOCH_OFFSET_SECONDS: i64 = 11_644_473_600;

pub fn unix_to_filetime(unix_seconds: i64, nanos: u32) -> i64 {
    (unix_seconds + FILETIME_EPOCH_OFFSET_SECONDS) * 10_000_000 + i64::from(nanos) / 100
}

/// A std SystemTime as a Windows FILETIME.
pub fn system_time_to_filetime(time: std::time::SystemTime) -> i64 {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => {
            let seconds = i64::try_from(duration.as_secs()).unwrap_or(0);
            unix_to_filetime(seconds, duration.subsec_nanos())
        }
        Err(_) => 0, // before 1601 is impossible for real files; 0 is a safe "unknown"
    }
}

/// Converts a Windows FILETIME to the user's LOCAL (year, month), using the
/// Windows time-zone machinery so DST and the user's settings are correct.
/// This is the ONLY Windows-dependent piece here.
#[cfg(windows)]
pub fn filetime_to_local_ym(filetime: i64) -> Option<(i32, u8)> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

    if filetime < 0 {
        return None;
    }
    let as_u64 = u64::try_from(filetime).ok()?;
    let file_time = FILETIME {
        dwLowDateTime: (as_u64 & 0xFFFF_FFFF) as u32,
        dwHighDateTime: (as_u64 >> 32) as u32,
    };
    unsafe {
        let mut utc: SYSTEMTIME = std::mem::zeroed();
        if FileTimeToSystemTime(&file_time, &mut utc) == 0 {
            return None;
        }
        let mut local: SYSTEMTIME = std::mem::zeroed();
        if SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0 {
            return None;
        }
        Some((
            i32::from(local.wYear),
            u8::try_from(local.wMonth).unwrap_or(0),
        ))
    }
}

#[cfg(not(windows))]
pub fn filetime_to_local_ym(_filetime: i64) -> Option<(i32, u8)> {
    None
}

/// Parses an EXIF ASCII date "YYYY:MM:DD HH:MM:SS" into (year, month).
/// Trims trailing NULs/spaces, demands the exact 19-character shape with
/// digit fields, a valid month 1–12, and a year between 1900 and
/// `current_year + 1` (so a far-future camera clock is rejected). The
/// all-zero date "0000:00:00 00:00:00" is rejected by the year rule.
pub fn parse_exif_datetime(text: &str, current_year: i32) -> Option<(i32, u8)> {
    let trimmed = text
        .trim_end_matches(['\u{0}', ' '])
        .trim_start_matches(' ');
    let bytes = trimmed.as_bytes();
    if bytes.len() != 19 {
        return None;
    }
    for &separator in [4usize, 7usize, 13usize, 16usize].iter() {
        if bytes[separator] != b':' {
            return None;
        }
    }
    if bytes[10] != b' ' {
        return None;
    }
    for (index, &byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 7 | 10 | 13 | 16) {
            continue; // separators, already checked
        }
        if !byte.is_ascii_digit() {
            return None;
        }
    }
    let number =
        |range: std::ops::Range<usize>| -> Option<i32> { trimmed.get(range)?.parse::<i32>().ok() };
    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    if !(1900..=current_year + 1).contains(&year) {
        return None;
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((year, month as u8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::system_manager::organize::core::OrganizeTarget;

    #[test]
    fn size_buckets_hit_every_boundary() {
        let mb = MB;
        assert_eq!(size_bucket(0), "Under 1 MB");
        assert_eq!(size_bucket(mb - 1), "Under 1 MB");
        assert_eq!(size_bucket(mb), "1-10 MB");
        assert_eq!(size_bucket(10 * mb - 1), "1-10 MB");
        assert_eq!(size_bucket(10 * mb), "10-100 MB");
        assert_eq!(size_bucket(100 * mb - 1), "10-100 MB");
        assert_eq!(size_bucket(100 * mb), "100 MB-1 GB");
        assert_eq!(size_bucket(GB - 1), "100 MB-1 GB");
        assert_eq!(size_bucket(GB), "Over 1 GB");
        assert_eq!(size_bucket(u64::MAX), "Over 1 GB");
    }

    #[test]
    fn date_folder_names_year_yearmonth_and_none() {
        assert_eq!(date_folder_name(Some((2023, 5)), Granularity::Year), "2023");
        assert_eq!(
            date_folder_name(Some((2023, 5)), Granularity::YearMonth),
            "2023-05"
        );
        assert_eq!(
            date_folder_name(Some((2023, 12)), Granularity::YearMonth),
            "2023-12"
        );
        assert_eq!(
            date_folder_name(Some((2023, 1)), Granularity::YearMonth),
            "2023-01"
        );
        assert_eq!(date_folder_name(None, Granularity::Year), "Date not found");
        assert_eq!(
            date_folder_name(None, Granularity::YearMonth),
            "Date not found"
        );
        // A nonsense month counts as no date.
        assert_eq!(
            date_folder_name(Some((2023, 0)), Granularity::Year),
            "Date not found"
        );
        assert_eq!(
            date_folder_name(Some((2023, 13)), Granularity::Year),
            "Date not found"
        );
    }

    #[test]
    fn exif_parser_accepts_valid_shapes() {
        let now = 2026;
        assert_eq!(
            parse_exif_datetime("2023:05:14 08:30:00", now),
            Some((2023, 5))
        );
        // Real EXIF strings are NUL-terminated.
        assert_eq!(
            parse_exif_datetime("2019:12:31 23:59:59\u{0}\u{0}", now),
            Some((2019, 12))
        );
        assert_eq!(
            parse_exif_datetime("1900:01:01 00:00:00", now),
            Some((1900, 1))
        );
        // current_year + 1 is accepted (clock slightly ahead).
        assert_eq!(
            parse_exif_datetime("2027:06:01 12:00:00", now),
            Some((2027, 6))
        );
    }

    #[test]
    fn exif_parser_rejects_garbage() {
        let now = 2026;
        assert_eq!(parse_exif_datetime("", now), None);
        assert_eq!(parse_exif_datetime("short", now), None);
        assert_eq!(
            parse_exif_datetime("2023:13:01 00:00:00", now),
            None,
            "month 13"
        );
        assert_eq!(
            parse_exif_datetime("2023:00:01 00:00:00", now),
            None,
            "month 0"
        );
        assert_eq!(parse_exif_datetime("0000:00:00 00:00:00", now), None);
        assert_eq!(
            parse_exif_datetime("1850:05:01 00:00:00", now),
            None,
            "year 1850"
        );
        assert_eq!(
            parse_exif_datetime("2999:05:01 00:00:00", now),
            None,
            "far future"
        );
        assert_eq!(
            parse_exif_datetime("2023-05-01 08:00:00", now),
            None,
            "wrong separators"
        );
        assert_eq!(
            parse_exif_datetime("2023:05:01  8:00:00", now),
            None,
            "bad hour field"
        );
        assert_eq!(
            parse_exif_datetime("2023:05:32 00:00:00", now),
            None,
            "day 32"
        );
        assert_eq!(
            parse_exif_datetime("2023:05:01 25:00:00", now),
            None,
            "hour 25"
        );
        // Trailing junk beyond the 19-character shape.
        assert_eq!(parse_exif_datetime("2023:05:01 00:00:00 junk", now), None);
    }

    #[test]
    fn protected_runtime_types_case_insensitive() {
        for ext in ["exe", "DLL", "Ini", "MSI", "ps1", "LNK", "url", "drv"] {
            assert!(is_protected_runtime_type(ext), "{ext}");
        }
        for ext in ["txt", "jpg", "pdf", "xyz", ""] {
            assert!(!is_protected_runtime_type(ext), "{ext}");
        }
    }

    #[test]
    fn date_mode_puts_undated_files_into_date_not_found() {
        let metadata = ModeMetadata {
            size: 100,
            date_ym: None,
        };
        assert_eq!(
            classify_entry(
                "photo.jpg",
                OrganizeTarget::Custom,
                &Mode::Date {
                    source: DateSource::PhotoTaken,
                    granularity: Granularity::Year,
                },
                &metadata
            ),
            Destination::Category("Date not found".to_string())
        );
        // A video has no EXIF shot date → Date not found in PhotoTaken mode.
        assert_eq!(
            classify_entry(
                "clip.mp4",
                OrganizeTarget::Custom,
                &Mode::Date {
                    source: DateSource::PhotoTaken,
                    granularity: Granularity::YearMonth,
                },
                &metadata
            ),
            Destination::Category("Date not found".to_string())
        );
    }

    #[test]
    fn date_mode_moves_all_non_protected_files() {
        let metadata = ModeMetadata {
            size: 5,
            date_ym: Some((2019, 3)),
        };
        for name in ["note.txt", "pic.jpg", "archive.zip", "song.mp3", "noext"] {
            assert_eq!(
                classify_entry(
                    name,
                    OrganizeTarget::Custom,
                    &Mode::Date {
                        source: DateSource::Created,
                        granularity: Granularity::Year,
                    },
                    &metadata
                ),
                Destination::Category("2019".to_string()),
                "{name}"
            );
        }
    }

    #[test]
    fn protected_runtime_types_stay_in_place_outside_downloads_only() {
        let metadata = ModeMetadata {
            size: MB,
            date_ym: Some((2020, 1)),
        };
        let date_mode = Mode::Date {
            source: DateSource::Modified,
            granularity: Granularity::Year,
        };
        for name in [
            "tool.exe",
            "lib.dll",
            "config.ini",
            "setup.msi",
            "page.url",
            "run.bat",
        ] {
            assert_eq!(
                classify_entry(name, OrganizeTarget::Custom, &date_mode, &metadata),
                Destination::LeaveInPlace,
                "{name} must stay in a custom folder"
            );
            assert_eq!(
                classify_entry(name, OrganizeTarget::Downloads, &date_mode, &metadata),
                Destination::Category("2020".to_string()),
                "{name} may move inside Downloads"
            );
        }
        // Same rule in Size mode.
        assert_eq!(
            classify_entry("tool.exe", OrganizeTarget::Custom, &Mode::Size, &metadata),
            Destination::LeaveInPlace
        );
        assert_eq!(
            classify_entry(
                "tool.exe",
                OrganizeTarget::Downloads,
                &Mode::Size,
                &metadata
            ),
            Destination::Category("1-10 MB".to_string())
        );
    }

    #[test]
    fn size_mode_buckets_by_real_sizes() {
        assert_eq!(
            classify_entry(
                "tiny.txt",
                OrganizeTarget::Downloads,
                &Mode::Size,
                &ModeMetadata {
                    size: 10,
                    date_ym: None
                }
            ),
            Destination::Category("Under 1 MB".to_string())
        );
        assert_eq!(
            classify_entry(
                "big.iso",
                OrganizeTarget::Custom,
                &Mode::Size,
                &ModeMetadata {
                    size: 5 * GB,
                    date_ym: None
                }
            ),
            Destination::Category("Over 1 GB".to_string())
        );
    }

    #[test]
    fn format_mode_results_are_unchanged() {
        let metadata = ModeMetadata {
            size: MB,
            date_ym: Some((2020, 1)),
        };
        assert_eq!(
            classify_entry(
                "photo.png",
                OrganizeTarget::Desktop,
                &Mode::Format,
                &metadata
            ),
            Destination::Category("More".to_string()),
            "Desktop .png still goes to More"
        );
        assert_eq!(
            classify_entry(
                "installer.exe",
                OrganizeTarget::Downloads,
                &Mode::Format,
                &metadata
            ),
            Destination::Category("Installers".to_string())
        );
        assert_eq!(
            classify_entry("prog.exe", OrganizeTarget::Custom, &Mode::Format, &metadata),
            Destination::LeaveInPlace
        );
    }

    #[test]
    fn skip_rules_still_win_in_every_mode() {
        let metadata = ModeMetadata {
            size: MB,
            date_ym: Some((2020, 1)),
        };
        for mode in [
            Mode::Format,
            Mode::Date {
                source: DateSource::Created,
                granularity: Granularity::Year,
            },
            Mode::Size,
        ] {
            assert!(matches!(
                classify_entry(
                    "still.crdownload",
                    OrganizeTarget::Downloads,
                    &mode,
                    &metadata
                ),
                Destination::Skip(_)
            ));
            assert!(matches!(
                classify_entry("~$doc.docx", OrganizeTarget::Custom, &mode, &metadata),
                Destination::Skip(_)
            ));
        }
    }

    #[test]
    fn calendar_math_boundaries() {
        // 1970-01-01
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(unix_utc_to_utc_ym(0), (1970, 1));
        // 1999-12-31 → 2000-01-01
        assert_eq!(unix_utc_to_utc_ym(946_684_799), (1999, 12));
        assert_eq!(unix_utc_to_utc_ym(946_684_800), (2000, 1));
        // Leap day 2024-02-29.
        assert_eq!(unix_utc_to_utc_ym(1_709_164_800), (2024, 2));
        // Year boundary 2023-12-31 → 2024-01-01.
        assert_eq!(unix_utc_to_utc_ym(1_704_067_199), (2023, 12));
        assert_eq!(unix_utc_to_utc_ym(1_704_067_200), (2024, 1));
        // Far-future sanity (checked against Python's datetime module):
        // 1970-01-01 + 1,000,000 days = 4707-11-29.
        assert_eq!(civil_from_days(1_000_000), (4707, 11, 29));
    }

    #[test]
    fn filetime_arithmetic_known_value() {
        // 1970-01-01 00:00:00 UTC = 11644473600_0000000 ticks.
        assert_eq!(unix_to_filetime(0, 0), 116_444_736_000_000_000);
        // One second later, half a millisecond in.
        assert_eq!(unix_to_filetime(1, 500_000), 116_444_736_010_005_000);
    }

    #[test]
    #[ignore]
    fn live_filetime_to_system_time_utc_matches_pure_math() {
        // The Windows conversion must agree with the pure UTC math on a
        // known instant (the LOCAL step is Windows' job and is trusted).
        let unix = 1_700_000_000; // 2023-11-14 22:13:20 UTC
        let filetime = unix_to_filetime(unix, 0);
        use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows_sys::Win32::System::Time::FileTimeToSystemTime;
        let as_u64 = u64::try_from(filetime).unwrap();
        let file_time = FILETIME {
            dwLowDateTime: (as_u64 & 0xFFFF_FFFF) as u32,
            dwHighDateTime: (as_u64 >> 32) as u32,
        };
        let mut system_time: SYSTEMTIME = unsafe { std::mem::zeroed() };
        let ok = unsafe { FileTimeToSystemTime(&file_time, &mut system_time) };
        assert_eq!(ok, 1);
        assert_eq!(system_time.wYear, 2023);
        assert_eq!(system_time.wMonth, 11);
        assert_eq!(system_time.wDay, 14);
        assert_eq!(system_time.wHour, 22);
    }
}
