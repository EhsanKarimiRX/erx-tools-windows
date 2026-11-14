//! Tauri command layer for the Quantum Resistance Vault.
//!
//! Thin by design: validate input, call `runtime`, shape the result.
//! Dialogs run on the Rust side (the webview needs no permissions beyond
//! `core:default`). The passphrase lives ONLY in `VaultState`: React gets
//! the words for display, `vault_encrypt_file` never receives words, and
//! only `vault_decrypt_file` receives what the user typed. Heavy work runs
//! in `spawn_blocking`; progress streams through a channel.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

use super::engine::EncryptSettings;
use super::error::VaultError;
use super::kdf::KdfParams;
use super::parts::PartSize;
use super::passphrase::{PassphraseError, PassphraseSession, strength};
use super::runtime::{
    BusyGuard, DecryptJob, DecryptOutcome, EncryptJob, EncryptOutcome, Phase, VaultEvent,
    cached_kdf, parse_part_size, plan_decrypt, plan_encrypt, resolve_out_dir, run_decrypt,
    run_encrypt,
};
use crate::tools::ToolError;

/// Shared state of the vault: the passphrase session, the cancel flag, the
/// "operation running" flag and the tuned Argon2 parameters (memory only).
#[derive(Default)]
pub struct VaultState {
    session: Mutex<PassphraseSession>,
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    kdf: Arc<Mutex<Option<KdfParams>>>,
}

impl VaultState {
    fn lock_session(&self) -> Result<MutexGuard<'_, PassphraseSession>, ToolError> {
        self.session.lock().map_err(|_| {
            ToolError::Internal("The vault is in an unexpected state. Restart the app.".into())
        })
    }

    /// Passphrase changes are refused while an operation is running.
    fn ensure_idle(&self) -> Result<(), ToolError> {
        if self.busy.load(Ordering::SeqCst) {
            Err(user_error(VaultError::Busy))
        } else {
            Ok(())
        }
    }
}

fn user_error(error: VaultError) -> ToolError {
    ToolError::Internal(error.to_string())
}

fn passphrase_error(error: PassphraseError) -> ToolError {
    ToolError::Internal(error.to_string())
}

fn unknown_part_size() -> ToolError {
    ToolError::Internal("Unknown part size.".into())
}

