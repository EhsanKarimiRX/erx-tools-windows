//! Folder encryption: a folder is turned into one archive stream (see
//! `archive`), encrypted by the same engine as a single file, and restored
//! into a NEW folder.
//!
//! Rules implemented here, in the spirit of `files`:
//! the source folder is only ever read; the output must not be inside the
//! folder; nothing existing is overwritten (a restored folder that would
//! collide gets a number); on ANY error or cancel every temporary file and
//! folder is removed; a restored folder only gets its final name after the
//! integrity check passed. The header flag says "folder" so the decrypting
//! side knows before it starts; the name of the folder is stored inside the
//! encryption.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::archive::{self, ArchiveReader, ArchiveWriter};
use super::engine::{self, EncryptSettings};
use super::error::VaultError;
use super::files::{self, DecryptFileSummary, EncryptFileSummary};
use super::format::{HEADER_LEN, Header};
use super::meta::{FileMeta, KIND_FOLDER};
use super::names::safe_file_name;
use super::parts::{PartSize, SplitReader, SplitWriter, random_token};
use super::runtime::{EncryptPlan, STALE_TEMP_AGE, SUGGEST_SPLIT_ABOVE, resolve_out_dir};
use super::space;

/// Extension of temporary folders (files use `erxtmp`).
const TEMP_DIR_EXTENSION: &str = "erxtmpdir";
/// File inside every temporary folder: only folders that contain it are
/// ever cleaned up, so a folder of the user's is never removed.
pub const INCOMPLETE_MARKER: &str = "erx-vault-incomplete.marker";

fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// `<final path>.<token>.erxtmpdir`
fn temp_dir_path(final_path: &Path, token: &str) -> PathBuf {
    let mut name = final_path.as_os_str().to_os_string();
    name.push(format!(".{token}.{TEMP_DIR_EXTENSION}"));
    PathBuf::from(name)
}

