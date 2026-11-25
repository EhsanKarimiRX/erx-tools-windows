//! The Tauri layer of Secure Notes: managed state, the command wrappers, the
//! one-second timer thread and the save on exit.
//!
//! Every command first checks the window label. Commands that run Argon2 are
//! asynchronous and do their work on a blocking thread; all other commands are
//! asynchronous too, so they never run on the main UI thread.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

use super::clock::RealClock;
use super::crypto::{CreateSettings, NOTES_KDF_TARGET, create, open, tune_kdf};
use super::error::NotesError;
use super::export::{
    LEGACY_EXPORT_PASSPHRASE, export_payload_all, export_payload_single, import_payload,
    is_encrypted_container, note_to_plain,
};
use super::model::{NOTE_FORMAT_TEXT, Note, NotesData, PreviewMode, new_note_id};
use super::session::{
    LockReport, NoteListItem, NoteView, NotesSession, NotesStatus, RefreshView, SaveState,
    SettingsView, TickOutcome, UnlockOutcome, WordsView,
};
use super::store::NotesStore;
use crate::tools::vault::passphrase::normalize_passphrase;

/// The label of the hardened notes window.
pub const NOTES_WINDOW_LABEL: &str = "notes";
/// The folder under the app data folder that holds the notes files.
pub const NOTES_DIR_NAME: &str = "notes";
/// How often the timer thread looks at the session.
pub const TICK_INTERVAL: Duration = Duration::from_secs(1);
/// Emitted when the session locks itself because of idle time.
pub const EVENT_AUTO_LOCKED: &str = "notes://auto-locked";
/// Emitted when an autosave ends.
pub const EVENT_AUTOSAVE: &str = "notes://autosave";

/// An error of a Secure Notes command, as the frontend sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotesCommandError {
    pub code: String,
    pub message: String,
}

impl NotesCommandError {
    /// Secure Notes is not available (no session could be built).
    pub fn unavailable() -> NotesCommandError {
        NotesCommandError {
            code: "unavailable".to_string(),
            message: "Secure Notes is not available.".to_string(),
        }
    }

    /// A background task failed.
    pub fn internal() -> NotesCommandError {
        NotesCommandError {
            code: "internal".to_string(),
            message: "Internal error in Secure Notes.".to_string(),
        }
    }
}

impl std::fmt::Display for NotesCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl From<NotesError> for NotesCommandError {
    fn from(error: NotesError) -> NotesCommandError {
        let code = match &error {
            NotesError::NotANotesFile => "not_a_notes_file",
            NotesError::UnsupportedVersion => "unsupported_version",
            NotesError::DamagedFile => "damaged_file",
            NotesError::WrongPassphraseOrHeader => "wrong_passphrase_or_header",
            NotesError::InvalidKdfParams => "invalid_kdf_params",
            NotesError::KeyDerivationFailed => "key_derivation_failed",
            NotesError::FileTooLarge => "file_too_large",
            NotesError::NotesTooLarge => "notes_too_large",
            NotesError::RandomUnavailable => "random_unavailable",
            NotesError::Internal => "internal",
            NotesError::DataInvalid => "data_invalid",
            NotesError::DataTooNew => "data_too_new",
            NotesError::NoteTooLarge => "note_too_large",
            NotesError::TitleTooLong => "title_too_long",
            NotesError::TooManyNotes => "too_many_notes",
            NotesError::NoteNotFound => "note_not_found",
            NotesError::Io => "io",
            NotesError::NoNotesFile => "no_notes_file",
            NotesError::AlreadyExists => "already_exists",
            NotesError::VerifyFailed => "verify_failed",
            NotesError::NotUnlocked => "not_unlocked",
            NotesError::AlreadyUnlocked => "already_unlocked",
            NotesError::WordsNotConfirmed => "words_not_confirmed",
            NotesError::WordsNotRetyped => "words_not_retyped",
            NotesError::Words(_) => "words",
        };
        NotesCommandError {
            code: code.to_string(),
            message: error.to_string(),
        }
    }
}

/// The session behind the commands. `None` means Secure Notes is unavailable.
pub struct NotesShared {
    session: Mutex<Option<NotesSession>>,
}