fn path_text(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// Passphrase
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrengthDto {
    pub bits: f64,
    pub grover_bits: f64,
    pub is_weak: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PassphraseDto {
    pub words: Vec<String>,
    /// Replacements left per word slot, in slot order.
    pub replacements_left: Vec<u8>,
    pub strength: StrengthDto,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshDto {
    pub word: String,
    pub replacements_left: u8,
}

fn passphrase_dto(session: &PassphraseSession, words: Vec<String>) -> PassphraseDto {
    let count = words.len();
    let figures = strength(count);
    PassphraseDto {
        replacements_left: (0..count)
            .filter_map(|slot| session.replacements_left(slot))
            .collect(),
        strength: StrengthDto {
            bits: figures.bits,
            grover_bits: figures.grover_bits,
            is_weak: figures.is_weak,
        },
        words,
    }
}

/// Starts a new passphrase session with `count` words (6 to 12).
#[tauri::command]
pub fn vault_generate_passphrase(
    state: State<'_, VaultState>,
    count: usize,
) -> Result<PassphraseDto, ToolError> {
    state.ensure_idle()?;
    let mut session = state.lock_session()?;
    let words = session.generate(count).map_err(passphrase_error)?;
    Ok(passphrase_dto(&session, words))
}

/// Replaces the word in `slot` (at most 3 times per slot, one-way).
#[tauri::command]
pub fn vault_refresh_word(
    state: State<'_, VaultState>,
    slot: usize,
) -> Result<RefreshDto, ToolError> {
    state.ensure_idle()?;
    let mut session = state.lock_session()?;
    let word = session.refresh(slot).map_err(passphrase_error)?;
    let replacements_left = session.replacements_left(slot).unwrap_or(0);
    Ok(RefreshDto {
        word,
        replacements_left,
    })
}

/// The user confirmed "I saved it": the words are frozen.
#[tauri::command]
pub fn vault_confirm_saved(state: State<'_, VaultState>) -> Result<(), ToolError> {
    state.ensure_idle()?;
    let mut session = state.lock_session()?;
    session.confirm_saved().map_err(passphrase_error)
}

/// Wipes the passphrase from memory. Always allowed.
#[tauri::command]
pub fn vault_clear_passphrase(state: State<'_, VaultState>) -> Result<(), ToolError> {
    let mut session = state.lock_session()?;
    session.clear();
    Ok(())
}

// ---------------------------------------------------------------------------
// Dialogs (Rust side; absolute paths, or null on cancel)
// ---------------------------------------------------------------------------

/// Native dialog to pick the file to encrypt.
#[tauri::command]
pub fn vault_pick_source_file(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("All files", &["*"])
        .blocking_pick_file()
        .and_then(|file| file.into_path().ok())
        .map(path_text)
}

/// Native dialog to pick the first part of an encrypted file.
#[tauri::command]
pub fn vault_pick_vault_file(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("ERX vault files", &["erx", "001"])
        .add_filter("All files", &["*"])
        .blocking_pick_file()
        .and_then(|file| file.into_path().ok())
        .map(path_text)
}

/// Native dialog to pick an output folder.
#[tauri::command]
pub fn vault_pick_folder(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|folder| folder.into_path().ok())
        .map(path_text)
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptPlanDto {
    pub file_name: String,
    pub size_bytes: u64,
    pub output_dir: String,
    pub output_base_path: String,
    pub conflicts: Vec<String>,
    pub compress_by_default: bool,
    pub suggest_split: bool,
    pub free_bytes: Option<u64>,
    pub required_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptPlanDto {
    pub file_name: String,
    pub part_count: u32,
    pub total_bytes: u64,
    pub output_dir: String,
    pub output_path: String,
    pub output_exists: bool,
}

/// Facts about a file the user wants to encrypt (size, output name,
/// existing files in the way, free space, compression advice).
#[tauri::command]
pub fn vault_prepare_encrypt(
    path: String,
    output_dir: Option<String>,
    part_size: String,
) -> Result<EncryptPlanDto, ToolError> {
    let size = parse_part_size(&part_size).ok_or_else(unknown_part_size)?;
    let chosen = output_dir.map(PathBuf::from);
    let plan = plan_encrypt(Path::new(&path), chosen.as_deref(), size).map_err(user_error)?;
    Ok(EncryptPlanDto {
        file_name: plan.file_name,
        size_bytes: plan.size_bytes,
        output_dir: path_text(&plan.out_dir),
        output_base_path: path_text(&plan.base_path),
        conflicts: plan.conflicts.iter().map(path_text).collect(),
        compress_by_default: plan.compress_by_default,
        suggest_split: plan.suggest_split,
        free_bytes: plan.free_bytes,
        required_bytes: plan.required_bytes,
    })
}

/// Facts about a file the user wants to decrypt (parts found, output name).
#[tauri::command]
pub fn vault_prepare_decrypt(
    first_part: String,
    output_dir: Option<String>,
) -> Result<DecryptPlanDto, ToolError> {
    let chosen = output_dir.map(PathBuf::from);
    let plan = plan_decrypt(Path::new(&first_part), chosen.as_deref()).map_err(user_error)?;
    Ok(DecryptPlanDto {
        file_name: plan.file_name,
        part_count: plan.part_count,
        total_bytes: plan.total_bytes,
        output_dir: path_text(&plan.out_dir),
        output_path: path_text(&plan.output_path),
        output_exists: plan.output_exists,
    })
}

// ---------------------------------------------------------------------------
// Encrypt / decrypt / cancel
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptRequestDto {
    pub path: String,
    pub output_dir: Option<String>,
    /// "off", "500mb", "1gb" or "2gb".
    pub part_size: String,
    pub compress: bool,
    pub overwrite: bool,
    pub verify: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptResultDto {
    pub cancelled: bool,
    pub parts: Vec<String>,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub verified: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptRequestDto {
    pub first_part: String,
    pub output_dir: Option<String>,
    /// The words exactly as the user typed them.
    pub words: String,
    pub overwrite: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptResultDto {
    pub cancelled: bool,
    pub output_path: String,
    pub output_bytes: u64,
}

fn encrypt_blocking(
    request: EncryptRequestDto,
    part_size: PartSize,
    passphrase: Zeroizing<String>,
    cancel: Arc<AtomicBool>,
    kdf_cache: Arc<Mutex<Option<KdfParams>>>,
    channel: Channel<VaultEvent>,
) -> Result<EncryptOutcome, VaultError> {
    let source = PathBuf::from(&request.path);
    let chosen = request.output_dir.as_deref().map(Path::new);
    let out_dir = resolve_out_dir(&source, chosen)?;

    let mut send = |event: VaultEvent| {
        let _ = channel.send(event);
    };
    let needs_tuning = kdf_cache
        .lock()
        .map(|guard| guard.is_none())
        .unwrap_or(true);
    if needs_tuning {
        send(VaultEvent::Phase {
            phase: Phase::Tuning,
        });
    }
    let kdf = cached_kdf(&kdf_cache)?;
    let settings = EncryptSettings::new(request.compress, kdf)?;
    let job = EncryptJob {
        source,
        out_dir,
        part_size,
        overwrite: request.overwrite,
        verify: request.verify,
    };
    run_encrypt(&job, &passphrase, &settings, &cancel, &mut send)
}

fn decrypt_blocking(
    first_part: String,
    output_dir: Option<String>,
    words: Zeroizing<String>,
    overwrite: bool,
    cancel: Arc<AtomicBool>,
    channel: Channel<VaultEvent>,
) -> Result<DecryptOutcome, VaultError> {
    let first = PathBuf::from(&first_part);
    let chosen = output_dir.as_deref().map(Path::new);
    let out_dir = resolve_out_dir(&first, chosen)?;

    let mut send = |event: VaultEvent| {
        let _ = channel.send(event);
    };
    let job = DecryptJob {
        first_part: first,
        out_dir,
        overwrite,
    };
    run_decrypt(&job, &words, &cancel, &mut send)
}

/// Encrypts a file with the confirmed passphrase of the current session.
/// On success the session is wiped. The command returns `cancelled: true`
/// (not an error) when the user cancelled.
#[tauri::command]
pub async fn vault_encrypt_file(
    state: State<'_, VaultState>,
    on_progress: Channel<VaultEvent>,
    request: EncryptRequestDto,
) -> Result<EncryptResultDto, ToolError> {
    let busy = BusyGuard::acquire(&state.busy).ok_or_else(|| user_error(VaultError::Busy))?;
    state.cancel.store(false, Ordering::SeqCst);
    let part_size = parse_part_size(&request.part_size).ok_or_else(unknown_part_size)?;
    let passphrase = {
        let session = state.lock_session()?;
        session.normalized_for_kdf().map_err(passphrase_error)?
    };
    let cancel = Arc::clone(&state.cancel);
    let kdf_cache = Arc::clone(&state.kdf);

    let task = tauri::async_runtime::spawn_blocking(move || {
        // Released when the work ends.
        let _busy = busy;
        encrypt_blocking(
            request,
            part_size,
            passphrase,
            cancel,
            kdf_cache,
            on_progress,
        )
    });
    let outcome = task
        .await
        .map_err(|error| ToolError::Internal(error.to_string()))?
        .map_err(user_error)?;

    match outcome {
        EncryptOutcome::Cancelled => Ok(EncryptResultDto {
            cancelled: true,
            parts: Vec::new(),
            input_bytes: 0,
            output_bytes: 0,
            verified: false,
        }),
        EncryptOutcome::Done {
            parts,
            input_bytes,
            output_bytes,
            verified,
        } => {
            // The passphrase is no longer needed in memory.
            if let Ok(mut session) = state.session.lock() {
                session.clear();
            }
            Ok(EncryptResultDto {
                cancelled: false,
                parts: parts.iter().map(path_text).collect(),
                input_bytes,
                output_bytes,
                verified,
            })
        }
    }
}

/// Decrypts the file set starting at the first part, using the words the
/// user typed.
#[tauri::command]
pub async fn vault_decrypt_file(
    state: State<'_, VaultState>,
    on_progress: Channel<VaultEvent>,
    request: DecryptRequestDto,
) -> Result<DecryptResultDto, ToolError> {
    let busy = BusyGuard::acquire(&state.busy).ok_or_else(|| user_error(VaultError::Busy))?;
    state.cancel.store(false, Ordering::SeqCst);
    let DecryptRequestDto {
        first_part,
        output_dir,
        words,
        overwrite,
    } = request;
    let words = Zeroizing::new(words);
    let cancel = Arc::clone(&state.cancel);

    let task = tauri::async_runtime::spawn_blocking(move || {
        // Released when the work ends.
        let _busy = busy;
        decrypt_blocking(
            first_part,
            output_dir,
            words,
            overwrite,
            cancel,
            on_progress,
        )
    });
    let outcome = task
        .await
        .map_err(|error| ToolError::Internal(error.to_string()))?
        .map_err(user_error)?;

    Ok(match outcome {
        DecryptOutcome::Cancelled => DecryptResultDto {
            cancelled: true,
            output_path: String::new(),
            output_bytes: 0,
        },
        DecryptOutcome::Done {
            output_path,
            output_bytes,
        } => DecryptResultDto {
            cancelled: false,
            output_path: path_text(&output_path),
            output_bytes,
        },
    })
}

/// Asks the running operation to stop. Safe to call when nothing runs.
#[tauri::command]
pub fn vault_cancel(state: State<'_, VaultState>) {
    state.cancel.store(true, Ordering::SeqCst);
}