fn is_temp_dir_name(name: &str) -> bool {
    let suffix = format!(".{TEMP_DIR_EXTENSION}");
    let Some(rest) = name.strip_suffix(suffix.as_str()) else {
        return false;
    };
    let Some((stem, token)) = rest.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty() && token.len() == 8 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Removes temporary folders left behind by a crashed decryption: folders
/// with our temporary name, containing the marker file, last changed at
/// least `min_age` ago. Returns how many were removed.
pub fn cleanup_stale_temp_dirs(dir: &Path, min_age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let os_name = entry.file_name();
        let Some(name) = os_name.to_str() else {
            continue;
        };
        if !is_temp_dir_name(name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        if !path.join(INCOMPLETE_MARKER).is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let age = metadata
            .modified()
            .ok()
            .map(|changed| now.duration_since(changed).unwrap_or_default());
        if age.is_some_and(|value| value >= min_age) && fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Deletes the temporary folder unless `keep` was set.
struct TempDir {
    path: PathBuf,
    keep: bool,
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn folder_name(source: &Path) -> Result<String, VaultError> {
    let name = source
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .ok_or(VaultError::BadFolder)?;
    if name.is_empty() {
        return Err(VaultError::BadFolder);
    }
    Ok(name)
}

/// The output folder must not be the source folder or inside it, or the
/// archive would swallow its own output.
fn ensure_outside(source: &Path, out_dir: &Path) -> Result<(), VaultError> {
    let (Ok(source_real), Ok(out_real)) = (fs::canonicalize(source), fs::canonicalize(out_dir))
    else {
        return Err(VaultError::BadFolder);
    };
    if out_real.starts_with(&source_real) {
        Err(VaultError::OutputInsideFolder)
    } else {
        Ok(())
    }
}

/// `<out_dir>/<folder name>.erx` for a folder (the whole name is kept);
/// for a file the name without its last extension, so the type is hidden.
pub fn output_path_for(source: &Path, out_dir: &Path) -> Option<PathBuf> {
    if source.is_dir() {
        files::default_output_path(source, out_dir)
    } else {
        files::hidden_output_path(source, out_dir)
    }
}

/// Facts about a folder the user wants to encrypt.
pub fn plan_folder(
    source: &Path,
    chosen_dir: Option<&Path>,
    part_size: PartSize,
) -> Result<EncryptPlan, VaultError> {
    let name = folder_name(source)?;
    let out_dir = resolve_out_dir(source, chosen_dir)?;
    ensure_outside(source, &out_dir)?;
    let scan = archive::scan_folder(source)?;
    let base_path = files::default_output_path(source, &out_dir).ok_or(VaultError::BadFolder)?;
    Ok(EncryptPlan {
        is_folder: true,
        item_count: scan.file_count,
        file_name: name,
        size_bytes: scan.stream_len,
        conflicts: files::find_conflicts(&base_path, part_size.limit_bytes().is_some()),
        compress_by_default: true,
        suggest_split: scan.stream_len > SUGGEST_SPLIT_ABOVE,
        free_bytes: space::free_bytes(&out_dir),
        required_bytes: space::required_for_encrypt(scan.stream_len, part_size.limit_bytes()),
        out_dir,
        base_path,
    })
}

/// Encrypts a file or a folder: same signature as `files::encrypt_file`.
pub fn encrypt_path(
    source: &Path,
    base: &Path,
    part_limit: Option<u64>,
    passphrase: &str,
    settings: &EncryptSettings,
    overwrite: bool,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<EncryptFileSummary, VaultError> {
    if source.is_dir() {
        encrypt_folder(
            source, base, part_limit, passphrase, settings, overwrite, progress,
        )
    } else {
        files::encrypt_file(
            source, base, part_limit, passphrase, settings, overwrite, progress,
        )
    }
}

/// Encrypts a whole folder into `base` (`name.erx`, or `name.001.erx`...).
/// `progress(done, total)` counts archive bytes; returning false cancels.
pub fn encrypt_folder(
    source: &Path,
    base: &Path,
    part_limit: Option<u64>,
    passphrase: &str,
    settings: &EncryptSettings,
    overwrite: bool,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<EncryptFileSummary, VaultError> {
    let name = folder_name(source)?;
    let meta = FileMeta::folder(&name)?;
    let dir = parent_dir(base);
    ensure_outside(source, dir)?;
    let scan = archive::scan_folder(source)?;
    let total = scan.stream_len;
    space::check_space_for_encrypt(dir, total, part_limit)?;
    let split = part_limit.is_some();
    if !overwrite && !files::find_conflicts(base, split).is_empty() {
        return Err(VaultError::OutputExists);
    }

    let mut reader = ArchiveReader::new(scan);
    // Dropping the writer on any early return deletes the temporary files.
    let mut writer = SplitWriter::new(base, part_limit, random_token()?)?;
    engine::encrypt_stream_meta(
        &mut reader,
        &mut writer,
        passphrase,
        settings,
        &meta,
        &mut |done: u64| progress(done, total),
    )?;
    let parts = writer.finish()?;

    let mut output_bytes = 0u64;
    for part in &parts {
        let size = fs::metadata(&part.temp)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        output_bytes = output_bytes.saturating_add(size);
    }
    files::commit(base, &parts, split, overwrite)?;

    Ok(EncryptFileSummary {
        parts: parts.into_iter().map(|part| part.final_path).collect(),
        input_bytes: total,
        output_bytes,
    })
}

/// True when the vault file starts a folder archive (read from the header,
/// before any decryption).
pub fn peek_is_folder(first_part: &Path) -> Result<bool, VaultError> {
    let mut reader = SplitReader::open(first_part)?;
    let mut head = Vec::with_capacity(HEADER_LEN);
    reader
        .by_ref()
        .take(HEADER_LEN as u64)
        .read_to_end(&mut head)?;
    Ok(Header::parse(&head)?.is_folder())
}

/// Decrypts a file or a folder: same signature as
/// `files::decrypt_file_restoring_name`.
pub fn decrypt_path(
    first_part: &Path,
    fallback_path: &Path,
    typed_passphrase: &str,
    overwrite: bool,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<DecryptFileSummary, VaultError> {
    if peek_is_folder(first_part)? {
        decrypt_folder(
            first_part,
            fallback_path,
            typed_passphrase,
            overwrite,
            progress,
        )
    } else {
        files::decrypt_file_restoring_name(
            first_part,
            fallback_path,
            typed_passphrase,
            overwrite,
            progress,
        )
    }
}

/// The final folder name: the stored one if it is safe, else the fallback;
/// a number is added when the name is taken.
fn restored_folder_path(fallback: &Path, stored_name: &str) -> Result<PathBuf, VaultError> {
    let dir = parent_dir(fallback);
    let wanted = match safe_file_name(stored_name) {
        Some(name) => dir.join(name),
        None => fallback.to_path_buf(),
    };
    if !wanted.exists() {
        return Ok(wanted);
    }
    let base_name = wanted
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    for number in 1..1000u32 {
        let candidate = dir.join(format!("{base_name} ({number})"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(VaultError::OutputExists)
}

/// Decrypts a folder archive into a NEW folder next to `fallback_path`.
/// An existing folder is never replaced (`_overwrite` is ignored on
/// purpose). Written under a temporary name; renamed after the integrity
/// check. `progress(done, total)` counts encrypted bytes.
pub fn decrypt_folder(
    first_part: &Path,
    fallback_path: &Path,
    typed_passphrase: &str,
    _overwrite: bool,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<DecryptFileSummary, VaultError> {
    let mut reader = SplitReader::open(first_part)?;
    let total = reader.logical_total();
    cleanup_stale_temp_dirs(parent_dir(fallback_path), STALE_TEMP_AGE);

    let temp_root = temp_dir_path(fallback_path, &random_token()?);
    fs::create_dir(&temp_root).map_err(VaultError::from)?;
    // Declared before the writer, so the writer is dropped (and its file
    // closed) before the temporary folder is deleted.
    let mut guard = TempDir {
        path: temp_root.clone(),
        keep: false,
    };
    fs::write(temp_root.join(INCOMPLETE_MARKER), b"").map_err(VaultError::from)?;

    let mut writer = ArchiveWriter::new(temp_root.clone());
    let summary = engine::decrypt_stream(
        &mut reader,
        &mut writer,
        typed_passphrase,
        &mut |done: u64| progress(done, total),
    )?;
    if summary.meta.kind != KIND_FOLDER {
        return Err(VaultError::CorruptedData);
    }
    let stats = writer.finish()?;

    let final_path = restored_folder_path(fallback_path, &summary.meta.name)?;
    fs::remove_file(temp_root.join(INCOMPLETE_MARKER)).map_err(VaultError::from)?;
    files::rename_with_retry(&temp_root, &final_path).map_err(|_| VaultError::Io)?;
    guard.keep = true;
    Ok(DecryptFileSummary {
        output_path: final_path,
        output_bytes: stats.bytes,
    })
}