impl NotesShared {
    pub fn new(session: Option<NotesSession>) -> NotesShared {
        NotesShared {
            session: Mutex::new(session),
        }
    }
}

impl Default for NotesShared {
    fn default() -> NotesShared {
        NotesShared::new(None)
    }
}

/// The managed state of Secure Notes.
#[derive(Default)]
pub struct NotesState {
    shared: Arc<NotesShared>,
}

impl NotesState {
    /// The shared session, for the other Secure Notes modules.
    pub(crate) fn shared(&self) -> Arc<NotesShared> {
        Arc::clone(&self.shared)
    }
}

/// Locks the session. A poisoned lock is recovered: the app aborts on a panic
/// in release builds, so this only matters in tests and development.
pub(crate) fn lock_shared(shared: &NotesShared) -> MutexGuard<'_, Option<NotesSession>> {
    shared
        .session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Runs `f` on the session, or fails with "unavailable" when there is none.
pub fn with_session<T>(
    shared: &NotesShared,
    f: impl FnOnce(&mut NotesSession) -> Result<T, NotesError>,
) -> Result<T, NotesCommandError> {
    let mut guard = lock_shared(shared);
    let session = guard.as_mut().ok_or_else(NotesCommandError::unavailable)?;
    f(session).map_err(NotesCommandError::from)
}

/// True only for the window that may use Secure Notes.
pub fn check_window_label(label: &str) -> bool {
    label == NOTES_WINDOW_LABEL
}

/// Rejects every window except the one named by `NOTES_WINDOW_LABEL`.
fn check_window(window: &tauri::Window) -> Result<(), NotesCommandError> {
    if check_window_label(window.label()) {
        Ok(())
    } else {
        Err(NotesCommandError {
            code: "wrong_window".to_string(),
            message: "This window may not use Secure Notes.".to_string(),
        })
    }
}

/// Saves (unless forced) and locks. A failed save leaves the session unlocked.
pub fn lock_impl(shared: &NotesShared, force: bool) -> Result<LockReport, NotesCommandError> {
    with_session(shared, |session| {
        if !force && session.status().unlocked {
            session.save_now()?;
        }
        Ok(session.lock())
    })
}

/// Completes the create flow with `settings`.
pub fn finish_create_impl(
    shared: &NotesShared,
    settings: &CreateSettings,
) -> Result<(), NotesCommandError> {
    with_session(shared, |session| session.finish_create(settings))
}

/// What changing the passphrase did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePassphraseResult {
    pub old_backup_gone: bool,
}

/// Changes the passphrase and reports what happened to the old backup.
pub fn change_passphrase_impl(
    shared: &NotesShared,
    current_typed: &str,
    settings: &CreateSettings,
) -> Result<ChangePassphraseResult, NotesCommandError> {
    with_session(shared, |session| {
        let report = session.change_passphrase(current_typed, settings)?;
        Ok(ChangePassphraseResult {
            old_backup_gone: report.old_backup_gone,
        })
    })
}

/// Tunes the KDF for this machine and builds the production settings. This
/// takes about one second, so it must run outside the session lock.
pub fn production_settings() -> Result<CreateSettings, NotesError> {
    let kdf = tune_kdf(NOTES_KDF_TARGET)?;
    CreateSettings::new(kdf)
}

/// One timer tick. Never blocks: a busy session is skipped.
pub fn tick_once(shared: &NotesShared) -> Option<TickOutcome> {
    let mut guard = match shared.session.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => return None,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
    };
    guard.as_mut().map(|session| session.tick())
}

/// The payload of the autosave event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutosaveEvent {
    pub ok: bool,
}

/// The payload of the auto-locked event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoLockedEvent {
    pub final_save_failed: bool,
}

/// The events a tick produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickEvents {
    pub autosave: Option<AutosaveEvent>,
    pub auto_locked: Option<AutoLockedEvent>,
}

