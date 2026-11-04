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
//! second notification. This is an EXPERIMENT aligned with the reference
//! behavior, not a claim that any earlier notification form was a proven
//! root cause.

use std::path::Path;
use std::ptr;

use windows_sys::Win32::UI::Shell::{
    SHCNE_ASSOCCHANGED, SHCNE_UPDATEITEM, SHCNF_FLUSHNOWAIT, SHCNF_IDLIST, SHCNF_PATHW,
    SHChangeNotify,
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
