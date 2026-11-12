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
//! 4. The Shell is notified per folder with one UPDATEITEM. After a whole
//!    batch the command sends one ASSOCCHANGED and schedules delayed view
//!    refreshes (see `notification.rs` - the ONE place for all
//!    notification strategies).
//!
//! Failure safety: writes go through temp-file-then-rename inside the same
//! folder, and every step after the first mutation rolls the previous
//! state back. A desktop.ini that cannot be decoded is never overwritten.
//! Only files matching our own icon name pattern are ever deleted.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_SYSTEM,
    GetFileAttributesW, INVALID_FILE_ATTRIBUTES, REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW,
    SetFileAttributesW,
};

use super::core::IconParams;
use super::eligibility::{self, Eligibility};
use super::ini::DesktopIni;
use super::platform;

/// Dev-only timeline tracing (spec 4.2-A3): enabled with the
/// ERX_FOLDER_ICON_TRACE=1 environment variable in debug builds only.
/// Logs operation kinds and elapsed milliseconds - never folder names or
/// user data. Compiles to nothing in release builds.
#[cfg(debug_assertions)]
fn trace(step: &str) {
    if std::env::var("ERX_FOLDER_ICON_TRACE").as_deref() == Ok("1") {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let start = START.get_or_init(std::time::Instant::now);
        eprintln!(
            "[erx-folder-icon +{:>5}ms] {step}",
            start.elapsed().as_millis()
        );
    }
}
#[cfg(not(debug_assertions))]
#[inline]
fn trace(_step: &str) {}

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
pub(super) fn is_our_icon_value(value: &str) -> bool {
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
///
/// Write ORDER (spec 4.1-A1) so the Shell only ever sees final state:
/// .ico written -> desktop.ini written -> attributes on both files ->
/// the folder's Read-only attribute LAST -> all handles closed (std fs
/// closes on drop, nothing is held) -> notification. Before 4.1-A1 the
/// folder attribute was set FIRST and the notification was a weak
/// UPDATEITEM/UPDATEDIR pair without flush, PIDL form or retry.
pub fn apply_icon(folder: &Path, params: &IconParams) -> Result<(), String> {
    trace("apply: begin");
    let resolved = check_eligibility(folder)?;
    trace("apply: eligibility ok");

    // Shortcuts (.lnk) are changed through the shortcut itself, not through
    // a desktop.ini (see shortcut.rs). Same eligibility check above, same
    // notification as folders.
    if super::shortcut::is_shortcut_path(&resolved) {
        let result = super::shortcut::apply_shortcut_icon(&resolved, params);
        if result.is_ok() {
            super::notification::notify_folder_now(&resolved);
        }
        return result;
    }

    let ico_bytes = super::ico::build_ico(params)?;
    let name = icon_file_name(&ico_bytes);
    trace("apply: ico built");
    let icon_path = resolved.join(&name);
    let ini_path = resolved.join("desktop.ini");

    // Snapshot the previous desktop.ini for rollback. An unreadable file
    // is NEVER overwritten (parse here, before anything is written).
    let previous_ini: Option<Vec<u8>> = match std::fs::read(&ini_path) {
        Ok(bytes) => {
            DesktopIni::parse(&bytes).map_err(|error| {
                format!("desktop.ini could not be read, so nothing was changed. {error}")
            })?;
            Some(bytes)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(friendly_io_error(&error, "desktop.ini could not be read.")),
    };
    let mut ini = match &previous_ini {
        Some(bytes) => DesktopIni::parse(bytes).expect("parsed above"),
        None => DesktopIni::new_file(),
    };
    ini.set_icon_resource(&name);
    let ini_bytes = ini.serialize();

    // 1. Write the .ico ONLY if it is not already on disk with the exact
    //    same bytes and attributes (a same-color re-apply then rewrites
    //    nothing at all - no intermediate states, no stale-cache risk).
    let ico_current = is_current(&icon_path, &ico_bytes);
    if !ico_current {
        replace_file(&icon_path, &ico_bytes)?;
    }
    trace("apply: ico written");
    // 2. Write desktop.ini, same skip rule.
    let ini_current = is_current(&ini_path, &ini_bytes);
    let write_result = if ini_current {
        Ok(())
    } else {
        replace_file(&ini_path, &ini_bytes)
    };
    if let Err(error) = write_result {
        trace("apply: ini write FAILED");
        rollback_apply(&resolved, previous_ini);
        return Err(error);
    }
    trace("apply: ini written");
    // 3. Attributes: replace_file already gave the files their final
    //    attributes (set on the temp BEFORE the swap and merged by
    //    ReplaceFileW) - only enforce defensively if something differs.
    if !ico_current && let Err(error) = ensure_hidden_system(&icon_path) {
        trace("apply: ico attrs FAILED");
        rollback_apply(&resolved, previous_ini);
        return Err(error);
    }
    if !ini_current && let Err(error) = ensure_hidden_system(&ini_path) {
        trace("apply: ini attrs FAILED");
        rollback_apply(&resolved, previous_ini);
        return Err(error);
    }
    trace("apply: files final (desktop.ini hidden+system throughout)");
    // 4. The folder's own attribute LAST (Read-only, preserving others) —
    //    the last file-system change the Shell sees before the notify.
    let attrs =
        get_attributes(&resolved).ok_or_else(|| "The folder could not be accessed.".to_string())?;
    if let Err(error) = set_attributes(&resolved, attrs | FILE_ATTRIBUTE_READONLY) {
        trace("apply: folder attr FAILED");
        rollback_apply(&resolved, previous_ini);
        return Err(error);
    }
    trace("apply: folder attr set");

    // 4b. Shell pass: let Windows itself register the icon (the call the
    //     folder Properties dialog makes), then restore our canonical
    //     desktop.ini. Only when something was actually written above (an
    //     identical re-apply must still rewrite nothing).
    if SHELL_PASS_ENABLED && (!ico_current || !ini_current) {
        let report = shell_pass(&resolved, &name, &ini_bytes);
        trace(&format!("apply: shell pass {:?}", report.outcome));
    }

    // 5. Remove previous ERX icon files (older content hashes only —
    //    never the new one, never a foreign .ico). A locked file here is
    //    not fatal: the new name differs, so the icon still updates.
    delete_our_icon_files(&resolved, Some(&name));
    trace("apply: old icons deleted");

    // 6. Verify the final on-disk state by reading it back (spec 4.2-B4):
    //    desktop.ini parses and points at the .ico, both files are
    //    Hidden+System, the folder is Read-only. A failed verification
    //    rolls the apply back (the state is then not what we promised).
    if let Err(error) = verify_applied(&resolved, &name) {
        trace("apply: verification FAILED");
        rollback_apply(&resolved, previous_ini);
        return Err(error);
    }
    trace("apply: verified on disk");

    // 7. Notify: one UPDATEITEM per the reference pattern (4.2 experiment).
    super::notification::notify_folder_now(&resolved);
    Ok(())
}

/// Reads the final state back from disk (fresh reads, not our cache):
/// desktop.ini parses and references the .ico, both files carry
/// Hidden+System, the folder is Read-only. Returns a friendly error on
/// any mismatch.
fn verify_applied(folder: &Path, icon_name: &str) -> Result<(), String> {
    let ini_path = folder.join("desktop.ini");
    let bytes = std::fs::read(&ini_path)
        .map_err(|error| format!("Verification failed: desktop.ini could not be read. {error}"))?;
    let ini = DesktopIni::parse(&bytes).map_err(|error| format!("Verification failed: {error}"))?;
    let expected = format!("{icon_name},0");
    if ini.icon_resource() != Some(expected.as_str()) {
        return Err("Verification failed: desktop.ini does not point at the new icon.".into());
    }
    for (label, path) in [
        ("desktop.ini", ini_path),
        ("the icon file", folder.join(icon_name)),
    ] {
        match get_attributes(&path) {
            Some(attrs)
                if attrs & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)
                    == (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) => {}
            Some(_) => return Err(format!("Verification failed: {label} lost its attributes.")),
            None => return Err(format!("Verification failed: {label} is missing.")),
        }
    }
    let attrs = get_attributes(folder)
        .ok_or_else(|| "Verification failed: the folder is not accessible.".to_string())?;
    if attrs & FILE_ATTRIBUTE_READONLY == 0 {
        return Err("Verification failed: the folder is not Read-only.".into());
    }
    Ok(())
}

/// Switch for the Shell pass (set to false to go back to plain file writes).
const SHELL_PASS_ENABLED: bool = true;

/// What the Shell pass did (read by tests and the trace; apply ignores it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellPassOutcome {
    /// The Shell call failed; our own complete desktop.ini was put back.
    ApiFailed,
    /// The Shell wrote something we cannot parse; our desktop.ini was put back.
    ParseFailed,
    /// The Shell's file already equals our canonical form.
    Unchanged,
    /// The Shell's file was rewritten into our canonical form.
    Normalized,
}

