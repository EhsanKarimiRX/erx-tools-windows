//! The organize engine: execute a scanned plan (never overwrite — move
//! with MoveFileExW flags 0 and " (2)" names), and undo the last run per
//! target from the manifest (reverse order, only EMPTY folders removed).
//! Everything here is testable in scratch directories.

use std::path::{Path, PathBuf};

use super::core::{OrganizeTarget, unique_name};
use super::manifest::{self, Manifest};
use super::modes::Mode;
use super::scan::{
    PlannedFile, SkippedFile, flags_from_attributes, mode_id, modified_seconds, raw_attributes,
};

/// The honest result of one run: how many moved and why each skipped one
/// was skipped (files left in place are already known from the scan).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub moved: u32,
    pub skipped: Vec<SkippedFile>,
}

/// Why a move could not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveError {
    /// 183 / 80: the destination exists (race) — try the next name.
    AlreadyExists,
    /// 32 / 33 / 5: locked or protected — reported as "in use or protected".
    InUseOrProtected,
    /// Anything else, with the raw code for the message.
    Other(u32),
}

/// One file move. Flags 0 on Windows: no MOVEFILE_REPLACE_EXISTING, no
/// copy — a plain rename inside the same folder tree that can NEVER
/// replace an existing file.
pub fn move_file(src: &Path, dst: &Path) -> Result<(), MoveError> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_LOCK_VIOLATION,
            ERROR_SHARING_VIOLATION,
        };
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

        fn wide(text: &Path) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            text.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }
        let code = unsafe { MoveFileExW(wide(src).as_ptr(), wide(dst).as_ptr(), 0) };
        if code != 0 {
            Ok(())
        } else {
            let error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            Err(match error {
                ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS => MoveError::AlreadyExists,
                ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION | ERROR_ACCESS_DENIED => {
                    MoveError::InUseOrProtected
                }
                other => MoveError::Other(other),
            })
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (src, dst);
        Err(MoveError::Other(5))
    }
}

fn move_error_text(error: MoveError) -> String {
    match error {
        MoveError::AlreadyExists => "the name is taken".into(),
        MoveError::InUseOrProtected => "in use or protected".into(),
        MoveError::Other(code) => format!("failed (Windows error code {code})"),
    }
}

fn skip_report(name: &str, reason: String, skipped: &mut Vec<SkippedFile>) {
    skipped.push(SkippedFile {
        name: name.to_string(),
        reason,
    });
}

