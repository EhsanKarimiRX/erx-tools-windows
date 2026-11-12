//! Changing the icon of a shortcut (.lnk) file (Windows only).
//!
//! A shortcut stores its own icon location (path + index). The same .ico as
//! for folders is generated, kept in the app's icon store
//! (`%LOCALAPPDATA%\ERX Tools\shortcut-icons\`, content-derived name) and the
//! shortcut is pointed at it with IShellLinkW::SetIconLocation. Nothing else
//! in the shortcut is touched. Eligibility is checked by the caller
//! (apply.rs); these functions only do the work.

use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IPersistFile, STGM_READ, STGM_READWRITE,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{Interface, PCWSTR};
use windows_sys::Win32::UI::Shell::FOLDERID_LocalAppData;

use super::apply::{self, IconState, RestoreOutcome};
use super::core::IconParams;
use super::eligibility::is_shortcut_file;
use super::platform;

/// IShellLinkW icon paths are limited to MAX_PATH (260 with the NUL).
const MAX_ICON_PATH_UNITS: usize = 259;

/// True for an existing .lnk file.
pub fn is_shortcut_path(path: &Path) -> bool {
    is_shortcut_file(path) && path.is_file()
}

/// The app's icon store for shortcut icons (created on demand).
pub fn icon_store_dir() -> Result<PathBuf, String> {
    let root = platform::known_folder(&FOLDERID_LocalAppData)
        .ok_or_else(|| "The app data folder could not be found.".to_string())?;
    Ok(root.join("ERX Tools").join("shortcut-icons"))
}

/// COM must be initialized on the calling thread. S_OK and S_FALSE both
/// need a matching CoUninitialize; RPC_E_CHANGED_MODE (thread already uses
/// another model) is usable but must not be uninitialized by us.
pub(super) struct ComGuard {
    initialized: bool,
}