/// Diagnostics of one Shell pass.
#[allow(dead_code)] // fields are read by tests only
#[derive(Debug, Clone)]
pub(super) struct ShellPassReport {
    pub(super) hresult: i32,
    pub(super) raw_after_api: Option<Vec<u8>>,
    pub(super) outcome: ShellPassOutcome,
}

/// Runs AFTER our own complete write sequence (icon file, desktop.ini,
/// attributes, Read-only folder). Asks the Windows Shell itself to register
/// the icon (SHGetSetFolderCustomSettings, forced write), then puts our
/// canonical desktop.ini form back: relative `IconResource=<name>,0`, no
/// legacy `IconFile`/`IconIndex` lines, foreign content untouched. Never
/// fails: whatever happens, the folder ends in a complete, valid state
/// (`our_ini_bytes` is written back if the Shell's result is unusable).
fn shell_pass(folder: &Path, icon_name: &str, our_ini_bytes: &[u8]) -> ShellPassReport {
    let ini_path = folder.join("desktop.ini");
    let hresult = super::shell_custom::set_folder_icon(folder, &folder.join(icon_name));
    let raw = std::fs::read(&ini_path).ok();
    let outcome = if hresult < 0 {
        if raw.as_deref() != Some(our_ini_bytes) {
            let _ = write_hidden_system(&ini_path, our_ini_bytes);
        }
        ShellPassOutcome::ApiFailed
    } else {
        match raw.as_deref().map(DesktopIni::parse) {
            Some(Ok(mut ini)) => {
                ini.set_icon_resource(icon_name);
                ini.remove_shell_legacy_icon_keys(icon_name);
                let canonical = ini.serialize();
                if raw.as_deref() == Some(canonical.as_slice()) {
                    ShellPassOutcome::Unchanged
                } else {
                    if write_hidden_system(&ini_path, &canonical).is_err() {
                        let _ = write_hidden_system(&ini_path, our_ini_bytes);
                    }
                    ShellPassOutcome::Normalized
                }
            }
            _ => {
                let _ = write_hidden_system(&ini_path, our_ini_bytes);
                ShellPassOutcome::ParseFailed
            }
        }
    };
    // The Shell may have touched the folder attributes; enforce ours.
    if let Some(attrs) = get_attributes(folder)
        && attrs & FILE_ATTRIBUTE_READONLY == 0
    {
        let _ = set_attributes(folder, attrs | FILE_ATTRIBUTE_READONLY);
    }
    ShellPassReport {
        hresult,
        raw_after_api: raw,
        outcome,
    }
}

