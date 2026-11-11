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
use std::time::Duration;

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

/// Delay (ms after the batch's ASSOCCHANGED) at which the shell views are
/// asked to refresh. Explorer needs a moment after the file changes before
/// a refresh takes effect, so the refresh is not sent immediately. A single
/// refresh; if icons are ever still late, add a second value (for example
/// 5000) to this array.
pub const VIEW_REFRESH_DELAYS_MS: [u64; 1] = [1000];

/// Starts the delayed view refreshes in a background thread and returns at
/// once (the batch command never waits for them).
pub fn schedule_view_refresh() {
    let _ = schedule_view_refresh_impl(&VIEW_REFRESH_DELAYS_MS, SEND_VIEW_REFRESH);
}

fn schedule_view_refresh_impl(
    delays_ms: &'static [u64],
    send: bool,
) -> Option<std::thread::JoinHandle<()>> {
    if !send {
        return None;
    }
    std::thread::Builder::new()
        .name("erx-view-refresh".into())
        .spawn(move || {
            let mut elapsed = 0_u64;
            for &delay in delays_ms {
                std::thread::sleep(Duration::from_millis(delay.saturating_sub(elapsed)));
                elapsed = delay;
                refresh_shell_views();
            }
        })
        .ok()
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

/// Shell APIs (SHChangeNotify, SHParseDisplayName) do not understand
/// extended-length paths, and `std::fs::canonicalize` returns them on
/// Windows (`\\?\C:\...`). Strip the prefix before telling the Shell
/// anything. Paths that cannot be written without it (for example
/// `\\?\Volume{...}`) are returned unchanged.
fn shell_path(path: &Path) -> String {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return rest.to_string();
        }
    }
    text.into_owned()
}

/// The Shell-friendly Unicode path, NUL-terminated.
fn path_wide(path: &Path) -> Vec<u16> {
    shell_path(path)
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

    #[test]
    fn shell_path_strips_the_extended_length_prefix() {
        assert_eq!(
            shell_path(Path::new(r"\\?\C:\Users\Me\New folder")),
            r"C:\Users\Me\New folder"
        );
        assert_eq!(
            shell_path(Path::new(r"\\?\UNC\server\share\dir")),
            r"\\server\share\dir"
        );
        assert_eq!(shell_path(Path::new(r"C:\Users\Me")), r"C:\Users\Me");
        assert_eq!(
            shell_path(Path::new(r"\\?\Volume{1234}\dir")),
            r"\\?\Volume{1234}\dir"
        );
    }

    #[test]
    fn view_refresh_schedule_returns_immediately_and_can_be_disabled() {
        assert!(schedule_view_refresh_impl(&[1, 2], false).is_none());
        let started = std::time::Instant::now();
        let handle = schedule_view_refresh_impl(&[5, 10], true).expect("thread starts");
        assert!(started.elapsed() < Duration::from_millis(200));
        handle.join().expect("thread finishes");
    }
}
