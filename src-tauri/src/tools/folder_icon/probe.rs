//! Lock probes for the 4.2 diagnosis (H1: is anything still holding the
//! folder, desktop.ini or the .ico open?). Used by a cfg(test) test and by
//! the debug-only `folder_icon_debug_probe` command (never registered in
//! release builds). No handle opened here outlives its function.

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

/// One probe result line for the debug command.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub path: String,
    pub exclusive_open: String,
    pub folder_rename: String,
    /// Process names the Restart Manager reports as holding the file
    /// (empty when the API is unavailable or nothing was found).
    pub restart_manager: Vec<String>,
}

/// Full probe of one path: exclusive open, folder rename, Restart Manager.
pub fn probe_path(path: &Path) -> ProbeResult {
    let exclusive = match try_exclusive_open(path) {
        Ok(true) => "openable (nothing holds it)".to_string(),
        Ok(false) => "SHARING VIOLATION (something holds it)".to_string(),
        Err(error) => format!("probe error: {error}"),
    };
    let rename = if path.is_dir() {
        match try_folder_rename(path) {
            Ok(true) => "renamable (nothing holds the folder)".to_string(),
            Ok(false) => "SHARING VIOLATION (something holds the folder)".to_string(),
            Err(error) => format!("probe error: {error}"),
        }
    } else {
        String::new()
    };
    ProbeResult {
        path: path.to_string_lossy().into_owned(),
        exclusive_open: exclusive,
        folder_rename: rename,
        restart_manager: restart_manager_holders(path).unwrap_or_default(),
    }
}

/// Asks the Restart Manager which processes hold the given file
/// (RmStartSession / RmRegisterResources / RmGetList / RmEndSession,
/// verified against windows-sys 0.61 and learn.microsoft.com). Returns
/// None when the session could not be started.
fn restart_manager_holders(path: &Path) -> Option<Vec<String>> {
    use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
    use windows_sys::Win32::System::RestartManager::{
        RM_PROCESS_INFO, RmEndSession, RmGetList, RmRegisterResources, RmStartSession,
    };

    unsafe {
        let mut session: u32 = 0;
        // The session key must be a non-empty string; any unique text.
        let mut key: Vec<u16> = format!("erx-probe-{}", std::process::id())
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        if RmStartSession(&mut session, 0, key.as_mut_ptr()) != 0 {
            return None;
        }
        let mut holders = Vec::new();
        let wide: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let files: [*const u16; 1] = [wide.as_ptr()];
        let registered = RmRegisterResources(
            session,
            1,
            files.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
        );
        if registered == 0 {
            let mut needed: u32 = 0;
            // Grow until the list fits (ERROR_MORE_DATA = 234).
            loop {
                let mut infos: Vec<RM_PROCESS_INFO> = if needed > 0 {
                    vec![std::mem::zeroed(); needed as usize]
                } else {
                    Vec::new()
                };
                let mut count: u32 = infos.capacity() as u32;
                let error = RmGetList(
                    session,
                    &mut needed,
                    &mut count,
                    if infos.is_empty() {
                        std::ptr::null_mut()
                    } else {
                        infos.as_mut_ptr()
                    },
                    std::ptr::null_mut(),
                );
                if error == 0 {
                    infos.truncate(count as usize);
                    for info in &infos {
                        let name_len = info
                            .strAppName
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(info.strAppName.len());
                        holders.push(String::from_utf16_lossy(&info.strAppName[..name_len]));
                    }
                    break;
                }
                if error == ERROR_MORE_DATA && needed > 0 {
                    continue;
                }
                break;
            }
        }
        RmEndSession(session);
        Some(holders)
    }
}
