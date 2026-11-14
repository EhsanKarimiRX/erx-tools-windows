//! File-level encrypt / decrypt: opens the files, runs the engine, writes
//! under temporary names and only renames when everything succeeded.
//!
//! Rules implemented here:
//! - the source file is only ever read, never changed or deleted;
//! - nothing existing is overwritten unless `overwrite` is true, and the
//!   source itself is never overwritten;
//! - on ANY error or cancel every temporary file is removed;
//! - the final rename happens only after all parts are complete;
//! - the output name of an encrypted file hides the file type; the original
//!   name is stored inside the encryption and restored when decrypting,
//!   after it passed `names::safe_file_name`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::engine::{self, EncryptSettings};
use super::error::VaultError;
use super::names::{numbered_name, safe_file_name};
use super::parts::{
    MAX_PARTS, PartPaths, SplitReader, SplitWriter, part_path, random_token, temp_path,
};
use super::space;

/// Buffer between the zstd decoder and the output file.
const WRITE_BUFFER: usize = 1 << 20;
/// Windows antivirus can briefly lock a freshly written file.
const RENAME_ATTEMPTS: u32 = 5;
const RENAME_WAIT: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptFileSummary {
    /// Final paths of all parts, in order.
    pub parts: Vec<PathBuf>,
    /// Size of the source file.
    pub input_bytes: u64,
    /// Total size of all written files.
    pub output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecryptFileSummary {
    pub output_path: PathBuf,
    pub output_bytes: u64,
}

/// `<out_dir>/<source file name>.erx` (keeps the original extension; the
/// app itself uses `hidden_output_path`).
pub fn default_output_path(source: &Path, out_dir: &Path) -> Option<PathBuf> {
    let mut name = source.file_name()?.to_os_string();
    name.push(".erx");
    Some(out_dir.join(name))
}

/// `<out_dir>/<source file name without its last extension>.erx`, so the
/// file type is not visible in the name of the encrypted file.
pub fn hidden_output_path(source: &Path, out_dir: &Path) -> Option<PathBuf> {
    let mut name = source.file_stem()?.to_os_string();
    name.push(".erx");
    Some(out_dir.join(name))
}

/// Suggested name of the restored file: the part number and the `.erx`
/// ending are removed; if neither was there, `.restored` is added.
pub fn default_restore_path(first_part: &Path, out_dir: &Path) -> Option<PathBuf> {
    let mut name = first_part.file_name()?.to_string_lossy().into_owned();
    let mut changed = false;
    if let Some((stem, digits)) = name.rsplit_once('.')
        && digits.len() == 3
        && digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        let stripped = stem.to_owned();
        name = stripped;
        changed = true;
    }
    if name.len() > 4 && name.to_ascii_lowercase().ends_with(".erx") {
        for _ in 0..4 {
            name.pop();
        }
        changed = true;
    }
    if !changed {
        name.push_str(".restored");
    }
    Some(out_dir.join(name))
}

/// Every existing file that a new output with this base path would be in
/// the way of: `name.erx` and `name.erx.001` ... `name.erx.999`.
pub fn find_conflicts(base: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if base.exists() {
        found.push(base.to_path_buf());
    }
    for index in 1..=MAX_PARTS {
        let path = part_path(base, true, index);
        if path.exists() {
            found.push(path);
        }
    }
    found
}

fn same_file(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(a), Ok(b)) => a == b,
        _ => first == second,
    }
}

pub(super) fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) => {
                attempt += 1;
                if attempt >= RENAME_ATTEMPTS {
                    return Err(error);
                }
                std::thread::sleep(RENAME_WAIT);
            }
        }
    }
}

/// Moves the finished temporary parts to their final names. Temporary
/// files are removed on every failure.
pub(super) fn commit(base: &Path, parts: &[PartPaths], overwrite: bool) -> Result<(), VaultError> {
    let cleanup = || {
        for part in parts {
            let _ = fs::remove_file(&part.temp);
        }
    };
    // Re-checked here: the folder may have changed while encrypting.
    let conflicts = find_conflicts(base);
    if !conflicts.is_empty() {
        if !overwrite {
            cleanup();
            return Err(VaultError::OutputExists);
        }
        for old in &conflicts {
            if fs::remove_file(old).is_err() {
                cleanup();
                return Err(VaultError::Io);
            }
        }
    }
    let mut renamed: Vec<&Path> = Vec::new();
    for part in parts {
        if rename_with_retry(&part.temp, &part.final_path).is_err() {
            for done in &renamed {
                let _ = fs::remove_file(done);
            }
            cleanup();
            return Err(VaultError::Io);
        }
        renamed.push(part.final_path.as_path());
    }
    Ok(())
}