/// Rolls an interrupted apply back: our icon files are removed (they are
/// ours) and the previous desktop.ini is put back byte-exactly (or
/// deleted if there was none). The folder attribute was set last, so on
/// any failure up to that point nothing needs reverting there.
fn rollback_apply(folder: &Path, previous_ini: Option<Vec<u8>>) {
    delete_our_icon_files(folder, None);
    let ini_path = folder.join("desktop.ini");
    match previous_ini {
        Some(bytes) => {
            let _ = write_hidden_system(&ini_path, &bytes);
        }
        None => {
            clear_attributes(&ini_path);
            let _ = std::fs::remove_file(&ini_path);
        }
    }
}

/// Restores the default icon: removes the IconResource key(s), deletes
/// only OUR icon files, deletes a desktop.ini that has no meaningful
/// content left (and then clears the Read-only attribute), keeps anything
/// else. A foreign .ico is never deleted.
pub fn restore_icon(folder: &Path) -> Result<RestoreOutcome, String> {
    let resolved = check_eligibility(folder)?;
    if super::shortcut::is_shortcut_path(&resolved) {
        let outcome = super::shortcut::restore_shortcut_icon(&resolved)?;
        super::notification::notify_folder_now(&resolved);
        return Ok(outcome);
    }

    let ini_path = resolved.join("desktop.ini");
    let bytes = match std::fs::read(&ini_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // No desktop.ini at all: clean up any leftovers of ours and
            // clear the Read-only attribute we may have set earlier.
            delete_our_icon_files(&resolved, None);
            let _ = clear_readonly(&resolved);
            super::notification::notify_folder_now(&resolved);
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
        super::notification::notify_folder_now(&resolved);
        return Ok(RestoreOutcome::Restored);
    }

    // Other customizations remain: keep desktop.ini — write first, then
    // its attributes (same order discipline as apply).
    replace_file(&ini_path, &ini.serialize())?;
    set_attributes(&ini_path, FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)?;
    super::notification::notify_folder_now(&resolved);
    if had_icon_key {
        Ok(RestoreOutcome::Restored)
    } else {
        Ok(RestoreOutcome::AlreadyDefault)
    }
}