/// Executes a scanned plan. The backend re-checks every planned file (it
/// must still exist with the same size and modified time); anything that
/// changed since the preview is skipped, and files that appeared after the
/// preview are never moved (only the planned list is processed). The
/// destination folder per file was decided by the scan — the run trusts it
/// so a preview always matches what happens.
pub fn run(
    target: OrganizeTarget,
    root: &Path,
    planned: &[PlannedFile],
    manifest_file: &Path,
    mode: &Mode,
    now_unix: i64,
    progress: &mut dyn FnMut(u32, u32),
) -> Result<RunReport, String> {
    let mut writer = manifest::start(manifest_file, target, root, mode_id(mode), now_unix)?;
    let total = planned.len() as u32;
    let mut moved: u32 = 0;
    let mut skipped: Vec<SkippedFile> = Vec::new();

    // Per-category folder state, decided once and cached: Ready(dir) or
    // Err(reason) for the whole category.
    let mut category_dirs: Vec<(String, Result<PathBuf, String>)> = Vec::new();

    for (index, file) in planned.iter().enumerate() {
        progress(index as u32, total);
        let src = root.join(&file.name);

        // Re-check: still the same file as at preview time?
        match std::fs::symlink_metadata(&src) {
            Err(_) => {
                skip_report(&file.name, "no longer there".into(), &mut skipped);
                continue;
            }
            Ok(metadata) => {
                let flags = flags_from_attributes(raw_attributes(&metadata));
                if metadata.is_dir()
                    || flags.reparse
                    || metadata.len() != file.size
                    || modified_seconds(&metadata) != file.modified_unix
                {
                    skip_report(&file.name, "changed since preview".into(), &mut skipped);
                    continue;
                }
            }
        }

        let category = file.category.clone();

        // The category folder: reuse an existing real folder, create one,
        // or skip the whole category on a name conflict.
        let dir = match category_dirs.iter().find(|(name, _)| *name == category) {
            Some((_, cached)) => cached.clone(),
            None => {
                let status = ensure_category_dir(root, &category, &mut writer);
                category_dirs.push((category.clone(), status.clone()));
                status
            }
        };
        let dir = match dir {
            Ok(dir) => dir,
            Err(reason) => {
                skip_report(&file.name, reason, &mut skipped);
                continue;
            }
        };

        // A free destination name — never overwrite.
        let destination = match unique_name(&file.name, &|candidate| {
            std::fs::symlink_metadata(dir.join(candidate)).is_ok()
        }) {
            Ok(name) => dir.join(name),
            Err(error) => {
                skip_report(&file.name, error, &mut skipped);
                continue;
            }
        };

        match move_file(&src, &destination) {
            Ok(()) => {
                // Record BEFORE moving on: a crash must never lose track.
                writer.record_move(&src, &destination)?;
                moved += 1;
            }
            Err(MoveError::AlreadyExists) => {
                // A race created the name after the check: try further
                // suffixes the same way (bounded by unique_name).
                let mut done = false;
                let mut last = destination;
                for _ in 0..1_000 {
                    let current = last
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let next = match unique_name(&current, &|candidate| {
                        std::fs::symlink_metadata(dir.join(candidate)).is_ok()
                    }) {
                        Ok(name) => dir.join(name),
                        Err(_) => break,
                    };
                    if next == last {
                        break;
                    }
                    match move_file(&src, &next) {
                        Ok(()) => {
                            writer.record_move(&src, &next)?;
                            moved += 1;
                            done = true;
                            break;
                        }
                        Err(MoveError::AlreadyExists) => last = next,
                        Err(error) => {
                            skip_report(&file.name, move_error_text(error), &mut skipped);
                            done = true;
                            break;
                        }
                    }
                }
                if !done {
                    skip_report(&file.name, "no free name found".into(), &mut skipped);
                }
            }
            Err(error) => {
                skip_report(&file.name, move_error_text(error), &mut skipped);
            }
        }
    }
    progress(total, total);

    writer.record_end(moved, skipped.len() as u32)?;
    Ok(RunReport { moved, skipped })
}

/// Reuses an existing real folder, creates one (recorded for undo), or
/// reports a name conflict when the category name is taken by a file or a
/// reparse point.
fn ensure_category_dir(
    root: &Path,
    category: &str,
    writer: &mut manifest::ManifestWriter,
) -> Result<PathBuf, String> {
    let dir = root.join(category);
    match std::fs::symlink_metadata(&dir) {
        Ok(metadata) => {
            let flags = flags_from_attributes(raw_attributes(&metadata));
            if metadata.is_dir() && !flags.reparse {
                Ok(dir)
            } else {
                Err("name conflict".into())
            }
        }
        Err(_) => match std::fs::create_dir(&dir) {
            Ok(()) => {
                writer.record_mkdir(&dir)?;
                Ok(dir)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(dir),
            Err(error) => Err(format!("could not create the folder: {error}")),
        },
    }
}

/// The honest result of one undo.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UndoReport {
    /// Moved back to their original place.
    pub restored: u32,
    /// Restored, but under "name (2).ext" because the original name was
    /// taken in the meantime (each case also produces a note).
    pub restored_renamed: u32,
    /// The destination file no longer exists (the user moved or deleted
    /// it) — nothing to restore.
    pub not_found: u32,
    /// Retry-able failures, plain English; these stay in the manifest.
    pub failed: Vec<String>,
    /// Non-retryable explanations (for example a renamed restore).
    pub notes: Vec<String>,
    pub folders_removed: u32,
    /// Category folders that still contain something are NEVER deleted.
    pub folders_kept: u32,
}

