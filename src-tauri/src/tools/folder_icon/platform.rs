//! Windows-side fact gathering for the eligibility rules: known-folder
//! locations via `SHGetKnownFolderPath` (never hardcoded), drive types via
//! `GetDriveTypeW`, and reparse-point detection via `GetFileAttributesW`.
//! Uses `windows-sys` with minimal features (Foundation, UI_Shell,
//! Storage_FileSystem, System_Com).

// Temporary: the gather/inspect functions are consumed by the
// `folder_icon_inspect` command landing in 4B-4 (same pattern as error.rs).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, GetDriveTypeW, GetFileAttributesW,
    INVALID_FILE_ATTRIBUTES,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_Contacts, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads,
    FOLDERID_Favorites, FOLDERID_Links, FOLDERID_LocalAppData, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_Profile, FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86,
    FOLDERID_RoamingAppData, FOLDERID_SavedGames, FOLDERID_SearchHome, FOLDERID_SkyDrive,
    FOLDERID_UserProfiles, FOLDERID_Videos, FOLDERID_Windows, SHGetKnownFolderPath,
};
use windows_sys::core::GUID;
use windows_sys::core::PWSTR;

use super::eligibility::{Environment, PathFacts};

/// `GetDriveTypeW` return value DRIVE_REMOTE (4) — documented at
/// learn.microsoft.com ("DRIVE_REMOTE: 4"); windows-sys does not export
/// the GetDriveType constants.
const DRIVE_REMOTE: u32 = 4;

/// Reads one known folder. Returns `None` when the folder is not defined
/// on this machine (e.g. OneDrive not installed) — the rules then simply
/// do not include it.
pub(super) fn known_folder(guid: &GUID) -> Option<PathBuf> {
    unsafe {
        let mut path: PWSTR = ptr::null_mut();
        // KF_FLAG_DEFAULT (0): return the current (possibly redirected)
        // location, which is exactly what the rules must compare against.
        let result = SHGetKnownFolderPath(guid, 0, ptr::null_mut(), &mut path);
        if result != 0 {
            return None;
        }
        if path.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *path.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(path, len));
        CoTaskMemFree(path.cast());
        Some(PathBuf::from(text))
    }
}

/// Gathers everything the rules compare against from the real machine.
pub fn gather_environment() -> Environment {
    super::perf::mark("env begin (known folders)"); // PERF-TRACE (temporary)
    // Blocked THEMSELVES: profile root and the known user folders, all
    // with their real (possibly redirected) locations.
    let known_guids: [&GUID; 15] = [
        &FOLDERID_Profile,
        &FOLDERID_Desktop,
        &FOLDERID_Documents,
        &FOLDERID_Downloads,
        &FOLDERID_Music,
        &FOLDERID_Pictures,
        &FOLDERID_Videos,
        &FOLDERID_Favorites,
        &FOLDERID_Links,
        &FOLDERID_SavedGames,
        &FOLDERID_SearchHome,
        &FOLDERID_Contacts,
        // AppData levels: Roaming and Local via their own IDs; the AppData
        // root itself has no known-folder ID and is derived as Roaming's
        // parent.
        &FOLDERID_RoamingAppData,
        &FOLDERID_LocalAppData,
        &FOLDERID_SkyDrive,
    ];
    let mut known_folders: Vec<PathBuf> = known_guids
        .iter()
        .filter_map(|guid| known_folder(guid))
        .collect();
    if let Some(appdata_root) = known_folder(&FOLDERID_RoamingAppData)
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
    {
        known_folders.push(appdata_root);
    }

    // Blocked with everything under them.
    let system_guids: [&GUID; 3] = [
        &FOLDERID_Windows,
        &FOLDERID_ProgramFiles,
        &FOLDERID_ProgramFilesX86,
    ];
    let system_under: Vec<PathBuf> = system_guids
        .iter()
        .filter_map(|guid| known_folder(guid))
        .collect();

    // Root-only system paths.
    let root_guids: [&GUID; 2] = [&FOLDERID_ProgramData, &FOLDERID_UserProfiles];
    let system_root_only: Vec<PathBuf> = root_guids
        .iter()
        .filter_map(|guid| known_folder(guid))
        .collect();

    // Cloud-sync roots: the OneDrive known folder plus the env-var roots
    // (OneDrive, OneDriveConsumer, OneDriveCommercial) that cover setups
    // where the known folder is not registered.
    let mut cloud_roots = Vec::new();
    if let Some(one_drive) = known_folder(&FOLDERID_SkyDrive) {
        cloud_roots.push(one_drive);
    }
    for variable in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(root) = std::env::var_os(variable) {
            let root = PathBuf::from(root);
            if !cloud_roots.iter().any(|existing| {
                existing
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&root.to_string_lossy())
            }) {
                cloud_roots.push(root);
            }
        }
    }

    // Remote (network) drives.
    super::perf::mark("env drives begin"); // PERF-TRACE (temporary)
    let mut remote_drives = Vec::new();
    for letter in b'A'..=b'Z' {
        let root = format!(r"{}:\", letter as char);
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let drive_type = unsafe { GetDriveTypeW(wide.as_ptr()) };
        if drive_type == DRIVE_REMOTE {
            remote_drives.push(format!("{}:", letter as char));
        }
    }
    super::perf::mark("env drives done"); // PERF-TRACE (temporary)

    let environment = Environment {
        known_folders,
        system_under,
        system_root_only,
        cloud_roots,
        remote_drives,
    };
    super::perf::mark("env done"); // PERF-TRACE (temporary)
    environment
}

