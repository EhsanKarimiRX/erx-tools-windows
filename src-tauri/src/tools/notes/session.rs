//! The Secure Notes session: the lock state, the words flows, the note
//! operations, autosave and the idle lock. No UI and no Tauri in this file.

use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use super::clock::{Clock, utc_parts};
use super::crypto::{CreateSettings, NotesKeys};
use super::error::NotesError;
use super::model::{
    MAX_AUTO_LOCK_MINUTES, MIN_AUTO_LOCK_MINUTES, Note, NotesData, PreviewMode, Settings,
    default_title, preview_line,
};
use super::search::search_titles;
use super::store::{LoadSource, NotesStore, SaveReport};
use crate::tools::vault::passphrase::{
    NOTES_WORD_LIMITS, PassphraseError, PassphraseSession, normalize_passphrase, strength,
    validate_typed_words_with,
};

/// How long after the last edit an autosave happens.
pub const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);
/// How long to wait before retrying a failed autosave.
pub const AUTOSAVE_RETRY: Duration = Duration::from_secs(5);
/// Longest note preview shown in the list.
pub const PREVIEW_MAX_CHARS: usize = 120;

/// Whether a notes file exists and whether it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesStatus {
    pub has_file: bool,
    pub unlocked: bool,
}

/// The words of a new passphrase for display, with their strength.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordsView {
    pub words: Vec<String>,
    pub replacements_left: Vec<u8>,
    pub entropy_bits: f64,
    pub weak: bool,
}

/// One replaced word for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshView {
    pub word: String,
    pub replacements_left: u8,
}

/// How an unlock ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockOutcome {
    pub used_backup: bool,
}

/// One row of the note list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteListItem {
    pub id: String,
    pub title: String,
    pub preview: Option<String>,
    pub created: i64,
    pub modified: i64,
}

/// One note with its text. The manual `Debug` and `Drop` keep the text out
/// of logs and wipe it when it is no longer needed.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteView {
    pub id: String,
    pub title: String,
    pub body: String,
    pub created: i64,
    pub modified: i64,
}

impl fmt::Debug for NoteView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "NoteView {{ id: {}, .. }}", self.id)
    }
}

impl Drop for NoteView {
    fn drop(&mut self) {
        self.title.zeroize();
        self.body.zeroize();
    }
}

/// The settings shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub preview: PreviewMode,
    pub auto_lock_minutes: u32,
}

/// What locking did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockReport {
    pub was_unlocked: bool,
    pub final_save_failed: bool,
}

/// What one timer tick did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickOutcome {
    pub autosaved: bool,
    pub autosave_failed: bool,
    pub locked: Option<LockReport>,
}

/// How the open notes compare with the file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SaveState {
    Saved,
    Pending,
    Failed,
}

impl TickOutcome {
    fn nothing() -> TickOutcome {
        TickOutcome {
            autosaved: false,
            autosave_failed: false,
            locked: None,
        }
    }
}

/// The open notes and their timers. Never printed.
struct Unlocked {
    keys: NotesKeys,
    data: NotesData,
    main_is_good: bool,
    dirty: bool,
    save_failed: bool,
    last_activity: Instant,
    last_edit: Option<Instant>,
    last_failed_attempt: Option<Instant>,
}

/// The Secure Notes session. The manual `Debug` never shows the notes.
pub struct NotesSession {
    store: NotesStore,
    clock: Box<dyn Clock>,
    words: PassphraseSession,
    retyped: bool,
    export_words: PassphraseSession,
    export_retyped: bool,
    unlocked: Option<Unlocked>,
}

impl fmt::Debug for NotesSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "NotesSession {{ unlocked: {}, .. }}",
            self.unlocked.is_some()
        )
    }
}

impl NotesSession {
    pub fn new(store: NotesStore, clock: Box<dyn Clock>) -> NotesSession {
        NotesSession {
            store,
            clock,
            words: PassphraseSession::new(NOTES_WORD_LIMITS),
            retyped: false,
            export_words: PassphraseSession::new(NOTES_WORD_LIMITS),
            export_retyped: false,
            unlocked: None,
        }
    }

    pub fn status(&self) -> NotesStatus {
        NotesStatus {
            has_file: self.store.exists(),
            unlocked: self.unlocked.is_some(),
        }
    }