/// Inspects a folder: Default / Custom by ERX / Custom by something else.
pub fn inspect_folder(folder: &Path) -> Result<IconState, String> {
    if super::shortcut::is_shortcut_path(folder) {
        return super::shortcut::inspect_shortcut(folder);
    }
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

/// Validates a batch size against the hard cap (used by the batch
/// commands).
pub fn validate_batch_len(count: usize) -> Result<(), String> {
    if count > MAX_BATCH {
        Err(format!("A batch is limited to {MAX_BATCH} folders."))
    } else {
        Ok(())
    }
}

/// Batch-facing wrapper: AlreadyDefault counts as success (the goal is
/// "the folder shows the default icon", which holds either way).
pub fn restore_icon_for_batch(folder: &Path) -> Result<(), String> {
    restore_icon(folder).map(|_| ())
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

/// Writes bytes to `path` with NO observable intermediate state (spec
/// 4.2-B2). Sequence: write the temp file, give the TEMP its final
/// Hidden+System attributes, then swap it in atomically:
/// - existing target: `ReplaceFileW` - verified against the official docs
///   it swaps the content in one operation, merges the replaced file's
///   attributes onto the replacement, and needs no attribute clearing of
///   the target (no window where desktop.ini is visible or non-system);
/// - new target: a plain rename (a new name never needs attribute games).
///
/// The old delete-then-create and clear-attributes-first behavior is gone.
pub(super) fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| "The target path is not a file.".to_string())?;
    let temp = path.with_file_name(format!("{file_name}.erx-tmp"));
    let write_result = std::fs::write(&temp, bytes);
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp);
        return Err(friendly_io_error(
            &error,
            "The icon file could not be written.",
        ));
    }
    // Final attributes on the TEMP, before it takes the real name.
    if let Err(error) = set_attributes(&temp, FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    if get_attributes(path).is_some() {
        let wide_target = wide(path);
        let wide_temp = wide(&temp);
        let ok = unsafe {
            ReplaceFileW(
                wide_target.as_ptr(),
                wide_temp.as_ptr(),
                std::ptr::null(), // no backup: the caller holds the rollback bytes
                REPLACEFILE_IGNORE_MERGE_ERRORS,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            let _ = std::fs::remove_file(&temp);
            return Err(
                "The file could not be replaced (is the folder open in another program?).".into(),
            );
        }
        trace("write: ReplaceFileW done (attrs intact throughout)");
    } else if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(friendly_io_error(
            &error,
            "The icon file could not be put in place (is the folder open in another program?).",
        ));
    }
    Ok(())
}

