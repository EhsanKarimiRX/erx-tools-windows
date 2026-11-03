//! Applying and restoring folder icons on Windows (spec 4.4). Windows-only
//! logic behind plain functions; the Tauri command layer stays thin.
//!
//! How an icon is attached to a folder (verified against Microsoft's
//! desktop.ini documentation):
//! 1. The generated multi-size .ico is written INSIDE the target folder
//!    with a content-derived name (`erx-folder-icon-<8 hex>.ico`), so a
//!    changed icon always gets a new file name and Explorer's icon cache
//!    can never show a stale image. Marked Hidden + System.
//! 2. desktop.ini gets `IconResource=<file name>,0` with a RELATIVE path
//!    inside `[.ShellClassInfo]`, so the icon survives moving/renaming.
//!    Everything else in the file is preserved byte-exactly (see `ini.rs`).
//! 3. desktop.ini is marked Hidden + System; the folder gets the Read-only
//!    attribute (which makes the Shell process desktop.ini and does NOT
//!    make the folder's contents read-only).
//! 4. The Shell is notified per folder (UPDATEITEM on the folder +
//!    UPDATEDIR on the parent) — deliberately not the heavy global
//!    "association changed" broadcast.
//!
//! Failure safety: writes go through temp-file-then-rename inside the same
//! folder, and every step after the first mutation rolls the previous
//! state back. A desktop.ini that cannot be decoded is never overwritten.
//! Only files matching our own icon name pattern are ever deleted.

use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_SYSTEM,
    GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
};
use windows_sys::Win32::UI::Shell::{
    SHCNE_UPDATEDIR, SHCNE_UPDATEITEM, SHCNF_PATHW, SHChangeNotify,
};

use super::core::IconParams;
use super::eligibility::{self, Eligibility};
use super::ini::DesktopIni;
use super::platform;

/// Hard cap on folders per batch (spec 4.4); the batch command (4C) must
/// refuse more.
pub const MAX_BATCH: usize = 100;

/// The name prefix of every icon file this tool creates inside target
/// folders. Only files matching the full pattern are ever deleted.
pub const ICON_FILE_PREFIX: &str = "erx-folder-icon-";

/// What Restore found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// The icon keys/files were removed.
    Restored,
    /// Nothing was set — nothing to do.
    AlreadyDefault,
}

/// What Inspect reports about a folder (spec 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    Default,
    CustomByErx,
    CustomByOther,
}

