//! The temporary WebView2 data folder of the Secure Notes window.
//!
//! A profile folder is a direct child of the system temporary folder whose
//! name is `erx-notes-web-` followed by 16 lower-case hex characters. Every
//! delete goes through the same safety check, so a mistake can never remove a
//! folder that the app did not create.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::error::NotesError;

/// The fixed prefix of a Secure Notes WebView2 data folder.
pub const PROFILE_PREFIX: &str = "erx-notes-web-";
/// Number of random bytes in a profile folder name (16 hex characters).
pub const PROFILE_RANDOM_BYTES: usize = 8;
/// Number of hex characters after the prefix.
pub const PROFILE_HEX_LEN: usize = 2 * PROFILE_RANDOM_BYTES;
/// How many names are tried before creating a folder is given up.
const CREATE_ATTEMPTS: usize = 8;

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// The result of removing stale profile folders.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaleReport {
    /// Folders that were deleted.
    pub removed: u32,
    /// Folders that matched but could not be deleted.
    pub failed: u32,
}

/// Why a profile folder was not deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteError {
    /// The path is not a profile folder this app may delete.
    Refused,
    /// The caller asked for zero delete attempts.
    NoAttempts,
    /// Every delete attempt failed.
    Failed,
}

/// Returns `bytes` random bytes as lower-case hex.
pub fn random_hex(bytes: usize) -> Result<String, NotesError> {
    let mut random = vec![0u8; bytes];
    getrandom::fill(&mut random).map_err(|_| NotesError::RandomUnavailable)?;
    let mut hex = String::with_capacity(bytes * 2);
    for byte in random {
        hex.push(HEX_DIGITS[usize::from(byte >> 4)] as char);
        hex.push(HEX_DIGITS[usize::from(byte & 0x0f)] as char);
    }
    Ok(hex)
}

/// The profile folder path for `hex` under `base`.
pub fn profile_dir_path(base: &Path, hex: &str) -> PathBuf {
    base.join(format!("{PROFILE_PREFIX}{hex}"))
}

/// True when `path` is a direct child of `base` with the fixed prefix, exactly
/// 16 lower-case hex characters after it, and is not a link or junction.
pub fn is_safe_profile_dir(path: &Path, base: &Path) -> bool {
    if path.parent() != Some(base) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(hex) = name.strip_prefix(PROFILE_PREFIX) else {
        return false;
    };
    if hex.len() != PROFILE_HEX_LEN
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return false;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => !metadata.file_type().is_symlink(),
        // A missing folder is safe; any other error means the check could not
        // be done, so the folder is refused.
        Err(error) => error.kind() == ErrorKind::NotFound,
    }
}

/// Creates a new, empty profile folder under `base`.
pub fn create_profile_dir(base: &Path) -> Result<PathBuf, NotesError> {
    for _ in 0..CREATE_ATTEMPTS {
        let path = profile_dir_path(base, &random_hex(PROFILE_RANDOM_BYTES)?);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(NotesError::Io),
        }
    }
    Err(NotesError::Internal)
}

/// Deletes a profile folder, retrying up to `attempts` times with `delay`.
/// Returns the attempt that succeeded, or 0 when the folder was already gone.
pub fn delete_profile_dir(
    path: &Path,
    base: &Path,
    attempts: u32,
    delay: Duration,
) -> Result<u32, DeleteError> {
    if !is_safe_profile_dir(path, base) {
        return Err(DeleteError::Refused);
    }
    if !path.exists() {
        return Ok(0);
    }
    if attempts == 0 {
        return Err(DeleteError::NoAttempts);
    }
    for attempt in 1..=attempts {
        match fs::remove_dir_all(path) {
            Ok(()) => return Ok(attempt),
            Err(_) if attempt < attempts => std::thread::sleep(delay),
            Err(_) => return Err(DeleteError::Failed),
        }
    }
    Err(DeleteError::Failed)
}

/// Deletes every safe profile folder under `base` that is not in `keep`.
pub fn cleanup_stale_profiles(
    base: &Path,
    keep: &[PathBuf],
    attempts: u32,
    delay: Duration,
) -> StaleReport {
    let mut report = StaleReport::default();
    let Ok(entries) = fs::read_dir(base) else {
        return report;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if keep.iter().any(|kept| kept == &path) {
            continue;
        }
        if !is_safe_profile_dir(&path, base) {
            continue;
        }
        let is_dir = fs::symlink_metadata(&path)
            .map(|metadata| metadata.file_type().is_dir())
            .unwrap_or(false);
        if !is_dir {
            continue;
        }
        match delete_profile_dir(&path, base, attempts, delay) {
            Ok(removed) if removed > 0 => report.removed += 1,
            Ok(_) => {}
            Err(_) => report.failed += 1,
        }
    }
    report
}