/// Encrypts `source` into `base` (`name.erx`, or `name.erx.001`... when
/// `part_limit` is set). The original file name is stored inside the
/// encryption.
///
/// `progress(done, total)` counts source bytes; returning false cancels.
/// The caller decides about compression through `settings`.
pub fn encrypt_file(
    source: &Path,
    base: &Path,
    part_limit: Option<u64>,
    passphrase: &str,
    settings: &EncryptSettings,
    overwrite: bool,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<EncryptFileSummary, VaultError> {
    let mut input = File::open(source).map_err(|_| VaultError::CannotOpenFile)?;
    let metadata = input.metadata().map_err(|_| VaultError::CannotOpenFile)?;
    if !metadata.is_file() {
        return Err(VaultError::CannotOpenFile);
    }
    let total = metadata.len();
    let original_name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    let dir = match base.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    space::check_space_for_encrypt(dir, total, part_limit)?;

    let conflicts = find_conflicts(base);
    if conflicts.iter().any(|path| same_file(path, source)) {
        return Err(VaultError::OutputIsSource);
    }
    if !overwrite && !conflicts.is_empty() {
        return Err(VaultError::OutputExists);
    }

    // Dropping the writer on any early return deletes the temporary files.
    let mut writer = SplitWriter::new(base, part_limit, random_token()?)?;
    engine::encrypt_stream_named(
        &mut input,
        &mut writer,
        passphrase,
        settings,
        &original_name,
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
    commit(base, &parts, overwrite)?;

    Ok(EncryptFileSummary {
        parts: parts.into_iter().map(|part| part.final_path).collect(),
        input_bytes: total,
        output_bytes,
    })
}

/// Deletes the temporary output file unless `keep` was set.
struct TempFile {
    path: PathBuf,
    keep: bool,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Where a decrypted file goes.
#[derive(Clone, Copy)]
enum Target<'a> {
    /// Exactly this path; an existing file needs `overwrite`.
    Exact(&'a Path),
    /// The original name stored inside the file, in the folder of
    /// `fallback`. `fallback` itself is used when the stored name is empty
    /// or unsafe.
    Restore { fallback: &'a Path },
}

/// The final path for a restored file. An existing file is only replaced
/// when `overwrite` is true; otherwise a number is added to the name.
fn restored_path(
    fallback: &Path,
    stored_name: &str,
    first_part: &Path,
    overwrite: bool,
) -> Result<PathBuf, VaultError> {
    let dir = match fallback.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let wanted = match safe_file_name(stored_name) {
        Some(name) => dir.join(name),
        None => fallback.to_path_buf(),
    };
    if overwrite && !same_file(&wanted, first_part) {
        return Ok(wanted);
    }
    if !wanted.exists() {
        return Ok(wanted);
    }
    let base_name = wanted
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    for number in 1..1000u32 {
        let candidate = dir.join(numbered_name(&base_name, number));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(VaultError::OutputExists)
}

fn decrypt_inner(
    first_part: &Path,
    target: Target<'_>,
    typed_passphrase: &str,
    overwrite: bool,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<DecryptFileSummary, VaultError> {
    let mut reader = SplitReader::open(first_part)?;
    let total = reader.logical_total();
    let base_path = match target {
        Target::Exact(path) => path,
        Target::Restore { fallback } => fallback,
    };
    if same_file(base_path, first_part) {
        return Err(VaultError::OutputIsSource);
    }
    if let Target::Exact(path) = target
        && path.exists()
        && !overwrite
    {
        return Err(VaultError::OutputExists);
    }

    let temp = temp_path(base_path, &random_token()?);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(VaultError::from)?;
    // Declared before the writer, so the file handle is closed first and
    // the temporary file can then be deleted.
    let mut guard = TempFile {
        path: temp,
        keep: false,
    };
    let mut writer = BufWriter::with_capacity(WRITE_BUFFER, file);
    let summary = engine::decrypt_stream(
        &mut reader,
        &mut writer,
        typed_passphrase,
        &mut |done: u64| progress(done, total),
    )?;
    writer.flush().map_err(VaultError::from)?;
    let file = writer.into_inner().map_err(|_| VaultError::Io)?;
    file.sync_all().map_err(VaultError::from)?;
    drop(file);

    let final_path = match target {
        Target::Exact(path) => {
            if !overwrite && path.exists() {
                return Err(VaultError::OutputExists);
            }
            path.to_path_buf()
        }
        Target::Restore { fallback } => {
            restored_path(fallback, &summary.meta.name, first_part, overwrite)?
        }
    };
    rename_with_retry(&guard.path, &final_path).map_err(|_| VaultError::Io)?;
    guard.keep = true;
    Ok(DecryptFileSummary {
        output_path: final_path,
        output_bytes: summary.output_bytes,
    })
}

/// Decrypts the file set starting at `first_part` into exactly `out_path`.
///
/// `progress(done, total)` counts encrypted input bytes; returning false
/// cancels. The output is written under a temporary name and only
/// renamed once the integrity check passed.
pub fn decrypt_file(
    first_part: &Path,
    out_path: &Path,
    typed_passphrase: &str,
    overwrite: bool,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<DecryptFileSummary, VaultError> {
    decrypt_inner(
        first_part,
        Target::Exact(out_path),
        typed_passphrase,
        overwrite,
        progress,
    )
}

/// Decrypts the file set and gives the result its ORIGINAL name (stored
/// inside the encryption) in the folder of `fallback_path`. Unsafe or
/// missing names fall back to `fallback_path`. An existing file is never
/// replaced unless `overwrite` is true; a number is added instead.
pub fn decrypt_file_restoring_name(
    first_part: &Path,
    fallback_path: &Path,
    typed_passphrase: &str,
    overwrite: bool,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<DecryptFileSummary, VaultError> {
    decrypt_inner(
        first_part,
        Target::Restore {
            fallback: fallback_path,
        },
        typed_passphrase,
        overwrite,
        progress,
    )
}