/// Reads the filesystem facts about one path (read-only; no side effects).
pub fn path_facts(path: &Path) -> PathFacts {
    let wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return PathFacts {
            exists: false,
            is_dir: false,
            is_reparse_point: false,
        };
    }
    PathFacts {
        exists: true,
        is_dir: attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        is_reparse_point: attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::folder_icon::eligibility::{BlockReason, Eligibility, evaluate};

    #[test]
    fn environment_contains_this_machines_real_locations() {
        let env = gather_environment();
        // The profile root and the Windows directory exist on every
        // Windows machine; the known-folder API must find them.
        assert!(
            env.known_folders
                .iter()
                .any(|path| path.file_name().is_some()),
            "profile root must be among the known folders"
        );
        assert!(
            env.system_under.iter().any(|path| {
                path.to_string_lossy().eq_ignore_ascii_case("windows")
                    || path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("windows"))
            }),
            "the Windows directory must be among the system roots"
        );
    }

    #[test]
    fn real_system_locations_are_blocked_and_temp_dirs_allowed() {
        let env = gather_environment();
        // Read-only checks against real system paths (no writes anywhere).
        assert_eq!(
            evaluate(
                Path::new(r"C:\Windows\System32"),
                super::path_facts(Path::new(r"C:\Windows\System32")),
                &env
            ),
            Eligibility::Blocked(BlockReason::ProtectedSystemLocation)
        );
        // A temp dir is allowed; a file inside is not a directory.
        let temp = std::env::temp_dir().join("erx-folder-icon-eligibility-test");
        std::fs::create_dir_all(&temp).expect("temp dir");
        let resolved = super::super::eligibility::resolve(&temp);
        assert_eq!(
            evaluate(&resolved, path_facts(&resolved), &env),
            Eligibility::Allowed
        );
        let file = temp.join("note.txt");
        std::fs::write(&file, b"x").expect("temp file");
        let resolved_file = super::super::eligibility::resolve(&file);
        assert_eq!(
            evaluate(&resolved_file, path_facts(&resolved_file), &env),
            Eligibility::Blocked(BlockReason::NotADirectory)
        );
        std::fs::remove_file(&file).ok();
        std::fs::remove_dir(&temp).ok();
    }
}