    pub fn generate_words(&mut self, count: usize) -> Result<WordsView, NotesError> {
        if self.unlocked.is_none() && self.store.exists() {
            return Err(NotesError::NotUnlocked);
        }
        let words = self
            .words
            .generate(count)
            .map_err(|error| NotesError::Words(error.to_string()))?;
        self.retyped = false;
        let replacements_left = (0..words.len())
            .map(|slot| self.words.replacements_left(slot).unwrap_or(0))
            .collect();
        let figures = strength(words.len());
        Ok(WordsView {
            words,
            replacements_left,
            entropy_bits: figures.bits,
            weak: figures.is_weak,
        })
    }

    pub fn refresh_word(&mut self, slot: usize) -> Result<RefreshView, NotesError> {
        if self.unlocked.is_none() && self.store.exists() {
            return Err(NotesError::NotUnlocked);
        }
        let word = self
            .words
            .refresh(slot)
            .map_err(|error| NotesError::Words(error.to_string()))?;
        let replacements_left = self.words.replacements_left(slot).unwrap_or(0);
        Ok(RefreshView {
            word,
            replacements_left,
        })
    }

    pub fn confirm_words_saved(&mut self) -> Result<(), NotesError> {
        self.words
            .confirm_saved()
            .map_err(|error| NotesError::Words(error.to_string()))
    }

    pub fn retype_words(&mut self, typed: &str) -> Result<bool, NotesError> {
        match self.words.typed_matches(typed) {
            Ok(true) => {
                self.retyped = true;
                Ok(true)
            }
            Ok(false) => Ok(false),
            Err(PassphraseError::NotConfirmed) => Err(NotesError::WordsNotConfirmed),
            Err(error) => Err(NotesError::Words(error.to_string())),
        }
    }

    pub fn clear_words(&mut self) {
        self.words.clear();
        self.retyped = false;
    }

    /// The words as one text (single spaces) for copying. Fails when there are no words. The caller must wipe the text; the pieces are wiped here.
    pub fn words_text(&self) -> Result<Zeroizing<String>, NotesError> {
        let mut parts = self.words.display_words();
        if parts.is_empty() {
            return Err(PassphraseError::NoPassphrase)
                .map_err(|error| NotesError::Words(error.to_string()));
        }
        let joined = parts.join(" ");
        for part in parts.iter_mut() {
            part.zeroize();
        }
        Ok(Zeroizing::new(joined))
    }

    // ---- Export words: a separate session (never shares `self.words`). ----

    pub fn generate_export_words(&mut self, count: usize) -> Result<WordsView, NotesError> {
        if self.unlocked.is_none() {
            return Err(NotesError::NotUnlocked);
        }
        let words = self
            .export_words
            .generate(count)
            .map_err(|error| NotesError::Words(error.to_string()))?;
        self.export_retyped = false;
        let replacements_left = (0..words.len())
            .map(|slot| self.export_words.replacements_left(slot).unwrap_or(0))
            .collect();
        let figures = strength(words.len());
        Ok(WordsView {
            words,
            replacements_left,
            entropy_bits: figures.bits,
            weak: figures.is_weak,
        })
    }

    pub fn refresh_export_word(&mut self, slot: usize) -> Result<RefreshView, NotesError> {
        if self.unlocked.is_none() {
            return Err(NotesError::NotUnlocked);
        }
        let word = self
            .export_words
            .refresh(slot)
            .map_err(|error| NotesError::Words(error.to_string()))?;
        let replacements_left = self.export_words.replacements_left(slot).unwrap_or(0);
        Ok(RefreshView {
            word,
            replacements_left,
        })
    }

    pub fn confirm_export_words_saved(&mut self) -> Result<(), NotesError> {
        self.export_words
            .confirm_saved()
            .map_err(|error| NotesError::Words(error.to_string()))
    }

    pub fn retype_export_words(&mut self, typed: &str) -> Result<bool, NotesError> {
        match self.export_words.typed_matches(typed) {
            Ok(true) => {
                self.export_retyped = true;
                Ok(true)
            }
            Ok(false) => Ok(false),
            Err(PassphraseError::NotConfirmed) => Err(NotesError::WordsNotConfirmed),
            Err(error) => Err(NotesError::Words(error.to_string())),
        }
    }

    pub fn clear_export_words(&mut self) {
        self.export_words.clear();
        self.export_retyped = false;
    }

    /// The export words as one text (single spaces) for copying. The caller must wipe the text; the pieces are wiped here.
    pub fn export_words_for_copy(&self) -> Result<Zeroizing<String>, NotesError> {
        let mut parts = self.export_words.display_words();
        if parts.is_empty() {
            return Err(NotesError::Words(PassphraseError::NoPassphrase.to_string()));
        }
        let joined = parts.join(" ");
        for part in parts.iter_mut() {
            part.zeroize();
        }
        Ok(Zeroizing::new(joined))
    }