impl UndoReport {
    pub fn summary(&self) -> String {
        let restored_total = self.restored + self.restored_renamed;
        let mut parts = vec![format!(
            "{} file{} restored",
            restored_total,
            if restored_total == 1 { "" } else { "s" }
        )];
        if self.not_found > 0 {
            parts.push(format!("{} not found", self.not_found));
        }
        if !self.failed.is_empty() {
            parts.push(format!("{} failed", self.failed.len()));
        }
        if self.folders_kept > 0 {
            parts.push(format!(
                "{} folder{} kept (not empty)",
                self.folders_kept,
                if self.folders_kept == 1 { "" } else { "s" }
            ));
        }
        parts.join(", ")
    }
}

/// Undoes the most recent run for one target: move lines in REVERSE order
/// (never overwriting — a taken original name gets a " (2)" name), then the
/// created folders in REVERSE order via remove_dir, which only succeeds
/// when the folder is EMPTY. A fully successful undo deletes the manifest;
/// a partial one rewrites it with what still needs undoing.
pub fn undo(manifest_file: &Path) -> Result<UndoReport, String> {
    let Manifest {
        moves,
        mkdirs,
        target,
        root,
        started_at,
        mode,
    } = manifest::read(manifest_file)?;
    let mut report = UndoReport::default();

    // Moves, reverse order; remember which original indices need a retry.
    let mut retry_indices: Vec<usize> = Vec::new();
    for (index, (from, to)) in moves.iter().enumerate().rev() {
        if std::fs::symlink_metadata(to).is_err() {
            // The user moved or deleted it: nothing to restore.
            report.not_found += 1;
            continue;
        }
        if std::fs::symlink_metadata(from).is_err() {
            match move_file(to, from) {
                Ok(()) => report.restored += 1,
                Err(error) => {
                    report.failed.push(format!(
                        "Could not move '{}' back: {}",
                        to.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| to.to_string_lossy().into_owned()),
                        move_error_text(error)
                    ));
                    retry_indices.push(index);
                }
            }
        } else {
            // The original name is occupied: restore under " (2)".
            let dir = from.parent().unwrap_or(Path::new("."));
            let name = from
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".into());
            match unique_name(&name, &|candidate| {
                std::fs::symlink_metadata(dir.join(candidate)).is_ok()
            }) {
                Ok(free_name) => match move_file(to, &dir.join(&free_name)) {
                    Ok(()) => {
                        report.restored_renamed += 1;
                        report.notes.push(format!(
                            "'{name}' was restored as '{free_name}' because the original name was taken."
                        ));
                    }
                    Err(error) => {
                        report.failed.push(format!(
                            "Could not move '{}' back: {}",
                            name,
                            move_error_text(error)
                        ));
                        retry_indices.push(index);
                    }
                },
                Err(error) => {
                    report.failed.push(error);
                    retry_indices.push(index);
                }
            }
        }
    }

    // Created folders, reverse order: remove_dir only succeeds when the
    // folder is EMPTY — exactly the safety we want.
    for dir in mkdirs.iter().rev() {
        match std::fs::remove_dir(dir) {
            Ok(()) => report.folders_removed += 1,
            Err(_) => report.folders_kept += 1, // not empty: leave it alone
        }
    }
    // Folders that still exist stay in the manifest: if the user empties
    // one, a later undo can still remove the EMPTY folder we created.
    let remaining_mkdirs: Vec<PathBuf> =
        mkdirs.iter().filter(|dir| dir.is_dir()).cloned().collect();

    // Only the retry-able moves stay in the manifest.
    let remaining_moves: Vec<(PathBuf, PathBuf)> = retry_indices
        .into_iter()
        .map(|index| moves[index].clone())
        .collect();

    let rewritten = Manifest {
        target,
        root,
        started_at,
        mode,
        moves: remaining_moves,
        mkdirs: remaining_mkdirs,
    };
    manifest::rewrite(manifest_file, &rewritten)?;
    Ok(report)
}