/// Turns one tick outcome into the events the frontend listens for.
pub fn events_for(outcome: &TickOutcome) -> TickEvents {
    let autosave = if outcome.autosaved {
        Some(AutosaveEvent { ok: true })
    } else if outcome.autosave_failed {
        Some(AutosaveEvent { ok: false })
    } else {
        None
    };
    let auto_locked = outcome.locked.map(|report| AutoLockedEvent {
        final_save_failed: report.final_save_failed,
    });
    TickEvents {
        autosave,
        auto_locked,
    }
}

/// Locks the session at exit, blocking until the lock is free.
pub fn exit_save(shared: &NotesShared) -> Option<LockReport> {
    let mut guard = lock_shared(shared);
    guard.as_mut().map(|session| session.lock())
}

/// Builds the session from the app data folder and starts the timer thread.
/// Without the timer there is no auto-lock, so a failure fails closed.
pub fn init(app: &AppHandle) {
    let Ok(app_local_data_dir) = app.path().app_local_data_dir() else {
        return;
    };
    let dir = app_local_data_dir.join(NOTES_DIR_NAME);
    let session = NotesSession::new(NotesStore::new(dir), Box::new(RealClock));
    let shared = Arc::clone(&app.state::<NotesState>().shared);
    *lock_shared(&shared) = Some(session);

    let handle = app.clone();
    let thread_shared = Arc::clone(&shared);
    let spawned = std::thread::Builder::new()
        .name("notes-tick".to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(TICK_INTERVAL);
                if let Some(outcome) = tick_once(&thread_shared) {
                    let events = events_for(&outcome);
                    if let Some(payload) = events.autosave {
                        let _ = handle.emit_to(NOTES_WINDOW_LABEL, EVENT_AUTOSAVE, payload);
                    }
                    if let Some(payload) = events.auto_locked {
                        let _ = handle.emit_to(NOTES_WINDOW_LABEL, EVENT_AUTO_LOCKED, payload);
                    }
                }
            }
        });
    if spawned.is_err() {
        *lock_shared(&shared) = None;
    }
}

/// Saves and locks on exit; the result is ignored.
pub fn on_exit(app: &AppHandle) {
    let _ = exit_save(&app.state::<NotesState>().shared);
}

#[tauri::command(async)]
pub fn notes_status(
    window: tauri::Window,
    state: State<'_, NotesState>,
) -> Result<NotesStatus, NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| Ok(session.status()))
}

#[tauri::command(async)]
pub fn notes_generate_words(
    window: tauri::Window,
    state: State<'_, NotesState>,
    count: usize,
) -> Result<WordsView, NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| session.generate_words(count))
}

#[tauri::command(async)]
pub fn notes_refresh_word(
    window: tauri::Window,
    state: State<'_, NotesState>,
    slot: usize,
) -> Result<RefreshView, NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| session.refresh_word(slot))
}

#[tauri::command(async)]
pub fn notes_confirm_words_saved(
    window: tauri::Window,
    state: State<'_, NotesState>,
) -> Result<(), NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| session.confirm_words_saved())
}

#[tauri::command(async)]
pub fn notes_retype_words(
    window: tauri::Window,
    state: State<'_, NotesState>,
    typed: String,
) -> Result<bool, NotesCommandError> {
    check_window(&window)?;
    let typed = Zeroizing::new(typed);
    with_session(&state.shared, |session| {
        session.retype_words(typed.as_str())
    })
}

#[tauri::command(async)]
pub fn notes_clear_words(
    window: tauri::Window,
    state: State<'_, NotesState>,
) -> Result<(), NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| {
        session.clear_words();
        Ok(())
    })
}

#[tauri::command(async)]
pub fn notes_export_generate_words(
    window: tauri::Window,
    state: State<'_, NotesState>,
    count: usize,
) -> Result<WordsView, NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| {
        session.generate_export_words(count)
    })
}

#[tauri::command(async)]
pub fn notes_export_refresh_word(
    window: tauri::Window,
    state: State<'_, NotesState>,
    slot: usize,
) -> Result<RefreshView, NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| session.refresh_export_word(slot))
}

#[tauri::command(async)]
pub fn notes_export_confirm_words_saved(
    window: tauri::Window,
    state: State<'_, NotesState>,
) -> Result<(), NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| {
        session.confirm_export_words_saved()
    })
}

