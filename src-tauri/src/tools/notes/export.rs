//! Export and import of notes: plain text for one note, the encrypted
//! `ERXNOTES` container for backups, and the shapes that restore accepts.
//! No UI and no Tauri in this file.

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::error::NotesError;
use super::format::{MAGIC, MAX_PAYLOAD_BYTES};
use super::model::{
    ID_LEN, MAX_NOTE_BODY_BYTES, MAX_NOTES, MAX_TITLE_BYTES, NOTE_FORMAT_TEXT, Note, NotesData,
    new_note_id,
};
use super::session::NoteView;

/// The fixed passphrase of every encrypted export. It keeps the file
/// protected from casual reading, not from someone who has this code.
pub const EXPORT_DEFAULT_PASSPHRASE: &str = "erx-notes-export-default";

/// The payload of a single-note encrypted export.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SingleNoteExport {
    note: NoteView,
}

/// The same shape, borrowing the note while it is serialized.
#[derive(Serialize)]
struct SingleNoteExportRef<'a> {
    note: &'a NoteView,
}

/// One note as plain text: `txt` has the title on its own line, `md` a
/// level-one heading.
pub fn note_to_plain(note: &NoteView, format: &str) -> Result<String, NotesError> {
    match format {
        "txt" => Ok(format!("{}\n\n{}", note.title, note.body)),
        "md" => Ok(format!("# {}\n\n{}", note.title, note.body)),
        _ => Err(NotesError::DataInvalid),
    }
}

/// The whole payload of a full backup, including settings.
pub fn export_payload_all(data: &NotesData) -> Result<Zeroizing<Vec<u8>>, NotesError> {
    data.to_payload()
}

/// The tagged payload of a single-note export.
pub fn export_payload_single(note: &NoteView) -> Result<Zeroizing<Vec<u8>>, NotesError> {
    let bytes =
        serde_json::to_vec(&SingleNoteExportRef { note }).map_err(|_| NotesError::DataInvalid)?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(NotesError::NotesTooLarge);
    }
    Ok(Zeroizing::new(bytes))
}

/// Appends every note of a full-backup or single-note payload to `target`.
/// Returns how many notes were added.
pub fn import_payload(bytes: &[u8], target: &mut NotesData) -> Result<usize, NotesError> {
    match NotesData::from_payload(bytes) {
        Ok(mut data) => append_notes(target, std::mem::take(&mut data.notes)),
        Err(NotesError::DataTooNew) => Err(NotesError::DataTooNew),
        Err(_) => {
            let single: SingleNoteExport =
                serde_json::from_slice(bytes).map_err(|_| NotesError::DataInvalid)?;
            let mut view = single.note;
            let note = Note {
                id: std::mem::take(&mut view.id),
                title: std::mem::take(&mut view.title),
                body: std::mem::take(&mut view.body),
                format: NOTE_FORMAT_TEXT.to_string(),
                created: view.created,
                modified: view.modified,
            };
            append_notes(target, vec![note])
        }
    }
}

/// True when the bytes start with the `ERXNOTES` magic.
pub fn is_encrypted_container(bytes: &[u8]) -> bool {
    bytes.starts_with(&MAGIC)
}

/// Appends notes to `target`, giving a colliding id a fresh value and
/// keeping the original timestamps. Nothing is appended when any note is
/// invalid or the target would become too full.
pub(crate) fn append_notes(target: &mut NotesData, notes: Vec<Note>) -> Result<usize, NotesError> {
    let room = MAX_NOTES.saturating_sub(target.notes.len());
    if notes.len() > room {
        return Err(NotesError::TooManyNotes);
    }
    for note in &notes {
        validate_note(note)?;
    }

    let count = notes.len();
    for mut note in notes {
        if target.find_index(&note.id).is_some() {
            let mut fresh = None;
            for _ in 0..10 {
                let candidate = new_note_id()?;
                if target.find_index(&candidate).is_none() {
                    fresh = Some(candidate);
                    break;
                }
            }
            note.id = fresh.ok_or(NotesError::Internal)?;
        }
        target.notes.push(note);
    }
    Ok(count)
}

fn validate_note(note: &Note) -> Result<(), NotesError> {
    if !is_valid_id(&note.id) || note.format != NOTE_FORMAT_TEXT {
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
    Ok(())
}

fn is_valid_id(id: &str) -> bool {
    id.len() == ID_LEN
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