    /// The normalized export words for the key derivation. Refused until the
    /// words were confirmed saved AND retyped correctly.
    #[allow(dead_code)] // used from R3
    pub fn export_words_for_kdf(&self) -> Result<Zeroizing<String>, NotesError> {
        let words = match self.export_words.normalized_for_kdf() {
            Ok(words) => words,
            Err(PassphraseError::NotConfirmed) => return Err(NotesError::WordsNotConfirmed),
            Err(error) => return Err(NotesError::Words(error.to_string())),
        };
        if !self.export_retyped {
            return Err(NotesError::WordsNotRetyped);
        }
        Ok(words)
    }

    pub fn finish_create(&mut self, settings: &CreateSettings) -> Result<(), NotesError> {
        if self.unlocked.is_some() {
            return Err(NotesError::AlreadyUnlocked);
        }
        if self.store.exists() {
            return Err(NotesError::AlreadyExists);
        }
        let normalized = match self.words.normalized_for_kdf() {
            Ok(words) => words,
            Err(PassphraseError::NotConfirmed) => return Err(NotesError::WordsNotConfirmed),
            Err(error) => return Err(NotesError::Words(error.to_string())),
        };
        if !self.retyped {
            return Err(NotesError::WordsNotRetyped);
        }
        let data = NotesData::new_empty();
        let keys = self.store.create_new(&normalized, settings, &data)?;
        let now = self.clock.now();
        self.unlocked = Some(Unlocked {
            keys,
            data,
            main_is_good: true,
            dirty: false,
            save_failed: false,
            last_activity: now,
            last_edit: None,
            last_failed_attempt: None,
        });
        self.words.clear();
        self.retyped = false;
        Ok(())
    }

    pub fn unlock(&mut self, typed: &str) -> Result<UnlockOutcome, NotesError> {
        if self.unlocked.is_some() {
            return Err(NotesError::AlreadyUnlocked);
        }
        if !self.store.exists() {
            return Err(NotesError::NoNotesFile);
        }
        let normalized = normalize_passphrase(typed);
        validate_typed_words_with(normalized.as_str(), NOTES_WORD_LIMITS)
            .map_err(|error| NotesError::Words(error.to_string()))?;
        let loaded = self.store.load(normalized.as_str())?;
        let used_backup = loaded.source == LoadSource::Backup;
        let now = self.clock.now();
        self.unlocked = Some(Unlocked {
            keys: loaded.keys,
            data: loaded.data,
            main_is_good: !used_backup,
            dirty: false,
            save_failed: false,
            last_activity: now,
            last_edit: None,
            last_failed_attempt: None,
        });
        Ok(UnlockOutcome { used_backup })
    }

    pub fn lock(&mut self) -> LockReport {
        self.export_words.clear();
        self.export_retyped = false;
        let Some(unlocked) = self.unlocked.take() else {
            return LockReport {
                was_unlocked: false,
                final_save_failed: false,
            };
        };
        let mut final_save_failed = false;
        if unlocked.dirty
            && self
                .store
                .save(&unlocked.keys, &unlocked.data, unlocked.main_is_good)
                .is_err()
        {
            final_save_failed = true;
        }
        drop(unlocked);
        self.words.clear();
        self.retyped = false;
        LockReport {
            was_unlocked: true,
            final_save_failed,
        }
    }

    pub fn activity(&mut self) {
        let now = self.clock.now();
        if let Some(unlocked) = self.unlocked.as_mut() {
            unlocked.last_activity = now;
        }
    }

    pub fn tick(&mut self) -> TickOutcome {
        let Some(unlocked) = self.unlocked.as_ref() else {
            return TickOutcome::nothing();
        };
        let now = self.clock.now();
        let idle_limit =
            Duration::from_secs(u64::from(unlocked.data.settings.auto_lock_minutes) * 60);
        let idle = now.duration_since(unlocked.last_activity) >= idle_limit;
        if idle {
            let report = self.lock();
            return TickOutcome {
                autosaved: false,
                autosave_failed: false,
                locked: Some(report),
            };
        }
        let last_edit = unlocked.last_edit.unwrap_or(unlocked.last_activity);
        let autosave_due = unlocked.dirty
            && now.duration_since(last_edit) >= AUTOSAVE_DELAY
            && unlocked
                .last_failed_attempt
                .is_none_or(|failed| now.duration_since(failed) >= AUTOSAVE_RETRY);
        if !autosave_due {
            return TickOutcome::nothing();
        }
        match self.save_open(now) {
            Ok(_) => TickOutcome {
                autosaved: true,
                autosave_failed: false,
                locked: None,
            },
            Err(_) => TickOutcome {
                autosaved: false,
                autosave_failed: true,
                locked: None,
            },
        }
    }