#[tauri::command(async)]
pub fn notes_export_retype_words(
    window: tauri::Window,
    state: State<'_, NotesState>,
    typed: String,
) -> Result<bool, NotesCommandError> {
    check_window(&window)?;
    let typed = Zeroizing::new(typed);
    with_session(&state.shared, |session| {
        session.retype_export_words(typed.as_str())
    })
}

#[tauri::command(async)]
pub fn notes_export_clear_words(
    window: tauri::Window,
    state: State<'_, NotesState>,
) -> Result<(), NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| {
        session.clear_export_words();
        Ok(())
    })
}

#[tauri::command(async)]
pub fn notes_lock(
    window: tauri::Window,
    state: State<'_, NotesState>,
    force: bool,
) -> Result<LockReport, NotesCommandError> {
    check_window(&window)?;
    lock_impl(&state.shared, force)
}

#[tauri::command(async)]
pub fn notes_activity(
    window: tauri::Window,
    state: State<'_, NotesState>,
) -> Result<(), NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| {
        session.activity();
        Ok(())
    })
}

#[tauri::command(async)]
pub fn notes_save_state(
    window: tauri::Window,
    state: State<'_, NotesState>,
) -> Result<SaveState, NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| Ok(session.save_state()))
}

#[tauri::command(async)]
pub fn notes_save_now(
    window: tauri::Window,
    state: State<'_, NotesState>,
) -> Result<(), NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| {
        session.save_now()?;
        Ok(())
    })
}

#[tauri::command(async)]
pub fn notes_get_settings(
    window: tauri::Window,
    state: State<'_, NotesState>,
) -> Result<SettingsView, NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| session.settings())
}

#[tauri::command(async)]
pub fn notes_set_settings(
    window: tauri::Window,
    state: State<'_, NotesState>,
    preview: PreviewMode,
    auto_lock_minutes: u32,
) -> Result<(), NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| {
        session.set_settings(preview, auto_lock_minutes)
    })
}

#[tauri::command(async)]
pub fn notes_list(
    window: tauri::Window,
    state: State<'_, NotesState>,
    query: String,
) -> Result<Vec<NoteListItem>, NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| session.list(&query))
}

#[tauri::command(async)]
pub fn notes_get_note(
    window: tauri::Window,
    state: State<'_, NotesState>,
    id: String,
) -> Result<NoteView, NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| session.get_note(&id))
}

#[tauri::command(async)]
pub fn notes_create_note(
    window: tauri::Window,
    state: State<'_, NotesState>,
    title: String,
    body: String,
) -> Result<NoteView, NotesCommandError> {
    check_window(&window)?;
    let title = Zeroizing::new(title);
    let body = Zeroizing::new(body);
    with_session(&state.shared, |session| {
        session.create_note(title.as_str(), body.as_str())
    })
}

#[tauri::command(async)]
pub fn notes_update_note(
    window: tauri::Window,
    state: State<'_, NotesState>,
    id: String,
    title: String,
    body: String,
) -> Result<(), NotesCommandError> {
    check_window(&window)?;
    let title = Zeroizing::new(title);
    let body = Zeroizing::new(body);
    with_session(&state.shared, |session| {
        session.update_note(&id, title.as_str(), body.as_str())
    })
}

#[tauri::command(async)]
pub fn notes_delete_note(
    window: tauri::Window,
    state: State<'_, NotesState>,
    id: String,
) -> Result<(), NotesCommandError> {
    check_window(&window)?;
    with_session(&state.shared, |session| session.delete_note(&id))
}

#[tauri::command]
pub async fn notes_finish_create(
    window: tauri::Window,
    state: State<'_, NotesState>,
) -> Result<(), NotesCommandError> {
    check_window(&window)?;
    let shared = Arc::clone(&state.shared);
    tauri::async_runtime::spawn_blocking(move || {
        let settings = production_settings()?;
        finish_create_impl(&shared, &settings)
    })
    .await
    .map_err(|_| NotesCommandError::internal())?
}

