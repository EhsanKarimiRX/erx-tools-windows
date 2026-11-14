//! Pure logic behind the Tauri commands: KDF tuning, progress throttling,
//! the single-operation guard, cleanup of stale temporary files, planning
//! and the encrypt / decrypt jobs. Nothing in here depends on Tauri, so all
//! of it is unit-tested.
//!
//! Events sent to the UI (through the Tauri channel, by the command layer):
//! - `Phase`    : "tuning" (first encryption of a run only), "deriving"
//!   (Argon2id, no percentage possible), "verifying".
//! - `Progress` : `done` / `total` bytes. The first `Progress` after
//!   "deriving" tells the UI to switch to a percentage bar.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use argon2::{Algorithm, Argon2, Params, Version};
use serde::Serialize;
use zeroize::Zeroizing;

use super::engine::{self, EncryptSettings};
use super::error::VaultError;
use super::files;
use super::folder;
use super::kdf::{KEY_LEN, KdfParams, MAX_T_COST};
use super::parts::{MAX_PARTS, PartSize, SplitReader, TEMP_EXTENSION, part_path};
use super::space;
use super::verify;

/// Argon2 memory used for every new file: 256 MiB.
pub const KDF_MEMORY_KIB: u32 = 256 * 1024;
/// The benchmark picks the number of passes so one derivation takes about
/// this long on the user's machine.
const KDF_TARGET: Duration = Duration::from_millis(700);
/// Never fewer passes than this, however fast the machine is.
const KDF_MIN_PASSES: u32 = 2;
/// At most one progress event per interval (about 12 per second).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(80);
/// Temporary files older than this are leftovers of a crashed run.
pub const STALE_TEMP_AGE: Duration = Duration::from_secs(3600);
/// Files larger than this get a "split the output" suggestion (2 GiB).
pub const SUGGEST_SPLIT_ABOVE: u64 = 2 * 1024 * 1024 * 1024;

/// Slow steps the UI shows without a percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Tuning,
    Deriving,
    Verifying,
}

/// What the UI receives while an operation runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum VaultEvent {
    Phase { phase: Phase },
    Progress { done: u64, total: u64 },
}

/// Maps the part-size names used by the UI to `PartSize`.
pub fn parse_part_size(name: &str) -> Option<PartSize> {
    match name {
        "off" => Some(PartSize::Off),
        "500mb" => Some(PartSize::Mib500),
        "1gb" => Some(PartSize::Gib1),
        "2gb" => Some(PartSize::Gib2),
        _ => None,
    }
}

/// Number of passes that makes one derivation take about `KDF_TARGET`,
/// given the measured time of a single pass.
pub fn params_for_pass_time(one_pass: Duration) -> KdfParams {
    let micros = one_pass.as_micros().max(1);
    let wanted = (KDF_TARGET.as_micros() + micros / 2) / micros;
    let passes = u32::try_from(wanted)
        .unwrap_or(MAX_T_COST)
        .clamp(KDF_MIN_PASSES, MAX_T_COST);
    KdfParams {
        m_cost_kib: KDF_MEMORY_KIB,
        t_cost: passes,
        p_cost: 1,
    }
}

/// Runs one Argon2id pass with the production memory size and derives the
/// parameters for this machine from its duration. The benchmark password
/// and salt are public constants: nothing secret is involved.
pub fn tune_kdf() -> Result<KdfParams, VaultError> {
    let params = Params::new(KDF_MEMORY_KIB, 1, 1, Some(KEY_LEN))
        .map_err(|_| VaultError::InvalidKdfParams)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = Zeroizing::new([0u8; KEY_LEN]);
    let started = Instant::now();
    argon
        .hash_password_into(b"erx-benchmark", &[0u8; 16], output.as_mut_slice())
        .map_err(|_| VaultError::KeyDerivationFailed)?;
    Ok(params_for_pass_time(started.elapsed()))
}

/// Tuned parameters, computed once per app run and kept in memory only.
pub fn cached_kdf(cache: &Mutex<Option<KdfParams>>) -> Result<KdfParams, VaultError> {
    {
        let guard = cache.lock().map_err(|_| VaultError::Internal)?;
        if let Some(params) = *guard {
            return Ok(params);
        }
    }
    let tuned = tune_kdf()?;
    let mut guard = cache.lock().map_err(|_| VaultError::Internal)?;
    *guard = Some(tuned);
    Ok(tuned)
}