    pub fn save_state(&self) -> SaveState {
        match self.unlocked.as_ref() {
            Some(unlocked) if unlocked.dirty => {
                if unlocked.save_failed {
                    SaveState::Failed
                } else {
                    SaveState::Pending
                }
            }
            _ => SaveState::Saved,
        }
    }

    pub fn save_now(&mut self) -> Result<SaveReport, NotesError> {
        let dirty = self.unlocked.as_ref().ok_or(NotesError::NotUnlocked)?.dirty;
        if !dirty {
            return Ok(SaveReport {
                old_backup_gone: true,
            });
        }
        let now = self.clock.now();
        self.save_open(now)
    }

    pub fn settings(&mut self) -> Result<SettingsView, NotesError> {
        let now = self.clock.now();
        let unlocked = self.unlocked.as_mut().ok_or(NotesError::NotUnlocked)?;
        unlocked.last_activity = now;
        Ok(SettingsView {
            preview: unlocked.data.settings.preview,
            auto_lock_minutes: unlocked.data.settings.auto_lock_minutes,
        })
    }

    pub fn set_settings(
        &mut self,
        preview: PreviewMode,
        auto_lock_minutes: u32,
    ) -> Result<(), NotesError> {
        let now = self.clock.now();
        let unlocked = self.unlocked.as_mut().ok_or(NotesError::NotUnlocked)?;
        if !(MIN_AUTO_LOCK_MINUTES..=MAX_AUTO_LOCK_MINUTES).contains(&auto_lock_minutes) {
            return Err(NotesError::DataInvalid);
        }
        unlocked.data.settings = Settings {
            preview,
            auto_lock_minutes,
        };
        unlocked.dirty = true;
        unlocked.save_failed = false;
        unlocked.last_edit = Some(now);
        unlocked.last_activity = now;
        Ok(())
    }

    pub fn list(&mut self, query: &str) -> Result<Vec<NoteListItem>, NotesError> {
        let now = self.clock.now();
        let unlocked = self.unlocked.as_mut().ok_or(NotesError::NotUnlocked)?;
        unlocked.last_activity = now;
        let show_preview = unlocked.data.settings.preview == PreviewMode::FirstLine;
        let mut indices = search_titles(&unlocked.data.notes, query);
        indices.sort_by(|&left, &right| {
            let left = &unlocked.data.notes[left];
            let right = &unlocked.data.notes[right];
            right
                .modified
                .cmp(&left.modified)
                .then_with(|| right.created.cmp(&left.created))
        });
        Ok(indices
            .into_iter()
            .map(|index| {
                let note = &unlocked.data.notes[index];
                NoteListItem {
                    id: note.id.clone(),
                    title: note.title.clone(),
                    preview: show_preview.then(|| preview_line(&note.body, PREVIEW_MAX_CHARS)),
                    created: note.created,
                    modified: note.modified,
                }
            })
            .collect())
    }

    pub fn get_note(&mut self, id: &str) -> Result<NoteView, NotesError> {
        let now = self.clock.now();
        let unlocked = self.unlocked.as_mut().ok_or(NotesError::NotUnlocked)?;
        unlocked.last_activity = now;
        let index = unlocked
            .data
            .find_index(id)
            .ok_or(NotesError::NoteNotFound)?;
        let note = &unlocked.data.notes[index];
        Ok(NoteView {
            id: note.id.clone(),
            title: note.title.clone(),
            body: note.body.clone(),
            created: note.created,
            modified: note.modified,
        })
    }

    /// A copy of the open notes for a full export. Fails when locked.
    pub fn export_data(&self) -> Result<NotesData, NotesError> {
        let unlocked = self.unlocked.as_ref().ok_or(NotesError::NotUnlocked)?;
        Ok(unlocked.data.clone())
    }

    /// Appends notes to the open notes, giving colliding ids a new value.
    /// Returns how many notes were added.
    pub fn import_notes(&mut self, imported: &[Note]) -> Result<usize, NotesError> {
        let now = self.clock.now();
        let unlocked = self.unlocked.as_mut().ok_or(NotesError::NotUnlocked)?;
        let count = super::export::append_notes(&mut unlocked.data, imported.to_vec())?;
        if count > 0 {
            unlocked.dirty = true;
            unlocked.save_failed = false;
            unlocked.last_edit = Some(now);
            unlocked.last_activity = now;
        }
        Ok(count)
    }