#[tauri::command]
pub async fn notes_unlock(
    window: tauri::Window,
    state: State<'_, NotesState>,
    typed: String,
) -> Result<UnlockOutcome, NotesCommandError> {
    check_window(&window)?;
    let typed = Zeroizing::new(typed);
    let shared = Arc::clone(&state.shared);
    tauri::async_runtime::spawn_blocking(move || {
        with_session(&shared, |session| session.unlock(typed.as_str()))
    })
    .await
    .map_err(|_| NotesCommandError::internal())?
}

#[tauri::command]
pub async fn notes_change_passphrase(
    window: tauri::Window,
    state: State<'_, NotesState>,
    current_typed: String,
) -> Result<ChangePassphraseResult, NotesCommandError> {
    check_window(&window)?;
    let current_typed = Zeroizing::new(current_typed);
    let shared = Arc::clone(&state.shared);
    tauri::async_runtime::spawn_blocking(move || {
        let settings = production_settings()?;
        change_passphrase_impl(&shared, current_typed.as_str(), &settings)
    })
    .await
    .map_err(|_| NotesCommandError::internal())?
}

/// A short, safe file name stem taken from a note title.
fn safe_file_stem(title: &str) -> String {
    const MAX_STEM_CHARS: usize = 80;
    let mut stem = String::with_capacity(title.len().min(MAX_STEM_CHARS));
    for ch in title.chars().take(MAX_STEM_CHARS) {
        let clean = if ch.is_control()
            || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        {
            '_'
        } else {
            ch
        };
        stem.push(clean);
    }
    let trimmed = stem.trim().trim_end_matches('.').trim();
    if trimmed.is_empty() {
        return "note".to_string();
    }
    let upper = trimmed.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit());
    if reserved {
        return format!("_{trimmed}");
    }
    trimmed.to_string()
}

/// Adds the extension when the chosen name has none.
fn ensure_extension(path: PathBuf, extension: &str) -> PathBuf {
    if path.extension().is_none() {
        path.with_extension(extension)
    } else {
        path
    }
}

/// Encrypts one export payload with the fixed export passphrase.
// TODO(R3): must stop using the legacy passphrase for writing.
fn encrypt_export(payload: &[u8]) -> Result<Vec<u8>, NotesCommandError> {
    let settings = production_settings()?;
    let normalized = normalize_passphrase(LEGACY_EXPORT_PASSPHRASE);
    let (file, keys) = create(normalized.as_str(), &settings, payload)?;
    drop(keys);
    Ok(file)
}

/// Writes one note as plain text or as an encrypted single-note file.
fn write_note_export(path: &Path, note: &NoteView, format: &str) -> Result<(), NotesCommandError> {
    if format == "erxnote" {
        let payload = export_payload_single(note)?;
        let file = encrypt_export(payload.as_slice())?;
        std::fs::write(path, file.as_slice()).map_err(|_| NotesError::Io)?;
    } else {
        let plain = Zeroizing::new(note_to_plain(note, format)?);
        std::fs::write(path, plain.as_bytes()).map_err(|_| NotesError::Io)?;
    }
    Ok(())
}

fn export_note_blocking(
    app: &AppHandle,
    note: NoteView,
    format: &str,
) -> Result<Option<String>, NotesCommandError> {
    let (label, extension) = match format {
        "txt" => ("Text file", "txt"),
        "md" => ("Markdown file", "md"),
        "erxnote" => ("ERX Secure Notes file", "erxnote"),
        _ => return Err(NotesError::DataInvalid.into()),
    };
    let target = app
        .dialog()
        .file()
        .add_filter(label, &[extension])
        .set_file_name(format!("{}.{extension}", safe_file_stem(&note.title)))
        .blocking_save_file();
    let Some(target) = target else {
        return Ok(None);
    };
    let path = ensure_extension(target.into_path().map_err(|_| NotesError::Io)?, extension);
    if path.as_os_str().is_empty() {
        return Ok(None);
    }
    write_note_export(&path, &note, format)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn export_all_blocking(
    app: &AppHandle,
    data: NotesData,
) -> Result<Option<String>, NotesCommandError> {
    let target = app
        .dialog()
        .file()
        .add_filter("ERX Secure Notes backup", &["erxnote"])
        .set_file_name("secure-notes-backup.erxnote")
        .blocking_save_file();
    let Some(target) = target else {
        return Ok(None);
    };
    let path = ensure_extension(target.into_path().map_err(|_| NotesError::Io)?, "erxnote");
    if path.as_os_str().is_empty() {
        return Ok(None);
    }
    let payload = export_payload_all(&data)?;
    let file = encrypt_export(payload.as_slice())?;
    std::fs::write(&path, file.as_slice()).map_err(|_| NotesError::Io)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn unix_now() -> Result<i64, NotesError> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| NotesError::Internal)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| NotesError::Internal)
}

