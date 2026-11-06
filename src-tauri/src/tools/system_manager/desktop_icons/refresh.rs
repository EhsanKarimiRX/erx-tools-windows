//! Desktop refresh for the Desktop Icons section, following the minimal
//! layer the Folder Icon work proved safe (see folder_icon/notification.rs
//! and DECISIONS.md "Icon refresh investigation"):
//!
//! Layer 1 (always): ONE `SHCNE_ASSOCCHANGED` broadcast, non-blocking.
//! Layer 2 (best effort, const-gated): post the shell view's internal
//! refresh command (WM_COMMAND 0x7103 — the command F5 triggers, not in
//! Microsoft's docs) to the DESKTOP's `SHELLDLL_DefView` under `Progman`
//! (on some builds under `WorkerW`). Posted, never sent, so a hung desktop
//! window can never block us.
//!
//! Honesty: which layer makes the desktop repaint instantly is an owner
//! observation, not an agent-claimable fact — the UI carries the F5 note.
//! If the owner reports layer 2 unnecessary, flip SEND_DESKTOP_VIEW_REFRESH.

use std::ptr;

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::Shell::{
    SHCNE_ASSOCCHANGED, SHCNF_FLUSHNOWAIT, SHCNF_IDLIST, SHChangeNotify,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClassNameW, PostMessageW, WM_COMMAND,
};

/// Best-effort second layer; set to false if the owner finds layer 1 enough.
pub const SEND_DESKTOP_VIEW_REFRESH: bool = true;

/// Notifies the shell that desktop icon settings changed. Non-blocking.
pub fn notify_desktop_change() {
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST | SHCNF_FLUSHNOWAIT,
            ptr::null(),
            ptr::null(),
        );
    }
    if SEND_DESKTOP_VIEW_REFRESH {
        refresh_desktop_views();
    }
}

/// Posts the refresh command to every desktop view window found. Returns
/// how many views were posted to (0 is normal on unusual shell setups).
pub fn refresh_desktop_views() -> usize {
    let counter = Box::into_raw(Box::new(0_usize));
    unsafe {
        EnumWindows(Some(enum_top_level_window), counter as LPARAM);
    }
    let posted = unsafe { Box::from_raw(counter) };
    *posted
}

/// The desktop hosts: Progman always; WorkerW when the desktop was
/// re-parented (wallpaper/toolbar tools can cause it). File Explorer
/// windows (CabinetWClass) are deliberately NOT included — the section
/// changes only the DESKTOP.
fn is_desktop_host_class(class: &str) -> bool {
    class.eq_ignore_ascii_case("Progman") || class.eq_ignore_ascii_case("WorkerW")
}

fn is_shell_view_class(class: &str) -> bool {
    class.eq_ignore_ascii_case("SHELLDLL_DefView")
}

/// The callback return type (windows-sys BOOL = i32).
type BoolLike = i32;

unsafe extern "system" fn enum_top_level_window(hwnd: HWND, lparam: LPARAM) -> BoolLike {
    unsafe { enum_top_level_window_impl(hwnd, lparam) };
    1 // keep enumerating
}

unsafe fn enum_top_level_window_impl(hwnd: HWND, lparam: LPARAM) {
    unsafe {
        if let Some(class) = window_class(hwnd)
            && is_desktop_host_class(&class)
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

/// The window's class name, if it can be read.
fn window_class(hwnd: HWND) -> Option<String> {
    let mut buffer = [0_u16; 64];
    let len = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..len as usize]))
}
