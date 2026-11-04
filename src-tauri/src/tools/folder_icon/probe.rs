//! Lock probes for the 4.2 diagnosis (H1: is anything still holding the
//! folder, desktop.ini or the .ico open?). Consumed only by the
//! `nothing_holds_the_files_after_apply` regression test in apply.rs.
//! No handle opened here outlives its function.

// Consumed only by tests (same pattern as error.rs's allow(dead_code));
// remove if the regression test ever goes away.
#![allow(dead_code)]

use std::path::Path;

/// Tries to open a file with share mode 0 (fully exclusive). Returns
/// `Ok(true)` when the open SUCCEEDED — meaning nothing anywhere holds
/// the file (the handle is dropped before returning) — and `Ok(false)`
/// when Windows refused with ERROR_SHARING_VIOLATION (something holds it).
pub fn try_exclusive_open(path: &Path) -> Result<bool, String> {
    use std::os::windows::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path);
    match file {
        // The handle is dropped here — nothing outlives this function.
        Ok(_file) => Ok(true),
        Err(error) if error.raw_os_error() == Some(32) => Ok(false), // ERROR_SHARING_VIOLATION
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true), // nothing to hold
        Err(error) => Err(error.to_string()),
    }
}

/// Tries to rename a folder to a sibling name and back. Renaming fails
/// with ERROR_SHARING_VIOLATION/ACCESS_DENIED while any process keeps an
/// open handle to the folder (or something inside it) without the right
/// sharing flags. Returns Ok(true) when both renames succeeded.
pub fn try_folder_rename(folder: &Path) -> Result<bool, String> {
    let probe_name = folder.with_file_name(format!(
        "{}.erx-probe-tmp",
        folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "probe".into())
    ));
    let back = match std::fs::rename(folder, &probe_name) {
        Ok(()) => true,
        Err(error) if error.raw_os_error() == Some(32) => return Ok(false),
        Err(error) if error.raw_os_error() == Some(5) => return Ok(false), // ACCESS_DENIED
        Err(error) => return Err(error.to_string()),
    };
    match std::fs::rename(&probe_name, folder) {
        Ok(()) => Ok(back),
        Err(error) => Err(format!(
            "the probe renamed the folder but could not rename it back: {error}"
        )),
    }
}