impl ComGuard {
    pub(super) fn new() -> Self {
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        ComGuard {
            initialized: result.is_ok(),
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { CoUninitialize() };
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_path(path: &Path) -> Vec<u16> {
    wide(&path.as_os_str().to_string_lossy())
}

/// Maps a COM error to a friendly English message.
fn friendly(error: &windows::core::Error, context: &str) -> String {
    // E_ACCESSDENIED and ERROR_SHARING_VIOLATION as HRESULTs.
    const ACCESS_DENIED: i32 = 0x8007_0005_u32 as i32;
    const SHARING_VIOLATION: i32 = 0x8007_0020_u32 as i32;
    let code = error.code().0;
    if code == ACCESS_DENIED || code == SHARING_VIOLATION {
        "No permission to modify this shortcut (or it is in use by another program).".to_string()
    } else {
        eprintln!("folder_icon shortcut error: {error}");
        context.to_string()
    }
}

/// Loads the shortcut into a fresh IShellLinkW (read-only or read-write).
fn open_link(link_path: &Path, writable: bool) -> Result<(IShellLinkW, IPersistFile), String> {
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(|error| friendly(&error, "The shortcut could not be opened."))?;
    let file: IPersistFile = link
        .cast()
        .map_err(|error| friendly(&error, "The shortcut could not be opened."))?;
    let path = wide_path(link_path);
    let mode = if writable { STGM_READWRITE } else { STGM_READ };
    unsafe { file.Load(PCWSTR(path.as_ptr()), mode) }
        .map_err(|error| friendly(&error, "The shortcut could not be read."))?;
    Ok((link, file))
}

/// The shortcut's icon location: (path, index). An empty path = default icon.
fn current_icon(link_path: &Path) -> Result<(String, i32), String> {
    let (link, _file) = open_link(link_path, false)?;
    let mut buffer = [0_u16; 520];
    let mut index = 0_i32;
    unsafe { link.GetIconLocation(&mut buffer, &mut index) }
        .map_err(|error| friendly(&error, "The shortcut icon could not be read."))?;
    let len = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    Ok((String::from_utf16_lossy(&buffer[..len]), index))
}

/// Sets the icon location and saves the shortcut in place.
fn write_icon(link_path: &Path, icon_path: &str, index: i32) -> Result<(), String> {
    let (link, file) = open_link(link_path, true)?;
    let icon = wide(icon_path);
    unsafe { link.SetIconLocation(PCWSTR(icon.as_ptr()), index) }
        .map_err(|error| friendly(&error, "The shortcut icon could not be changed."))?;
    // A null file name with fRemember = save to the file it was loaded from.
    unsafe { file.Save(PCWSTR::null(), true) }
        .map_err(|error| friendly(&error, "The shortcut could not be saved."))?;
    Ok(())
}

/// Applies the icon to one shortcut (icon store = the real app data folder).
pub fn apply_shortcut_icon(link_path: &Path, params: &IconParams) -> Result<(), String> {
    apply_shortcut_icon_in(&icon_store_dir()?, link_path, params)
}

/// Same, with an explicit icon store folder (tests use a temp folder).
pub fn apply_shortcut_icon_in(
    store_dir: &Path,
    link_path: &Path,
    params: &IconParams,
) -> Result<(), String> {
    let _com = ComGuard::new();
    let ico_bytes = super::ico::build_ico(params)?;
    let icon_path = store_dir.join(apply::icon_file_name(&ico_bytes));
    let icon_text = icon_path.as_os_str().to_string_lossy().into_owned();
    if icon_text.encode_utf16().count() > MAX_ICON_PATH_UNITS {
        return Err("The app data folder path is too long for a shortcut icon.".into());
    }
    std::fs::create_dir_all(store_dir)
        .map_err(|_| "The icon store folder could not be created.".to_string())?;
    if !apply::is_current(&icon_path, &ico_bytes) {
        apply::replace_file(&icon_path, &ico_bytes)?;
    }
    let (previous_path, previous_index) = current_icon(link_path)?;
    if previous_path.eq_ignore_ascii_case(&icon_text) && previous_index == 0 {
        return Ok(()); // already points at exactly this icon
    }
    write_icon(link_path, &icon_text, 0)?;
    // Verify with a fresh read; undo on mismatch.
    match current_icon(link_path) {
        Ok((path, 0)) if path.eq_ignore_ascii_case(&icon_text) => Ok(()),
        _ => {
            let _ = write_icon(link_path, &previous_path, previous_index);
            Err("Verification failed: the shortcut does not point at the new icon.".into())
        }
    }
}

/// Restores the default icon of one shortcut: clears its icon location.
/// Files in the icon store are never deleted (other shortcuts may use them).
pub fn restore_shortcut_icon(link_path: &Path) -> Result<RestoreOutcome, String> {
    let _com = ComGuard::new();
    let (previous_path, previous_index) = current_icon(link_path)?;
    if previous_path.is_empty() {
        return Ok(RestoreOutcome::AlreadyDefault);
    }
    write_icon(link_path, "", 0)?;
    match current_icon(link_path) {
        Ok((path, _)) if path.is_empty() => Ok(RestoreOutcome::Restored),
        _ => {
            let _ = write_icon(link_path, &previous_path, previous_index);
            Err("Verification failed: the shortcut still has a custom icon.".into())
        }
    }
}

/// Default / Custom by ERX / Custom by something else.
pub fn inspect_shortcut(link_path: &Path) -> Result<IconState, String> {
    let _com = ComGuard::new();
    let (path, _index) = current_icon(link_path)?;
    if path.is_empty() {
        Ok(IconState::Default)
    } else if apply::is_our_icon_value(&path) {
        Ok(IconState::CustomByErx)
    } else {
        Ok(IconState::CustomByOther)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("erx-lnk-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn params(color: &str) -> IconParams {
        let settings =
            super::super::settings::IconSettings::from_parts(color, "none", "#FFFFFF", 3, 70)
                .expect("settings");
        super::super::core::build(&settings).params
    }

    /// A real shortcut with target, arguments, working directory, description.
    fn make_shortcut(dir: &Path) -> PathBuf {
        let _com = ComGuard::new();
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.expect("link");
        unsafe {
            link.SetPath(w!(r"C:\Windows\System32\notepad.exe"))
                .expect("path");
            link.SetArguments(w!("/A")).expect("arguments");
            link.SetWorkingDirectory(w!(r"C:\Windows"))
                .expect("working dir");
            link.SetDescription(w!("ERX test shortcut"))
                .expect("description");
        }
        let file: IPersistFile = link.cast().expect("persist");
        let path = dir.join("test.lnk");
        let target = wide_path(&path);
        unsafe { file.Save(PCWSTR(target.as_ptr()), true) }.expect("save");
        path
    }

    fn text(buffer: &[u16]) -> String {
        let len = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..len])
    }

    /// (target, arguments, working directory, description)
    fn fields(path: &Path) -> (String, String, String, String) {
        let (link, _file) = open_link(path, false).expect("open");
        let mut target = [0_u16; 520];
        let mut arguments = [0_u16; 520];
        let mut working = [0_u16; 520];
        let mut description = [0_u16; 520];
        unsafe {
            link.GetPath(&mut target, std::ptr::null_mut(), 0)
                .expect("target");
            link.GetArguments(&mut arguments).expect("arguments");
            link.GetWorkingDirectory(&mut working).expect("working dir");
            link.GetDescription(&mut description).expect("description");
        }
        (
            text(&target),
            text(&arguments),
            text(&working),
            text(&description),
        )
    }

    #[test]
    fn apply_points_the_shortcut_at_our_icon_and_keeps_everything_else() {
        let _com = ComGuard::new();
        let dir = temp_dir("apply");
        let store = dir.join("store");
        let lnk = make_shortcut(&dir);
        let before = fields(&lnk);
        assert_eq!(inspect_shortcut(&lnk).unwrap(), IconState::Default);

        apply_shortcut_icon_in(&store, &lnk, &params("#0078D4")).expect("apply");
        assert_eq!(inspect_shortcut(&lnk).unwrap(), IconState::CustomByErx);
        let (icon, index) = current_icon(&lnk).unwrap();
        assert_eq!(index, 0);
        assert!(Path::new(&icon).starts_with(&store));
        assert!(Path::new(&icon).is_file());
        assert_eq!(
            fields(&lnk),
            before,
            "target, arguments, working dir, description unchanged"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_design_is_not_rewritten_and_a_new_design_gets_a_new_icon_file() {
        let _com = ComGuard::new();
        let dir = temp_dir("redo");
        let store = dir.join("store");
        let lnk = make_shortcut(&dir);
        apply_shortcut_icon_in(&store, &lnk, &params("#0078D4")).expect("first");
        let (first_icon, _) = current_icon(&lnk).unwrap();
        let first_time = std::fs::metadata(&first_icon).unwrap().modified().unwrap();

        apply_shortcut_icon_in(&store, &lnk, &params("#0078D4")).expect("same design");
        assert_eq!(
            std::fs::metadata(&first_icon).unwrap().modified().unwrap(),
            first_time,
            "the icon file must not be rewritten"
        );

        apply_shortcut_icon_in(&store, &lnk, &params("#E81123")).expect("new design");
        let (second_icon, _) = current_icon(&lnk).unwrap();
        assert_ne!(first_icon.to_lowercase(), second_icon.to_lowercase());
        assert!(Path::new(&second_icon).is_file());
        assert!(
            Path::new(&first_icon).is_file(),
            "other shortcuts may still use the old icon"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_clears_the_icon_and_keeps_the_target() {
        let _com = ComGuard::new();
        let dir = temp_dir("restore");
        let store = dir.join("store");
        let lnk = make_shortcut(&dir);
        let before = fields(&lnk);
        assert_eq!(
            restore_shortcut_icon(&lnk).unwrap(),
            RestoreOutcome::AlreadyDefault
        );

        apply_shortcut_icon_in(&store, &lnk, &params("#0078D4")).expect("apply");
        assert_eq!(
            restore_shortcut_icon(&lnk).unwrap(),
            RestoreOutcome::Restored
        );
        assert_eq!(inspect_shortcut(&lnk).unwrap(), IconState::Default);
        assert_eq!(fields(&lnk), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_foreign_icon_is_reported_as_custom_by_other_and_restore_clears_it() {
        let _com = ComGuard::new();
        let dir = temp_dir("foreign");
        let lnk = make_shortcut(&dir);
        write_icon(&lnk, r"C:\Windows\System32\shell32.dll", 3).expect("foreign icon");
        assert_eq!(inspect_shortcut(&lnk).unwrap(), IconState::CustomByOther);
        assert_eq!(
            restore_shortcut_icon(&lnk).unwrap(),
            RestoreOutcome::Restored
        );
        assert_eq!(inspect_shortcut(&lnk).unwrap(), IconState::Default);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