/// Reads one file and appends its notes to the open session.
fn import_blocking(
    app: &AppHandle,
    shared: &NotesShared,
) -> Result<Option<usize>, NotesCommandError> {
    let target = app
        .dialog()
        .file()
        .add_filter("ERX Secure Notes or text", &["erxnote", "txt", "md"])
        .add_filter("All files", &["*"])
        .blocking_pick_file();
    let Some(target) = target else {
        return Ok(None);
    };
    let path = target.into_path().map_err(|_| NotesError::Io)?;
    let bytes = std::fs::read(&path).map_err(|_| NotesError::Io)?;

    let notes = if is_encrypted_container(&bytes) {
        let normalized = normalize_passphrase(LEGACY_EXPORT_PASSPHRASE);
        let (keys, payload) = open(&bytes, normalized.as_str())?;
        drop(keys);
        let mut staging = NotesData::new_empty();
        import_payload(payload.as_slice(), &mut staging)?;
        std::mem::take(&mut staging.notes)
    } else {
        let body = Zeroizing::new(String::from_utf8(bytes).map_err(|_| NotesError::DataInvalid)?);
        let title = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("Imported note")
            .to_string();
        let created = unix_now()?;
        vec![Note {
            id: new_note_id()?,
            title,
            body: body.to_string(),
            format: NOTE_FORMAT_TEXT.to_string(),
            created,
            modified: created,
        }]
    };

    let count = with_session(shared, |session| session.import_notes(&notes))?;
    Ok(Some(count))
}

/// Saves one note as `txt`, `md` or an encrypted `erxnote` file. Returns the
/// written path, or `None` when the dialog was cancelled.
#[tauri::command]
pub async fn notes_export_note(
    window: tauri::Window,
    app: AppHandle,
    state: State<'_, NotesState>,
    id: String,
    format: String,
) -> Result<Option<String>, NotesCommandError> {
    check_window(&window)?;
    if !matches!(format.as_str(), "txt" | "md" | "erxnote") {
        return Err(NotesError::DataInvalid.into());
    }
    let note = with_session(&state.shared, |session| session.get_note(&id))?;
    tauri::async_runtime::spawn_blocking(move || export_note_blocking(&app, note, &format))
        .await
        .map_err(|_| NotesCommandError::internal())?
}

/// Saves every note as one encrypted `erxnote` backup. Returns the written
/// path, or `None` when the dialog was cancelled.
#[tauri::command]
pub async fn notes_export_all(
    window: tauri::Window,
    app: AppHandle,
    state: State<'_, NotesState>,
) -> Result<Option<String>, NotesCommandError> {
    check_window(&window)?;
    let data = with_session(&state.shared, |session| session.export_data())?;
    tauri::async_runtime::spawn_blocking(move || export_all_blocking(&app, data))
        .await
        .map_err(|_| NotesCommandError::internal())?
}

/// Appends the notes of an `erxnote` backup or a plain `txt` / `md` file.
/// Returns how many notes were added, or `None` when the dialog was cancelled.
#[tauri::command]
pub async fn notes_import(
    window: tauri::Window,
    app: AppHandle,
    state: State<'_, NotesState>,
) -> Result<Option<usize>, NotesCommandError> {
    check_window(&window)?;
    let shared = Arc::clone(&state.shared);
    tauri::async_runtime::spawn_blocking(move || import_blocking(&app, &shared))
        .await
        .map_err(|_| NotesCommandError::internal())?
}
