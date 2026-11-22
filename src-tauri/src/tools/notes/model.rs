//! The Secure Notes data model: settings, notes and the JSON payload.
//!
//! The payload is the plaintext that the container from `format.rs` protects.
//! It is plain UTF-8 JSON; every function here works on memory only.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use super::error::NotesError;
use super::format::MAX_PAYLOAD_BYTES;

/// Payload version written by this code.
pub const PAYLOAD_VERSION: u32 = 1;
/// The only note format this version knows.
pub const NOTE_FORMAT_TEXT: &str = "text";
/// Largest number of notes in one file.
pub const MAX_NOTES: usize = 10_000;
/// Largest note body in UTF-8 bytes (4 MiB).
pub const MAX_NOTE_BODY_BYTES: usize = 4 * 1024 * 1024;
/// Largest note title in UTF-8 bytes.
pub const MAX_TITLE_BYTES: usize = 1024;
/// Length of a note id: 16 random bytes in lower-case hex.
pub const ID_LEN: usize = 32;
/// Auto-lock time used when a file has no other setting.
pub const DEFAULT_AUTO_LOCK_MINUTES: u32 = 10;
/// Shortest allowed auto-lock time.
pub const MIN_AUTO_LOCK_MINUTES: u32 = 1;
/// Longest allowed auto-lock time.
pub const MAX_AUTO_LOCK_MINUTES: u32 = 240;

/// How the note list shows each note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PreviewMode {
    Titles,
    FirstLine,
}

/// User settings stored inside the payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Settings {
    pub preview: PreviewMode,
    pub auto_lock_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            preview: PreviewMode::Titles,
            auto_lock_minutes: DEFAULT_AUTO_LOCK_MINUTES,
        }
    }
}

/// One note. Field order decides the JSON order.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub format: String,
    pub created: i64,
    pub modified: i64,
}

impl std::fmt::Debug for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Note {{ id: {}, .. }}", self.id)
    }
}

impl Note {
    /// Builds a text note; `modified` starts equal to `created`.
    pub fn new(id: String, title: String, body: String, created: i64) -> Note {
        Note {
            id,
            title,
            body,
            format: NOTE_FORMAT_TEXT.to_string(),
            created,
            modified: created,
        }
    }

    /// Clears all text and the timestamps in place.
    pub fn wipe(&mut self) {
        self.id.zeroize();
        self.title.zeroize();
        self.body.zeroize();
        self.created = 0;
        self.modified = 0;
    }
}

impl Drop for Note {
    fn drop(&mut self) {
        self.wipe();
    }
}

/// The whole payload of one notes file.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotesData {
    pub version: u32,
    #[serde(default)]
    pub settings: Settings,
    pub notes: Vec<Note>,
}

impl std::fmt::Debug for NotesData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NotesData {{ notes: {}, .. }}", self.notes.len())
    }
}

impl NotesData {
    /// A payload with the current version, default settings and no notes.
    pub fn new_empty() -> NotesData {
        NotesData {
            version: PAYLOAD_VERSION,
            settings: Settings::default(),
            notes: Vec::new(),
        }
    }

    /// Checks every rule of the payload in the documented order.
    pub fn validate(&self) -> Result<(), NotesError> {
        if self.version != PAYLOAD_VERSION {
            return Err(NotesError::DataInvalid);
        }
        if !(MIN_AUTO_LOCK_MINUTES..=MAX_AUTO_LOCK_MINUTES)
            .contains(&self.settings.auto_lock_minutes)
        {
            return Err(NotesError::DataInvalid);
        }
        if self.notes.len() > MAX_NOTES {
            return Err(NotesError::TooManyNotes);
        }
        let mut seen = HashSet::with_capacity(self.notes.len());
        for note in &self.notes {
            if !is_valid_id(&note.id) {
                return Err(NotesError::DataInvalid);
            }
            if !seen.insert(note.id.as_str()) {
                return Err(NotesError::DataInvalid);
            }
            if note.format != NOTE_FORMAT_TEXT {
                return Err(NotesError::DataInvalid);
            }
            if note.title.len() > MAX_TITLE_BYTES {
                return Err(NotesError::TitleTooLong);
            }
            if note.body.len() > MAX_NOTE_BODY_BYTES {
                return Err(NotesError::NoteTooLarge);
            }
            if note.created < 0 || note.modified < 0 {
                return Err(NotesError::DataInvalid);
            }
        }
        Ok(())
    }