/// True when the file already holds exactly these bytes with the final
/// attributes (an identical re-apply then rewrites nothing).
pub(super) fn is_current(path: &Path, bytes: &[u8]) -> bool {
    let Some(attrs) = get_attributes(path) else {
        return false;
    };
    if attrs & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)
        != (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)
    {
        return false;
    }
    std::fs::read(path)
        .map(|existing| existing == bytes)
        .unwrap_or(false)
}

/// Sets Hidden+System unless they are already both set (defensive; the
/// normal path gives files their attributes before the swap).
fn ensure_hidden_system(path: &Path) -> Result<(), String> {
    match get_attributes(path) {
        Some(attrs)
            if attrs & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)
                == (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) =>
        {
            Ok(())
        }
        Some(attrs) => set_attributes(path, attrs | FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM),
        None => Ok(()), // gone; nothing to enforce
    }
}

/// Writes bytes and marks the file Hidden + System in one go (used by the
/// rollback path to put the previous desktop.ini back in its original
/// attribute state).
fn write_hidden_system(path: &Path, bytes: &[u8]) -> Result<(), String> {
    replace_file(path, bytes)?;
    ensure_hidden_system(path)
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

    /// Spec 4.2-A2: after a real apply NOTHING may keep the folder,
    /// desktop.ini or the .ico open (H1 check for our own process).
    #[test]
    fn nothing_holds_the_files_after_apply() {
        let folder = make_dir("probe-locks");
        apply_icon(&folder, &BLUE).expect("apply");
        let ico_name = icon_file_name(&super::super::ico::build_ico(&BLUE).unwrap());
        let icon_path = folder.join(&ico_name);

        // Exclusive open must SUCCEED (true) = nobody anywhere holds it.
        assert_eq!(
            crate::tools::folder_icon::probe::try_exclusive_open(&folder.join("desktop.ini")),
            Ok(true),
            "desktop.ini must not be held"
        );
        assert_eq!(
            crate::tools::folder_icon::probe::try_exclusive_open(&icon_path),
            Ok(true),
            "the .ico must not be held"
        );
        // The folder itself must be renamable (no directory handle kept).
        assert_eq!(
            crate::tools::folder_icon::probe::try_folder_rename(&folder),
            Ok(true),
            "the folder must not be held"
        );
        restore_icon(&folder).expect("restore");
        cleanup(&folder);
    }

    /// Spec 4.2-B5: replacing an existing Hidden+System file must keep
    /// it Hidden+System the whole way through (no observable window).
    #[test]
    fn replace_file_keeps_hidden_system_through_replacement() {
        let folder = make_dir("probe-replace");
        let target = folder.join("desktop.ini");
        std::fs::write(&target, b"[.ShellClassInfo]\r\nIconResource=old.ico,0\r\n").unwrap();
        set_attributes(&target, FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM).unwrap();
        let new_bytes = b"[.ShellClassInfo]\r\nIconResource=new.ico,0\r\n";
        replace_file(&target, new_bytes).expect("replace");
        // Content swapped...
        assert_eq!(std::fs::read(&target).unwrap(), new_bytes);
        // ...attributes intact (either merged from the old file or taken
        // from the temp - both are Hidden+System)...
        let attrs = get_attributes(&target).unwrap();
        assert_ne!(attrs & FILE_ATTRIBUTE_HIDDEN, 0);
        assert_ne!(attrs & FILE_ATTRIBUTE_SYSTEM, 0);
        // ...and no temp file is left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".erx-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
        cleanup(&folder);
    }

    /// Spec 4.2-B5: an identical re-apply must not rewrite the files at
    /// all (no intermediate states, nothing for Explorer to mis-read).
    #[test]
    fn identical_reapply_skips_the_writes() {
        let folder = make_dir("probe-skip");
        apply_icon(&folder, &BLUE).expect("first apply");
        let ini_path = folder.join("desktop.ini");
        let ico_path = folder.join(icon_file_name(
            &super::super::ico::build_ico(&BLUE).unwrap(),
        ));
        let ini_before = std::fs::read(&ini_path).unwrap();
        let ico_before = std::fs::read(&ico_path).unwrap();
        let ini_time = std::fs::metadata(&ini_path).unwrap().modified().unwrap();
        let ico_time = std::fs::metadata(&ico_path).unwrap().modified().unwrap();

        apply_icon(&folder, &BLUE).expect("second apply");

        // Byte-identical AND not even rewritten (same modified time).
        assert_eq!(std::fs::read(&ini_path).unwrap(), ini_before);
        assert_eq!(std::fs::read(&ico_path).unwrap(), ico_before);
        assert_eq!(
            std::fs::metadata(&ini_path).unwrap().modified().unwrap(),
            ini_time,
            "desktop.ini must not be rewritten"
        );
        assert_eq!(
            std::fs::metadata(&ico_path).unwrap().modified().unwrap(),
            ico_time,
            "the .ico must not be rewritten"
        );
        // Still fully verified.
        verify_applied(
            &folder,
            &icon_file_name(&super::super::ico::build_ico(&BLUE).unwrap()),
        )
        .expect("verification");
        restore_icon(&folder).expect("restore");
        cleanup(&folder);
    }

    /// Spec 4.2-B4: the verification read-back catches a broken state.
    #[test]
    fn verification_detects_wrong_content() {
        let folder = make_dir("probe-verify");
        apply_icon(&folder, &BLUE).expect("apply");
        let name = icon_file_name(&super::super::ico::build_ico(&BLUE).unwrap());
        // Point the ini at a wrong icon: verification must fail.
        std::fs::write(
            folder.join("desktop.ini"),
            b"[.ShellClassInfo]\r\nIconResource=wrong.ico,0\r\n",
        )
        .unwrap();
        assert!(verify_applied(&folder, &name).is_err());
        restore_icon(&folder).expect("restore");
        cleanup(&folder);
    }

    /// Prompt 4.3: apply and restore must never rename the folder and must
    /// never create sibling files or folders in the parent directory.
    #[test]
    fn apply_and_restore_never_change_names_or_siblings() {
        let base = make_dir("name-stability-base");
        let folder = base.join("target");
        std::fs::create_dir_all(&folder).unwrap();
        let listing = |p: &Path| {
            let mut names: Vec<String> = std::fs::read_dir(p)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        let before = listing(&base);
        assert_eq!(before, vec!["target".to_string()]);

        apply_icon(&folder, &BLUE).expect("apply");
        assert_eq!(
            listing(&base),
            before,
            "apply must not rename or add siblings"
        );

        restore_icon(&folder).expect("restore");
        assert_eq!(
            listing(&base),
            before,
            "restore must not rename or add siblings"
        );
        cleanup(&base);
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

    /// Decodes desktop.ini bytes (UTF-16 LE with BOM, otherwise UTF-8/ANSI) for assertions.
    fn ini_text(bytes: &[u8]) -> String {
        if bytes.starts_with(&[0xFF, 0xFE]) {
            let units: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        }
    }

    /// Calls the Shell pass directly on a folder prepared exactly like apply
    /// prepares it, prints what the Shell wrote, and checks the final state.
    #[test]
    fn shell_pass_reports_what_the_shell_wrote_and_leaves_a_canonical_file() {
        let folder = make_dir("shell-pass");
        let ico_bytes = super::super::ico::build_ico(&BLUE).unwrap();
        let name = icon_file_name(&ico_bytes);
        replace_file(&folder.join(&name), &ico_bytes).unwrap();
        let mut ini = DesktopIni::new_file();
        ini.set_icon_resource(&name);
        let ini_bytes = ini.serialize();
        let ini_path = folder.join("desktop.ini");
        replace_file(&ini_path, &ini_bytes).unwrap();
        let attrs = get_attributes(&folder).unwrap();
        set_attributes(&folder, attrs | FILE_ATTRIBUTE_READONLY).unwrap();

        let report = shell_pass(&folder, &name, &ini_bytes);
        println!(
            "SHELL_PASS hresult=0x{:08X} outcome={:?}",
            report.hresult, report.outcome
        );
        println!(
            "SHELL_PASS raw_after_api_text={:?}",
            report.raw_after_api.as_ref().map(|bytes| ini_text(bytes))
        );
        println!(
            "SHELL_PASS raw_after_api_first_bytes={:02X?}",
            report
                .raw_after_api
                .as_ref()
                .map(|bytes| bytes.iter().take(4).copied().collect::<Vec<u8>>())
        );
        println!(
            "SHELL_PASS attrs folder=0x{:08X} ini=0x{:08X} ico=0x{:08X}",
            get_attributes(&folder).unwrap_or(0),
            get_attributes(&ini_path).unwrap_or(0),
            get_attributes(&folder.join(&name)).unwrap_or(0)
        );
        assert!(report.hresult >= 0, "Shell call failed: 0x{:08X}", report.hresult);

        let text = ini_text(&std::fs::read(&ini_path).unwrap());
        assert!(text.contains(&format!("IconResource={name},0")), "{text:?}");
        assert!(!text.contains("IconFile"), "{text:?}");
        assert!(!text.contains("IconIndex"), "{text:?}");
        verify_applied(&folder, &name).expect("verified");
        restore_icon(&folder).expect("restore");
        assert!(!ini_path.exists(), "meaningless ini is deleted after restore");
        cleanup(&folder);
    }

    /// A pre-existing UTF-16 desktop.ini with foreign content must survive
    /// apply (including the Shell pass) and restore.
    #[test]
    fn shell_pass_keeps_foreign_utf16_content() {
        let folder = make_dir("shell-utf16");
        let mut original: Vec<u8> = vec![0xFF, 0xFE];
        for unit in "[.ShellClassInfo]\r\nInfoTip=S\u{00E9}\r\n".encode_utf16() {
            original.extend_from_slice(&unit.to_le_bytes());
        }
        let ini_path = folder.join("desktop.ini");
        std::fs::write(&ini_path, &original).unwrap();
        set_attributes(&ini_path, FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM).unwrap();

        apply_icon(&folder, &BLUE).expect("apply");
        let name = icon_file_name(&super::super::ico::build_ico(&BLUE).unwrap());
        let text = ini_text(&std::fs::read(&ini_path).unwrap());
        assert!(text.contains("InfoTip=S\u{00E9}"), "{text:?}");
        assert!(text.contains(&format!("IconResource={name},0")), "{text:?}");
        assert!(!text.contains("IconFile"), "{text:?}");
        assert_eq!(inspect_folder(&folder).unwrap(), IconState::CustomByErx);

        restore_icon(&folder).expect("restore");
        let text = ini_text(&std::fs::read(&ini_path).unwrap());
        assert!(text.contains("InfoTip=S\u{00E9}"), "{text:?}");
        assert!(!text.contains("IconResource"), "{text:?}");
        assert!(!text.contains("IconFile"), "{text:?}");
        cleanup(&folder);
    }
}