/// FNV-1a 32-bit: a tiny deterministic hash for the content-derived file
/// name (no dependency; only used for cache-busting uniqueness).
fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &byte in bytes {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// The content-derived icon file name: same content → same name (so
/// re-applying the same icon rewrites one file instead of piling up).
pub fn icon_file_name(ico_bytes: &[u8]) -> String {
    format!("{ICON_FILE_PREFIX}{:08x}.ico", fnv1a(ico_bytes))
}

/// True when a file name is one this tool created (and may delete).
pub fn is_our_icon_file_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix(ICON_FILE_PREFIX) else {
        return false;
    };
    let Some(hex) = rest.strip_suffix(".ico") else {
        return false;
    };
    hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// True when an `IconResource` value points at one of OUR icon files.
fn is_our_icon_value(value: &str) -> bool {
    let base = value.trim().rsplit(['\\', '/']).next().unwrap_or(value);
    let name = base
        .split(',')
        .next()
        .unwrap_or(base)
        .trim()
        .trim_matches('"');
    is_our_icon_file_name(name)
}

/// Applies the icon to ONE folder. Returns a friendly English error on any
/// failure, with the previous state rolled back.
pub fn apply_icon(folder: &Path, params: &IconParams) -> Result<(), String> {
    let resolved = check_eligibility(folder)?;

    let ico_bytes = super::ico::build_ico(params)?;
    let name = icon_file_name(&ico_bytes);
    let icon_path = resolved.join(&name);

    // 1. The folder needs the Read-only attribute so the Shell processes
    //    desktop.ini. Preserve all other attributes; remember whether we
    //    added it so a later failure can undo exactly that.
    let attrs =
        get_attributes(&resolved).ok_or_else(|| "The folder could not be accessed.".to_string())?;
    let folder_had_readonly = attrs & FILE_ATTRIBUTE_READONLY != 0;
    if !folder_had_readonly {
        set_attributes(&resolved, attrs | FILE_ATTRIBUTE_READONLY)?;
    }

    // 2. Write the icon file (temp + rename, Hidden + System).
    let icon_existed_before = get_attributes(&icon_path).is_some();
    if let Err(error) = write_hidden_system(&icon_path, &ico_bytes) {
        rollback_folder_attribute(&resolved, attrs, folder_had_readonly);
        return Err(error);
    }

    // 3. Update desktop.ini. An unreadable file is NEVER overwritten.
    let ini_path = resolved.join("desktop.ini");
    let mut ini = match std::fs::read(&ini_path) {
        Ok(bytes) => match DesktopIni::parse(&bytes) {
            Ok(ini) => ini,
            Err(error) => {
                if !icon_existed_before {
                    let _ = std::fs::remove_file(&icon_path);
                }
                rollback_folder_attribute(&resolved, attrs, folder_had_readonly);
                return Err(format!(
                    "desktop.ini could not be read, so nothing was changed. {error}"
                ));
            }
        },
        Err(_) => DesktopIni::new_file(),
    };
    ini.set_icon_resource(&name);
    if let Err(error) = write_hidden_system(&ini_path, &ini.serialize()) {
        if !icon_existed_before {
            let _ = std::fs::remove_file(&icon_path);
        }
        rollback_folder_attribute(&resolved, attrs, folder_had_readonly);
        return Err(error);
    }

    // 4. Remove previous ERX icon files (older content hashes only —
    //    never the new one, never a foreign .ico). A locked file here is
    //    not fatal: the new name differs, so the icon still updates.
    delete_our_icon_files(&resolved, Some(&name));

    // 5. Targeted Shell notification: the folder itself + the parent view.
    notify(&resolved);
    Ok(())
}

/// Restores the default icon: removes the IconResource key(s), deletes
/// only OUR icon files, deletes a desktop.ini that has no meaningful
/// content left (and then clears the Read-only attribute), keeps anything
/// else. A foreign .ico is never deleted.
pub fn restore_icon(folder: &Path) -> Result<RestoreOutcome, String> {
    let resolved = check_eligibility(folder)?;

    let ini_path = resolved.join("desktop.ini");
    let bytes = match std::fs::read(&ini_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // No desktop.ini at all: clean up any leftovers of ours and
            // clear the Read-only attribute we may have set earlier.
            delete_our_icon_files(&resolved, None);
            let _ = clear_readonly(&resolved);
            notify(&resolved);
            return Ok(RestoreOutcome::AlreadyDefault);
        }
        Err(error) => return Err(friendly_io_error(&error, "desktop.ini could not be read.")),
    };
    let mut ini = DesktopIni::parse(&bytes).map_err(|error| {
        format!("desktop.ini could not be read, so nothing was changed. {error}")
    })?;

    let had_icon_key = ini.icon_resource().is_some();
    ini.remove_icon_resource();

    // Delete our icon files regardless of who set the key: stale files of
    // ours must not survive a restore. Foreign .ico files stay.
    delete_our_icon_files(&resolved, None);

    if ini.is_meaningless() {
        clear_attributes(&ini_path);
        std::fs::remove_file(&ini_path)
            .map_err(|error| friendly_io_error(&error, "desktop.ini could not be removed."))?;
        let _ = clear_readonly(&resolved);
        notify(&resolved);
        return Ok(RestoreOutcome::Restored);
    }

    // Other customizations remain: keep desktop.ini (with its attributes).
    write_hidden_system(&ini_path, &ini.serialize())?;
    notify(&resolved);
    if had_icon_key {
        Ok(RestoreOutcome::Restored)
    } else {
        Ok(RestoreOutcome::AlreadyDefault)
    }
}

