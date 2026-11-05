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

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::Shell::{
    SHCNE_ASSOCCHANGED, SHCNE_UPDATEITEM, SHCNF_FLUSHNOWAIT, SHCNF_IDLIST, SHCNF_PATHW,
    SHChangeNotify,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClassNameW, PostMessageW, WM_COMMAND,
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

/// Best effort. In a Desktop test it made earlier folders show their icons
/// at once in one case out of four; not reliable. If it ever causes
/// trouble, set to false.
pub const SEND_VIEW_REFRESH: bool = true;

/// Best-effort refresh of every open File Explorer view and the Desktop:
/// posts the shell view's internal Refresh command (WM_COMMAND 0x7103, the
/// same command F5 triggers) to each `SHELLDLL_DefView` child of the
/// Explorer windows (`CabinetWClass`) and the Desktop hosts (`Progman`,
/// `WorkerW`). Posted with `PostMessageW` only, so a hung window can never
/// block us; if a window ignores it, nothing happens. Returns how many
/// views the message was posted to. Called ONCE per finished batch, after
/// the ASSOCCHANGED broadcast.
///
/// Honesty note: 0x7103 is NOT in Microsoft's documentation - it is the
/// shell view's internal refresh command, widely used by community
/// scripts. See DECISIONS.md ("Icon refresh investigation").
pub fn refresh_shell_views() -> usize {
    refresh_shell_views_impl(SEND_VIEW_REFRESH)
}

fn refresh_shell_views_impl(send: bool) -> usize {
    if !send {
        return 0;
    }
    // The posted-count travels through the EnumWindows lparam as a raw
    // pointer to a heap usize (the callbacks are extern "system").
    let counter = Box::into_raw(Box::new(0_usize));
    unsafe {
        EnumWindows(Some(enum_top_level_window), counter as LPARAM);
    }
    let posted = unsafe { Box::from_raw(counter) };
    *posted
}

/// True for the top-level windows that host shell views.
fn is_view_host_class(class: &str) -> bool {
    class.eq_ignore_ascii_case("CabinetWClass")
        || class.eq_ignore_ascii_case("Progman")
        || class.eq_ignore_ascii_case("WorkerW")
}

/// True for the shell view windows themselves.
fn is_shell_view_class(class: &str) -> bool {
    class.eq_ignore_ascii_case("SHELLDLL_DefView")
}

unsafe extern "system" fn enum_top_level_window(hwnd: HWND, lparam: LPARAM) -> BoolLike {
    unsafe { enum_top_level_window_impl(hwnd, lparam) };
    1 // keep enumerating
}

unsafe fn enum_top_level_window_impl(hwnd: HWND, lparam: LPARAM) {
    unsafe {
        if let Some(class) = window_class(hwnd)
            && is_view_host_class(&class)
        {
            EnumChildWindows(hwnd, Some(enum_shell_view_child), lparam);
        }
    }
}

unsafe extern "system" fn enum_shell_view_child(hwnd: HWND, lparam: LPARAM) -> BoolLike {
    unsafe { enum_shell_view_child_impl(hwnd, lparam) };
    1 // keep enumerating
}

unsafe fn enum_shell_view_child_impl(hwnd: HWND, lparam: LPARAM) {
    unsafe {
        if let Some(class) = window_class(hwnd)
            && is_shell_view_class(&class)
        {
            PostMessageW(hwnd, WM_COMMAND, 0x7103, 0);
            let counter = &mut *(lparam as *mut usize);
            *counter += 1;
        }
    }
}

/// The callback return type (windows-sys BOOL = i32).
type BoolLike = i32;

/// The window's class name, if it can be read.
fn window_class(hwnd: HWND) -> Option<String> {
    let mut buffer = [0_u16; 64];
    let len = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..len as usize]))
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
    fn view_refresh_disabled_posts_nothing() {
        assert_eq!(refresh_shell_views_impl(false), 0);
    }

    #[test]
    fn view_refresh_enabled_is_safe_and_returns_a_count() {
        // Must not panic on a real desktop; the count is whatever the
        // machine has (Explorer windows + Desktop hosts) - do not assert
        // a specific value.
        let posted = refresh_shell_views_impl(true);
        assert!(posted <= 64, "implausible view count: {posted}");
    }

    #[test]
    fn view_host_classes_are_matched_case_insensitively() {
        assert!(is_view_host_class("CabinetWClass"));
        assert!(is_view_host_class("cabinetwclass"));
        assert!(is_view_host_class("Progman"));
        assert!(is_view_host_class("workerw"));
        assert!(!is_view_host_class("Notepad"));
        assert!(is_shell_view_class("SHELLDLL_DefView"));
        assert!(is_shell_view_class("shelldll_defview"));
        assert!(!is_shell_view_class("SysListView32"));
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
