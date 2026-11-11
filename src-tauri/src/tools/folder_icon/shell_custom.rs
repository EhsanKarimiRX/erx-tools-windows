//! The Windows Shell's own way of setting a folder icon:
//! `SHGetSetFolderCustomSettings` (the call the folder Properties dialog
//! makes). Besides writing desktop.ini, the Shell registers the change
//! internally, which a plain file write does not. This module only wraps
//! the raw call; apply.rs decides when to use it and normalizes the result.

use std::path::Path;

use windows_sys::Win32::UI::Shell::{
    FCSM_ICONFILE, FCS_FORCEWRITE, SHFOLDERCUSTOMSETTINGS, SHGetSetFolderCustomSettings,
};

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

/// Asks the Shell to set `icon_file` (index 0) as the icon of `folder`,
/// forcing the write even if the same value is already there. Returns the
/// raw HRESULT (>= 0 means success). COM is initialized for the duration.
pub fn set_folder_icon(folder: &Path, icon_file: &Path) -> i32 {
    let _com = super::shortcut::ComGuard::new();
    let mut icon_w = wide(icon_file);
    let folder_w = wide(folder);
    let mut settings: SHFOLDERCUSTOMSETTINGS = unsafe { std::mem::zeroed() };
    settings.dwSize = std::mem::size_of::<SHFOLDERCUSTOMSETTINGS>() as u32;
    settings.dwMask = FCSM_ICONFILE;
    settings.pszIconFile = icon_w.as_mut_ptr();
    settings.cchIconFile = 0;
    settings.iIconIndex = 0;
    unsafe { SHGetSetFolderCustomSettings(&mut settings, folder_w.as_ptr(), FCS_FORCEWRITE) }
}