/// Inspects a folder: Default / Custom by ERX / Custom by something else.
pub fn inspect_folder(folder: &Path) -> Result<IconState, String> {
    let ini_path = folder.join("desktop.ini");
    let bytes = match std::fs::read(&ini_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(IconState::Default);
        }
        Err(error) => return Err(friendly_io_error(&error, "desktop.ini could not be read.")),
    };
    let ini = DesktopIni::parse(&bytes)
        .map_err(|error| format!("desktop.ini could not be read. {error}"))?;
    match ini.icon_resource() {
        None => Ok(IconState::Default),
        Some(value) if is_our_icon_value(value) => Ok(IconState::CustomByErx),
        Some(_) => Ok(IconState::CustomByOther),
    }
}

/// Validates a batch size against the hard cap. Consumed by the batch
/// command in 4C-2 (same pattern as error.rs).
#[allow(dead_code)]
pub fn validate_batch_len(count: usize) -> Result<(), String> {
    if count > MAX_BATCH {
        Err(format!("A batch is limited to {MAX_BATCH} folders."))
    } else {
        Ok(())
    }
}

// ----- internals -----

/// Resolves the folder and re-checks eligibility (right before touching it).
fn check_eligibility(folder: &Path) -> Result<PathBuf, String> {
    let resolved = eligibility::resolve(folder);
    let env = platform::gather_environment();
    let facts = platform::path_facts(&resolved);
    match eligibility::evaluate(&resolved, facts, &env) {
        Eligibility::Allowed | Eligibility::AllowedWithWarning(_) => Ok(resolved),
        Eligibility::Blocked(reason) => Err(format!(
            "Icon change is not available for this folder. {}",
            reason.message()
        )),
    }
}

fn rollback_folder_attribute(folder: &Path, original: u32, we_changed_it: bool) {
    if we_changed_it {
        let _ = set_attributes(folder, original);
    }
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

fn get_attributes(path: &Path) -> Option<u32> {
    let attributes = unsafe { GetFileAttributesW(wide(path).as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        None
    } else {
        Some(attributes)
    }
}

fn set_attributes(path: &Path, attributes: u32) -> Result<(), String> {
    let ok = unsafe { SetFileAttributesW(wide(path).as_ptr(), attributes) };
    if ok == 0 {
        Err("Windows refused to change the file attributes. Do you have permission to modify this folder?".into())
    } else {
        Ok(())
    }
}

/// Clears the Read-only attribute (restore cleanup).
fn clear_readonly(path: &Path) -> Result<(), String> {
    let Some(attrs) = get_attributes(path) else {
        return Ok(()); // gone already
    };
    set_attributes(path, attrs & !FILE_ATTRIBUTE_READONLY)
}

fn clear_attributes(path: &Path) {
    let _ = set_attributes(path, FILE_ATTRIBUTE_NORMAL);
}

/// Writes bytes to `path` via a temp file + rename inside the same folder,
/// then marks the file Hidden + System. Existing targets get their
/// attributes cleared first (replacing a Hidden+System file by rename
/// would otherwise be refused).
fn write_hidden_system(path: &Path, bytes: &[u8]) -> Result<(), String> {
    clear_attributes(path); // fine if the file does not exist yet
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| "The target path is not a file.".to_string())?;
    let temp = path.with_file_name(format!("{file_name}.erx-tmp"));
    std::fs::write(&temp, bytes).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        friendly_io_error(&error, "The icon file could not be written.")
    })?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(friendly_io_error(
            &error,
            "The icon file could not be put in place (is the folder open in another program?).",
        ));
    }
    set_attributes(path, FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)?;
    Ok(())
}