    pub fn create_note(&mut self, title: &str, body: &str) -> Result<NoteView, NotesError> {
        let now = self.clock.now();
        let created = self.clock.unix();
        let title = self.effective_title(title, created);
        let unlocked = self.unlocked.as_mut().ok_or(NotesError::NotUnlocked)?;
        let index = unlocked.data.add_note(&title, body, created)?;
        unlocked.dirty = true;
        unlocked.save_failed = false;
        unlocked.last_edit = Some(now);
        unlocked.last_activity = now;
        let note = &unlocked.data.notes[index];
        Ok(NoteView {
            id: note.id.clone(),
            title: note.title.clone(),
            body: note.body.clone(),
            created: note.created,
            modified: note.modified,
        })
    }

    pub fn update_note(&mut self, id: &str, title: &str, body: &str) -> Result<(), NotesError> {
        let now = self.clock.now();
        let modified = self.clock.unix();
        let created = {
            let unlocked = self.unlocked.as_ref().ok_or(NotesError::NotUnlocked)?;
            unlocked
                .data
                .notes
                .iter()
                .find(|note| note.id == id)
                .map(|note| note.created)
                .ok_or(NotesError::NoteNotFound)?
        };
        let title = self.effective_title(title, created);
        let unlocked = self.unlocked.as_mut().ok_or(NotesError::NotUnlocked)?;
        unlocked.data.update_note(id, &title, body, modified)?;
        unlocked.dirty = true;
        unlocked.save_failed = false;
        unlocked.last_edit = Some(now);
        unlocked.last_activity = now;
        Ok(())
    }

    pub fn delete_note(&mut self, id: &str) -> Result<(), NotesError> {
        let now = self.clock.now();
        let unlocked = self.unlocked.as_mut().ok_or(NotesError::NotUnlocked)?;
        unlocked.data.delete_note(id)?;
        unlocked.dirty = true;
        unlocked.save_failed = false;
        unlocked.last_edit = Some(now);
        unlocked.last_activity = now;
        Ok(())
    }

    pub fn change_passphrase(
        &mut self,
        current_typed: &str,
        settings: &CreateSettings,
    ) -> Result<SaveReport, NotesError> {
        if self.unlocked.is_none() {
            return Err(NotesError::NotUnlocked);
        }
        let new_words = match self.words.normalized_for_kdf() {
            Ok(words) => words,
            Err(PassphraseError::NotConfirmed) => return Err(NotesError::WordsNotConfirmed),
            Err(error) => return Err(NotesError::Words(error.to_string())),
        };
        if !self.retyped {
            return Err(NotesError::WordsNotRetyped);
        }

        let current = normalize_passphrase(current_typed);
        validate_typed_words_with(current.as_str(), NOTES_WORD_LIMITS)
            .map_err(|error| NotesError::Words(error.to_string()))?;
        // Drop the loaded notes at once: they are only the proof that the
        // current words open the file.
        drop(self.store.load(current.as_str())?);

        let (keys, report) = {
            let unlocked = self.unlocked.as_ref().ok_or(NotesError::NotUnlocked)?;
            self.store
                .change_passphrase(&new_words, settings, &unlocked.data)?
        };
        let unlocked = self.unlocked.as_mut().ok_or(NotesError::NotUnlocked)?;
        unlocked.keys = keys;
        unlocked.main_is_good = true;
        unlocked.dirty = false;
        unlocked.save_failed = false;
        unlocked.last_failed_attempt = None;
        self.words.clear();
        self.retyped = false;
        Ok(report)
    }

    /// Saves the open notes and updates the save timers.
    fn save_open(&mut self, now: Instant) -> Result<SaveReport, NotesError> {
        let unlocked = self.unlocked.as_ref().ok_or(NotesError::NotUnlocked)?;
        let saved = self
            .store
            .save(&unlocked.keys, &unlocked.data, unlocked.main_is_good);
        let unlocked = self.unlocked.as_mut().ok_or(NotesError::NotUnlocked)?;
        match saved {
            Ok(report) => {
                unlocked.dirty = false;
                unlocked.main_is_good = true;
                unlocked.save_failed = false;
                unlocked.last_failed_attempt = None;
                Ok(report)
            }
            Err(error) => {
                unlocked.save_failed = true;
                unlocked.last_failed_attempt = Some(now);
                Err(error)
            }
        }
    }

    fn effective_title(&self, title: &str, created: i64) -> String {
        if !title.trim().is_empty() {
            return title.to_string();
        }
        let parts = match self.clock.local_parts(created) {
            Some(parts) => parts,
            None => utc_parts(created),
        };
        default_title(parts.year, parts.month, parts.day, parts.hour, parts.minute)
    }
}