/// Lets a progress event through at most once per interval.
pub struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
        }
    }

    pub fn ready(&mut self) -> bool {
        let now = Instant::now();
        match self.last {
            Some(last) if now.duration_since(last) < self.interval => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

/// Only one vault operation may run at a time. Releases on drop.
pub struct BusyGuard {
    flag: Arc<AtomicBool>,
}

impl BusyGuard {
    /// `None` when an operation is already running.
    pub fn acquire(flag: &Arc<AtomicBool>) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Self {
                flag: Arc::clone(flag),
            })
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

/// True for names made by `parts::temp_path`: `<name>.<8 hex>.erxtmp`.
pub fn is_temp_name(name: &str) -> bool {
    let suffix = format!(".{TEMP_EXTENSION}");
    let Some(rest) = name.strip_suffix(suffix.as_str()) else {
        return false;
    };
    let Some((stem, token)) = rest.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty() && token.len() == 8 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Deletes temporary files left behind by a crashed run: regular files
/// that look like our temporary names and were last changed at least
/// `min_age` ago. Returns how many were removed. Only one vault operation
/// runs at a time, so an old temporary file is never in use.
pub fn cleanup_stale_temp(dir: &Path, min_age: Duration) -> usize {
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
        if !is_temp_name(name) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let age = metadata
            .modified()
            .ok()
            .map(|changed| now.duration_since(changed).unwrap_or_default());
        if age.is_some_and(|value| value >= min_age) && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// The folder for the output: the chosen one, or the folder of `anchor`.
pub fn resolve_out_dir(anchor: &Path, chosen: Option<&Path>) -> Result<PathBuf, VaultError> {
    let dir = match chosen {
        Some(dir) => dir.to_path_buf(),
        None => match anchor.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        },
    };
    if dir.is_dir() {
        Ok(dir)
    } else {
        Err(VaultError::BadFolder)
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// How many parts exist for a first part (1 for an unsplit file).
pub fn count_parts(first: &Path) -> u32 {
    let is_split = first
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|digits| {
            digits.len() == 3 && digits.bytes().all(|byte| byte.is_ascii_digit())
        });
    if !is_split {
        return 1;
    }
    let base = first.with_extension("");
    let mut count = 0;
    for index in 1..=MAX_PARTS {
        if part_path(&base, true, index).exists() {
            count = index;
        } else {
            break;
        }
    }
    count
}

/// Everything the UI needs to know before encrypting a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptPlan {
    /// True when the source is a folder.
    pub is_folder: bool,
    /// Number of files that will be encrypted (1 for a single file).
    pub item_count: u64,
    pub file_name: String,
    pub size_bytes: u64,
    pub out_dir: PathBuf,
    /// Final path of the unsplit output (`name.erx`).
    pub base_path: PathBuf,
    /// Existing files that the output would be in the way of.
    pub conflicts: Vec<PathBuf>,
    pub compress_by_default: bool,
    pub suggest_split: bool,
    pub free_bytes: Option<u64>,
    pub required_bytes: u64,
}

pub fn plan_encrypt(
    source: &Path,
    chosen_dir: Option<&Path>,
    part_size: PartSize,
) -> Result<EncryptPlan, VaultError> {
    let metadata = fs::metadata(source).map_err(|_| VaultError::CannotOpenFile)?;
    if metadata.is_dir() {
        return folder::plan_folder(source, chosen_dir, part_size);
    }
    if !metadata.is_file() {
        return Err(VaultError::CannotOpenFile);
    }
    let out_dir = resolve_out_dir(source, chosen_dir)?;
    let base_path =
        files::hidden_output_path(source, &out_dir).ok_or(VaultError::CannotOpenFile)?;
    let size_bytes = metadata.len();
    Ok(EncryptPlan {
        file_name: file_name_of(source),
        is_folder: false,
        item_count: 1,
        size_bytes,
        conflicts: files::find_conflicts(&base_path),
        compress_by_default: engine::should_compress(source),
        suggest_split: size_bytes > SUGGEST_SPLIT_ABOVE,
        free_bytes: space::free_bytes(&out_dir),
        required_bytes: space::required_for_encrypt(size_bytes, part_size.limit_bytes()),
        out_dir,
        base_path,
    })
}

/// Everything the UI needs to know before decrypting a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecryptPlan {
    /// True when the encrypted data is a folder.
    pub is_folder: bool,
    pub file_name: String,
    pub part_count: u32,
    /// Encrypted bytes of all parts that exist right now.
    pub total_bytes: u64,
    pub out_dir: PathBuf,
    pub output_path: PathBuf,
    pub output_exists: bool,
}

pub fn plan_decrypt(first: &Path, chosen_dir: Option<&Path>) -> Result<DecryptPlan, VaultError> {
    // Opening the reader checks the file is a first part of a vault file.
    let total_bytes = SplitReader::open(first)?.logical_total();
    let is_folder = folder::peek_is_folder(first)?;
    let out_dir = resolve_out_dir(first, chosen_dir)?;
    let output_path =
        files::default_restore_path(first, &out_dir).ok_or(VaultError::CannotOpenFile)?;
    Ok(DecryptPlan {
        file_name: file_name_of(first),
        is_folder,
        part_count: count_parts(first),
        total_bytes,
        output_exists: output_path.exists(),
        out_dir,
        output_path,
    })
}

/// One encryption request, fully resolved.
#[derive(Debug, Clone)]
pub struct EncryptJob {
    pub source: PathBuf,
    pub out_dir: PathBuf,
    pub part_size: PartSize,
    pub overwrite: bool,
    pub verify: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncryptOutcome {
    Done {
        parts: Vec<PathBuf>,
        input_bytes: u64,
        output_bytes: u64,
        /// True when the check after writing ran and passed.
        verified: bool,
    },
    Cancelled,
}

/// One decryption request, fully resolved.
#[derive(Debug, Clone)]
pub struct DecryptJob {
    pub first_part: PathBuf,
    pub out_dir: PathBuf,
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecryptOutcome {
    Done {
        output_path: PathBuf,
        output_bytes: u64,
    },
    Cancelled,
}

/// Cancel check plus throttled progress event. False means: stop.
fn report(
    cancel: &AtomicBool,
    throttle: &mut Throttle,
    emit: &mut dyn FnMut(VaultEvent),
    done: u64,
    total: u64,
) -> bool {
    if cancel.load(Ordering::Relaxed) {
        return false;
    }
    if done >= total || throttle.ready() {
        emit(VaultEvent::Progress { done, total });
    }
    true
}

fn remove_parts(parts: &[PathBuf]) {
    for part in parts {
        let _ = fs::remove_file(part);
    }
}

/// Encrypts the source file; optionally verifies the result afterwards.
///
/// `passphrase` is the normalized passphrase of the confirmed session.
/// A cancel during encryption leaves no files. A failed check removes the
/// new files (the original is never touched); a cancel during the check
/// keeps them and reports `verified: false`.
pub fn run_encrypt(
    job: &EncryptJob,
    passphrase: &str,
    settings: &EncryptSettings,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(VaultEvent),
) -> Result<EncryptOutcome, VaultError> {
    cleanup_stale_temp(&job.out_dir, STALE_TEMP_AGE);
    let base =
        folder::output_path_for(&job.source, &job.out_dir).ok_or(VaultError::CannotOpenFile)?;

    emit(VaultEvent::Phase {
        phase: Phase::Deriving,
    });
    let mut throttle = Throttle::new(PROGRESS_INTERVAL);
    let result = folder::encrypt_path(
        &job.source,
        &base,
        job.part_size.limit_bytes(),
        passphrase,
        settings,
        job.overwrite,
        &mut |done: u64, total: u64| report(cancel, &mut throttle, emit, done, total),
    );
    let summary = match result {
        Ok(summary) => summary,
        Err(VaultError::Cancelled) => return Ok(EncryptOutcome::Cancelled),
        Err(error) => return Err(error),
    };

    let mut verified = false;
    if job.verify {
        emit(VaultEvent::Phase {
            phase: Phase::Verifying,
        });
        let first = summary.parts.first().ok_or(VaultError::Internal)?;
        let mut verify_throttle = Throttle::new(PROGRESS_INTERVAL);
        let checked = verify::verify_file(first, passphrase, &mut |done: u64, total: u64| {
            report(cancel, &mut verify_throttle, emit, done, total)
        });
        match checked {
            Ok(()) => verified = true,
            Err(VaultError::Cancelled) => {}
            Err(_) => {
                remove_parts(&summary.parts);
                return Err(VaultError::VerifyFailed);
            }
        }
    }

    Ok(EncryptOutcome::Done {
        parts: summary.parts,
        input_bytes: summary.input_bytes,
        output_bytes: summary.output_bytes,
        verified,
    })
}

/// Decrypts the file set starting at `job.first_part`. `words` is what
/// the user typed. A cancel leaves no files.
pub fn run_decrypt(
    job: &DecryptJob,
    words: &str,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(VaultEvent),
) -> Result<DecryptOutcome, VaultError> {
    cleanup_stale_temp(&job.out_dir, STALE_TEMP_AGE);
    let out_path = files::default_restore_path(&job.first_part, &job.out_dir)
        .ok_or(VaultError::CannotOpenFile)?;
    let total = SplitReader::open(&job.first_part)?.logical_total();

    emit(VaultEvent::Phase {
        phase: Phase::Deriving,
    });
    let mut throttle = Throttle::new(PROGRESS_INTERVAL);
    let result = folder::decrypt_path(
        &job.first_part,
        &out_path,
        words,
        job.overwrite,
        &mut |done: u64, seen_total: u64| report(cancel, &mut throttle, emit, done, seen_total),
    );
    match result {
        Ok(summary) => {
            emit(VaultEvent::Progress { done: total, total });
            Ok(DecryptOutcome::Done {
                output_path: summary.output_path,
                output_bytes: summary.output_bytes,
            })
        }
        Err(VaultError::Cancelled) => Ok(DecryptOutcome::Cancelled),
        Err(error) => Err(error),
    }
}