/// Deletes every icon file of OURS in the folder except `keep`.
/// Foreign .ico files are never touched. Returns how many were removed.
fn delete_our_icon_files(folder: &Path, keep: Option<&str>) -> usize {
    let mut deleted = 0;
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_our_icon_file_name(&name) && Some(name.as_str()) != keep {
                let path = entry.path();
                clear_attributes(&path);
                if std::fs::remove_file(&path).is_ok() {
                    deleted += 1;
                }
            }
        }
    }
    deleted
}

/// Targeted Shell notification: UPDATEITEM for the folder, UPDATEDIR for
/// the parent view. No global association-changed broadcast.
fn notify(folder: &Path) {
    unsafe {
        let folder_wide = wide(folder);
        SHChangeNotify(
            SHCNE_UPDATEITEM as i32,
            SHCNF_PATHW,
            folder_wide.as_ptr().cast(),
            ptr::null(),
        );
        if let Some(parent) = folder.parent() {
            let parent_wide = wide(parent);
            SHChangeNotify(
                SHCNE_UPDATEDIR as i32,
                SHCNF_PATHW,
                parent_wide.as_ptr().cast(),
                ptr::null(),
            );
        }
    }
}

/// Maps an io::Error to the user-facing message, with the permission case
/// (spec 4.3 item 8) spelled out.
fn friendly_io_error(error: &std::io::Error, context: &str) -> String {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => "No permission to modify this folder.".to_string(),
        _ => {
            eprintln!("folder_icon io error: {error}");
            context.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::folder_icon::colors::Rgb;
    use crate::tools::folder_icon::settings::PatternKind;

    const BLUE: IconParams = IconParams {
        base_color: Rgb {
            r: 0,
            g: 0x78,
            b: 0xd4,
        },
        back_color: Rgb {
            r: 0,
            g: 0x40,
            b: 0x80,
        },
        pattern: PatternKind::None,
        pattern_color: Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        pattern_size: 3,
        pattern_opacity: 1.0,
    };

    const RED: IconParams = IconParams {
        base_color: Rgb {
            r: 232,
            g: 17,
            b: 35,
        },
        back_color: Rgb {
            r: 0x80,
            g: 0x08,
            b: 0x10,
        },
        pattern: PatternKind::None,
        pattern_color: Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        pattern_size: 3,
        pattern_opacity: 1.0,
    };

    fn unique_temp(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("erx-fi-{}-{}-{}", tag, std::process::id(), n))
    }

    fn make_dir(tag: &str) -> PathBuf {
        let path = unique_temp(tag);
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn cleanup(path: &Path) {
        clear_attributes(path);
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                clear_attributes(&entry.path());
                if entry.path().is_dir() {
                    let _ = std::fs::remove_dir(entry.path());
                } else {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let _ = std::fs::remove_dir(path);
    }

    fn attrs_of(path: &Path) -> u32 {
        get_attributes(path).expect("attributes readable")
    }

    fn assert_hidden_system(path: &Path) {
        let attrs = attrs_of(path);
        assert_ne!(attrs & FILE_ATTRIBUTE_HIDDEN, 0, "{path:?} must be hidden");
        assert_ne!(attrs & FILE_ATTRIBUTE_SYSTEM, 0, "{path:?} must be system");
    }

    #[test]
    fn icon_names_are_content_derived_and_our_pattern_matches_tightly() {
        let a = icon_file_name(b"content-a");
        let b = icon_file_name(b"content-b");
        assert_ne!(a, b);
        assert_eq!(a.len(), ICON_FILE_PREFIX.len() + 12);
        assert!(is_our_icon_file_name(&a));
        assert!(!is_our_icon_file_name("erx-folder-icon-xyz.ico"));
        assert!(!is_our_icon_file_name("erx-folder-icon-123456789.ico"));
        assert!(!is_our_icon_file_name("other-folder-icon-12345678.ico"));
        assert!(!is_our_icon_file_name("picture.ico"));
        // Same content → same name (idempotent re-apply rewrites one file).
        assert_eq!(icon_file_name(b"content-a"), icon_file_name(b"content-a"));
    }

    #[test]
    fn our_icon_values_are_recognized() {
        assert!(is_our_icon_value("erx-folder-icon-1a2b3c4d.ico,0"));
        assert!(is_our_icon_value(
            r"C:\elsewhere\erx-folder-icon-1a2b3c4d.ico,2"
        ));
        assert!(is_our_icon_value(r#""erx-folder-icon-1a2b3c4d.ico",0"#));
        assert!(!is_our_icon_value(r"C:\custom\my icon.ico,0"));
        assert!(!is_our_icon_value("erx-folder-icon-nothex.ico,0"));
    }

    #[test]
    fn apply_creates_icon_and_ini_and_sets_attributes() {
        let folder = make_dir("apply-basic");
        apply_icon(&folder, &BLUE).expect("apply");

        let ico_name = icon_file_name(&super::super::ico::build_ico(&BLUE).unwrap());
        let icon_path = folder.join(&ico_name);
        assert!(icon_path.exists());
        assert_hidden_system(&icon_path);

        let ini_path = folder.join("desktop.ini");
        assert!(ini_path.exists());
        assert_hidden_system(&ini_path);
        let text = std::fs::read_to_string(&ini_path).unwrap();
        assert!(text.contains("[.ShellClassInfo]\r\n"));
        assert!(text.contains(&format!("IconResource={ico_name},0")));
        assert_ne!(attrs_of(&folder) & FILE_ATTRIBUTE_READONLY, 0);

        restore_icon(&folder).expect("restore");
        assert!(!ini_path.exists(), "meaningless ini is deleted");
        assert!(!icon_path.exists());
        assert_eq!(attrs_of(&folder) & FILE_ATTRIBUTE_READONLY, 0);
        cleanup(&folder);
    }

    #[test]
    fn reapply_is_idempotent_and_recolor_replaces_the_icon_file() {
        let folder = make_dir("apply-idempotent");
        apply_icon(&folder, &BLUE).expect("apply 1");
        let ini_bytes_before = std::fs::read(folder.join("desktop.ini")).unwrap();
        apply_icon(&folder, &BLUE).expect("apply 2");
        assert_eq!(
            std::fs::read(folder.join("desktop.ini")).unwrap(),
            ini_bytes_before
        );

        // A different color derives a different file; the old one is gone.
        apply_icon(&folder, &RED).expect("apply 3");
        let red_name = icon_file_name(&super::super::ico::build_ico(&RED).unwrap());
        let blue_name = icon_file_name(&super::super::ico::build_ico(&BLUE).unwrap());
        assert!(folder.join(&red_name).exists());
        assert!(!folder.join(&blue_name).exists());
        let text = std::fs::read_to_string(folder.join("desktop.ini")).unwrap();
        assert!(text.contains(&format!("IconResource={red_name},0")));
        let our_icons = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .filter(|entry| is_our_icon_file_name(&entry.file_name().to_string_lossy()))
            .count();
        assert_eq!(our_icons, 1, "exactly one ERX icon file remains");
        restore_icon(&folder).expect("restore");
        cleanup(&folder);
    }

    #[test]
    fn foreign_content_is_preserved_and_foreign_ico_survives() {
        let folder = make_dir("apply-foreign");
        // A foreign .ico and a pre-existing desktop.ini with other content.
        std::fs::write(folder.join("my icon.ico"), b"foreign").unwrap();
        std::fs::write(
            folder.join("desktop.ini"),
            b"[.ShellClassInfo]\r\nInfoTip=My tip\r\nIconResource=C:\\custom\\my icon.ico,0\r\n",
        )
        .unwrap();

        apply_icon(&folder, &BLUE).expect("apply");
        let text = std::fs::read_to_string(folder.join("desktop.ini")).unwrap();
        assert!(text.contains("InfoTip=My tip\r\n"));
        let ico_name = icon_file_name(&super::super::ico::build_ico(&BLUE).unwrap());
        assert!(text.contains(&format!("IconResource={ico_name},0")));
        assert!(folder.join("my icon.ico").exists(), "foreign .ico stays");

        // Restore: the foreign key is removed (default icon), the foreign
        // file stays, the InfoTip stays, desktop.ini stays.
        restore_icon(&folder).expect("restore");
        let text = std::fs::read_to_string(folder.join("desktop.ini")).unwrap();
        assert!(text.contains("InfoTip=My tip\r\n"));
        assert!(!text.contains("IconResource"));
        assert!(folder.join("my icon.ico").exists(), "foreign .ico stays");
        assert!(!folder.join(&ico_name).exists(), "our .ico is gone");
        cleanup(&folder);
    }

    #[test]
    fn unreadable_desktop_ini_is_never_overwritten() {
        let folder = make_dir("apply-broken-ini");
        // BOM + odd byte count = damaged UTF-16 that we refuse to parse.
        let broken = vec![0xFF, 0xFE, 0x41];
        std::fs::write(folder.join("desktop.ini"), &broken).unwrap();

        let error = apply_icon(&folder, &BLUE).unwrap_err();
        assert!(error.contains("could not be read"), "{error}");
        assert_eq!(std::fs::read(folder.join("desktop.ini")).unwrap(), broken);
        // No icon file was left behind (rollback).
        let ours: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .filter(|entry| is_our_icon_file_name(&entry.file_name().to_string_lossy()))
            .collect();
        assert!(ours.is_empty(), "rollback removed our icon file");
        cleanup(&folder);
    }

    #[test]
    fn blocked_folders_are_refused_before_anything_is_touched() {
        // Real environment: the Windows directory is protected.
        let error = apply_icon(Path::new(r"C:\Windows"), &BLUE).unwrap_err();
        assert!(error.contains("not available"), "{error}");
    }

    #[test]
    fn inspect_reports_default_custom_by_erx_and_custom_by_other() {
        let folder = make_dir("inspect");
        assert_eq!(inspect_folder(&folder).unwrap(), IconState::Default);
        apply_icon(&folder, &BLUE).expect("apply");
        assert_eq!(inspect_folder(&folder).unwrap(), IconState::CustomByErx);
        std::fs::write(
            folder.join("desktop.ini"),
            b"[.ShellClassInfo]\r\nIconResource=C:\\custom\\mine.ico,0\r\n",
        )
        .unwrap();
        assert_eq!(inspect_folder(&folder).unwrap(), IconState::CustomByOther);
        restore_icon(&folder).expect("restore");
        assert_eq!(inspect_folder(&folder).unwrap(), IconState::Default);
        cleanup(&folder);
    }

    #[test]
    fn restore_on_a_never_touched_folder_reports_already_default() {
        let folder = make_dir("restore-fresh");
        assert_eq!(
            restore_icon(&folder).unwrap(),
            RestoreOutcome::AlreadyDefault
        );
        cleanup(&folder);
    }

    #[test]
    fn batch_cap_is_enforced_at_100() {
        assert!(validate_batch_len(0).is_ok());
        assert!(validate_batch_len(MAX_BATCH).is_ok());
        assert!(validate_batch_len(MAX_BATCH + 1).is_err());
    }

    #[test]
    fn a_batch_of_100_folders_applies_and_restores() {
        let base = make_dir("apply-100");
        let mut folders = Vec::new();
        for index in 0..MAX_BATCH {
            let folder = base.join(format!("f{index:03}"));
            std::fs::create_dir_all(&folder).unwrap();
            folders.push(folder);
        }
        assert!(validate_batch_len(folders.len()).is_ok());
        for folder in &folders {
            apply_icon(folder, &BLUE).unwrap_or_else(|error| panic!("apply {:?}: {error}", folder));
        }
        for folder in &folders {
            assert_eq!(inspect_folder(folder).unwrap(), IconState::CustomByErx);
        }
        for folder in &folders {
            restore_icon(folder).expect("restore");
        }
        for folder in &folders {
            assert_eq!(inspect_folder(folder).unwrap(), IconState::Default);
        }
        cleanup(&base);
    }
}
