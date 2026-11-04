//! Shell notification layer for the Folder Icon tool — the minimal pattern
//! used by the working reference applications (4.2 experiment, based on
//! the C:\dev\FolderIcon vs C:\dev\OurIcon comparison).
//!
//! Per changed folder: ONE `SHCNE_UPDATEITEM` with the full Unicode path
//! (`SHCNF_PATHW | SHCNF_FLUSHNOWAIT`). Per finished batch: ONE
//! `SHCNE_ASSOCCHANGED` with `SHCNF_IDLIST | SHCNF_FLUSHNOWAIT` and null
//! item pointers. `FLUSHNOWAIT` begins delivery and returns immediately —
//! per the SHChangeNotify documentation, plain `SHCNF_FLUSH` modifies the
//! thread used to deliver the notifications and can deadlock, and the
//! working reference implementations use only the NOWAIT form.
//!
//! Deliberately NOT sent (matched to the reference pattern): SHCNE_ATTRIBUTES,
//! SHCNE_UPDATEDIR, duplicated PIDL-form notifications, and any deferred
//! second notification.
//!
//! EXPERIMENT VERDICT (2026-11-05, verified on the owner's Windows 11):
//! notifications alone do NOT repaint the folder icon (UPDATEITEM/
//! ASSOCCHANGED in any flag combination fail; F5 does not re-evaluate
//! either). A REAL RENAME EVENT does - instantly. After every Apply or
//! Restore the folder is therefore renamed to its own name with an
//! invisible zero-width space appended and back again
//! (`nudge_folder_by_rename`), sending SHCNE_RENAMEFOLDER for each hop.
//! The visible name never changes and the path ends where it started. This is an EXPERIMENT aligned with the reference
//! behavior, not a claim that any earlier notification form was a proven
//! root cause.

use std::path::Path;
use std::ptr;

use windows_sys::Win32::Storage::FileSystem::MoveFileW;
use windows_sys::Win32::UI::Shell::{
    SHCNE_ASSOCCHANGED, SHCNE_RENAMEFOLDER, SHCNE_UPDATEITEM, SHCNF_FLUSHNOWAIT, SHCNF_IDLIST,
    SHCNF_PATHW, SHChangeNotify,
};

/// Notifies the Shell that ONE folder's icon changed: a single
/// UPDATEITEM with the full Unicode path, non-blocking.
pub fn notify_folder_now(folder: &Path) {
    let wide = path_wide(folder);
    unsafe {
        SHChangeNotify(
            SHCNE_UPDATEITEM as i32,
            SHCNF_PATHW | SHCNF_FLUSHNOWAIT,
            wide.as_ptr().cast(),
            ptr::null(),
        );
    }
}

/// Sends the global association-changed broadcast once, after a finished
/// batch. Heavy — every Explorer window rebuilds its icon associations and
/// views may flicker briefly — so it is never sent per folder. Non-blocking.
pub fn notify_all_associations() {
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST | SHCNF_FLUSHNOWAIT,
            ptr::null(),
            ptr::null(),
        );
    }
}

/// Forces Windows 11 Explorer to re-read the folder's desktop.ini NOW by
/// renaming the folder to its own name with an invisible zero-width space
/// appended and back again, sending SHCNE_RENAMEFOLDER for each hop.
/// Verified by experiment: a rename event repaints the folder icon
/// instantly; UPDATEITEM/ASSOCCHANGED/F5 do not. The visible name and the
/// final path are unchanged. If the folder cannot be renamed (locked),
/// the function returns silently - the UI's F5 hint stays as fallback.
/// The (away, back) name pair for the rename nudge: the visible name
/// never changes - only an invisible zero-width space toggles.
fn nudge_names(name: &str) -> (String, String) {
    const ZWSP: char = '\u{200B}';
    if name.ends_with(ZWSP) {
        (name.trim_end_matches(ZWSP).to_string(), name.to_string())
    } else {
        (format!("{name}{ZWSP}"), name.to_string())
    }
}

pub fn nudge_folder_by_rename(folder: &Path) {
    let Some(name) = folder
        .file_name()
        .map(|file| file.to_string_lossy().into_owned())
    else {
        return;
    };
    let (away_name, back_name) = nudge_names(&name);
    let away = folder.with_file_name(&away_name);
    let back = folder.with_file_name(&back_name);

    let old_w = path_wide(folder);
    let away_w = path_wide(&away);
    let back_w = path_wide(&back);
    unsafe {
        if MoveFileW(old_w.as_ptr(), away_w.as_ptr()) == 0 {
            return; // locked or otherwise unrenamable: F5 hint is the fallback
        }
        SHChangeNotify(
            SHCNE_RENAMEFOLDER as i32,
            SHCNF_PATHW | SHCNF_FLUSHNOWAIT,
            old_w.as_ptr().cast(),
            away_w.as_ptr().cast(),
        );
        if MoveFileW(away_w.as_ptr(), back_w.as_ptr()) != 0 {
            SHChangeNotify(
                SHCNE_RENAMEFOLDER as i32,
                SHCNF_PATHW | SHCNF_FLUSHNOWAIT,
                away_w.as_ptr().cast(),
                back_w.as_ptr().cast(),
            );
        }
    }
}

/// The full Unicode path, NUL-terminated.
fn path_wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nudge_names_toggle_an_invisible_suffix() {
        let (away, back) = nudge_names("demo");
        assert_eq!(away, "demo\u{200B}");
        assert_eq!(back, "demo");
        // A folder that already carries the suffix toggles the other way.
        let (away2, back2) = nudge_names("demo\u{200B}");
        assert_eq!(away2, "demo");
        assert_eq!(back2, "demo\u{200B}");
    }

    #[cfg(windows)]
    #[test]
    fn nudge_renames_away_and_back() {
        let folder = std::env::temp_dir().join(format!(
            "erx-nudge-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&folder).expect("temp folder");
        nudge_folder_by_rename(&folder);
        // The folder must end under its ORIGINAL name (renamed away and
        // back), and no zero-width-named sibling may remain.
        assert!(folder.exists(), "folder must be back at its original name");
        let away = folder.with_file_name(format!(
            "{}\u{200B}",
            folder.file_name().unwrap().to_string_lossy()
        ));
        assert!(!away.exists(), "the away name must not remain");
        std::fs::remove_dir(&folder).ok();
    }

    #[test]
    fn notification_calls_are_safe_for_arbitrary_paths() {
        // Real notifications for a temp path — harmless, must not panic.
        let folder = std::env::temp_dir().join(format!(
            "erx-notify-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&folder).expect("temp dir");
        notify_folder_now(&folder);
        notify_all_associations();
        std::fs::remove_dir(&folder).ok();
    }
}