    /// Validates and serializes the payload; the bytes are wiped on drop.
    pub fn to_payload(&self) -> Result<Zeroizing<Vec<u8>>, NotesError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| NotesError::DataInvalid)?;
        if bytes.len() > MAX_PAYLOAD_BYTES {
            return Err(NotesError::NotesTooLarge);
        }
        Ok(Zeroizing::new(bytes))
    }

    /// Parses the version first, then the whole payload, then validates it.
    pub fn from_payload(bytes: &[u8]) -> Result<NotesData, NotesError> {
        #[derive(Deserialize)]
        struct VersionProbe {
            version: u32,
        }

        let probe: VersionProbe =
            serde_json::from_slice(bytes).map_err(|_| NotesError::DataInvalid)?;
        if probe.version == 0 {
            return Err(NotesError::DataInvalid);
        }
        if probe.version > PAYLOAD_VERSION {
            return Err(NotesError::DataTooNew);
        }
        let data: NotesData = serde_json::from_slice(bytes).map_err(|_| NotesError::DataInvalid)?;
        data.validate()?;
        Ok(data)
    }

    /// Position of the note with this id.
    pub fn find_index(&self, id: &str) -> Option<usize> {
        self.notes.iter().position(|note| note.id == id)
    }

    /// Adds a note and returns its index.
    pub fn add_note(&mut self, title: &str, body: &str, created: i64) -> Result<usize, NotesError> {
        if self.notes.len() >= MAX_NOTES {
            return Err(NotesError::TooManyNotes);
        }
        check_note_size(title, body)?;
        if created < 0 {
            return Err(NotesError::DataInvalid);
        }
        for _ in 0..10 {
            let id = new_note_id()?;
            if self.find_index(&id).is_none() {
                let index = self.notes.len();
                self.notes
                    .push(Note::new(id, title.to_string(), body.to_string(), created));
                return Ok(index);
            }
        }
        Err(NotesError::Internal)
    }

    /// Replaces the title and body of an existing note.
    pub fn update_note(
        &mut self,
        id: &str,
        title: &str,
        body: &str,
        modified: i64,
    ) -> Result<(), NotesError> {
        let index = self.find_index(id).ok_or(NotesError::NoteNotFound)?;
        check_note_size(title, body)?;
        if modified < 0 {
            return Err(NotesError::DataInvalid);
        }
        let note = &mut self.notes[index];
        note.title = title.to_string();
        note.body = body.to_string();
        note.modified = modified;
        Ok(())
    }

    /// Removes one note.
    pub fn delete_note(&mut self, id: &str) -> Result<(), NotesError> {
        let index = self.find_index(id).ok_or(NotesError::NoteNotFound)?;
        self.notes.remove(index);
        Ok(())
    }
}

fn is_valid_id(id: &str) -> bool {
    id.len() == ID_LEN
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn check_note_size(title: &str, body: &str) -> Result<(), NotesError> {
    if title.len() > MAX_TITLE_BYTES {
        return Err(NotesError::TitleTooLong);
    }
    if body.len() > MAX_NOTE_BODY_BYTES {
        return Err(NotesError::NoteTooLarge);
    }
    Ok(())
}

/// 16 random bytes as 32 lower-case hex characters.
pub fn new_note_id() -> Result<String, NotesError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0u8; ID_LEN / 2];
    getrandom::fill(&mut bytes).map_err(|_| NotesError::RandomUnavailable)?;
    let mut id = String::with_capacity(ID_LEN);
    for byte in bytes {
        id.push(HEX[(byte >> 4) as usize] as char);
        id.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(id)
}

/// Default title for a new note without one.
pub fn default_title(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> String {
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// One-line preview of a note body, at most `max_chars` characters plus an
/// ellipsis when text was cut.
pub fn preview_line(body: &str, max_chars: usize) -> String {
    for line in body.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut preview: String = trimmed.chars().take(max_chars).collect();
        if trimmed.chars().count() > max_chars {
            preview.push('\u{2026}');
        }
        return preview;
    }
    String::new()
}
