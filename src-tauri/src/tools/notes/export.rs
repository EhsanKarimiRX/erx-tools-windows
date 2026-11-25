//! Export and import of notes: plain text for one note, the encrypted
//! `ERXNOTES` container for backups, and the shapes that restore accepts.
//! No UI and no Tauri in this file.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::crypto::{CreateSettings, create_export, open};
use super::error::NotesError;
use super::format::{FLAG_USER_PASSPHRASE, HEADER_LEN, Header, MAGIC, MAX_PAYLOAD_BYTES};
use super::model::{
    ID_LEN, MAX_NOTE_BODY_BYTES, MAX_NOTES, MAX_TITLE_BYTES, NOTE_FORMAT_TEXT, Note, NotesData,
    new_note_id,
};
use super::session::NoteView;
use crate::tools::vault::passphrase::normalize_passphrase;

/// The fixed passphrase that v0.5.0 used for every `.erxnote` export.
/// ONLY for opening files exported by v0.5.0. It must never be used to
/// write a new export: such files are effectively unprotected.
pub const LEGACY_EXPORT_PASSPHRASE: &str = "erx-notes-export-default";

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

/// What an export contains.
#[allow(dead_code)] // used from R3
pub enum ExportKind<'a> {
    Single(&'a NoteView),
    All(&'a NotesData),
}

/// Builds one encrypted export file (fresh randomness, FLAG_USER_PASSPHRASE
/// set). `normalized_words` must already be normalized by the caller.
#[allow(dead_code)] // used from R3
pub fn export_bytes(
    kind: ExportKind<'_>,
    normalized_words: &str,
    settings: &CreateSettings,
) -> Result<Vec<u8>, NotesError> {
    let payload = match kind {
        ExportKind::Single(note) => export_payload_single(note)?,
        ExportKind::All(data) => export_payload_all(data)?,
    };
    let (file, keys) = create_export(normalized_words, settings, payload.as_slice())?;
    drop(keys);
    Ok(file)
}

/// The words and the key-derivation settings for one encrypted export.
#[allow(dead_code)] // used from R3b
pub struct ExportCrypto<'a> {
    pub words: &'a str,
    pub settings: &'a CreateSettings,
}

/// Builds the bytes of one exported note. `txt` and `md` are plain and must
/// come with `crypto == None`; `erxnote` is encrypted and needs `Some`.
/// Anything else is `DataInvalid`.
#[allow(dead_code)] // used from R3b
pub fn render_note_export(
    note: &NoteView,
    format: &str,
    crypto: Option<&ExportCrypto<'_>>,
) -> Result<Zeroizing<Vec<u8>>, NotesError> {
    match (format, crypto) {
        ("txt" | "md", None) => Ok(Zeroizing::new(note_to_plain(note, format)?.into_bytes())),
        ("erxnote", Some(crypto)) => {
            let file = export_bytes(ExportKind::Single(note), crypto.words, crypto.settings)?;
            Ok(Zeroizing::new(file))
        }
        _ => Err(NotesError::DataInvalid),
    }
}

/// Builds the bytes of an encrypted backup of all notes.
#[allow(dead_code)] // used from R3b
pub fn render_all_export(
    data: &NotesData,
    crypto: &ExportCrypto<'_>,
) -> Result<Vec<u8>, NotesError> {
    export_bytes(ExportKind::All(data), crypto.words, crypto.settings)
}

/// Result of trying to open an encrypted export. No variant carries
/// plaintext except the two that opened.
#[derive(Debug)]
#[allow(dead_code)] // used from R4
pub enum ImportOutcome {
    /// Opened with one of the candidate passphrases.
    Opened(Vec<Note>),
    /// Opened with the built-in v0.5.0 passphrase.
    LegacyOpened(Vec<Note>),
    /// Nothing worked yet: ask the user for the words.
    NeedsPassphrase,
    /// The typed words did not open the file.
    WrongPassphrase,
}

fn parse_header(bytes: &[u8]) -> Result<Header, NotesError> {
    if bytes.len() < HEADER_LEN {
        return Err(NotesError::DamagedFile);
    }
    Header::parse(&bytes[..HEADER_LEN])
}

fn decode_notes(payload: &[u8]) -> Result<Vec<Note>, NotesError> {
    let mut staging = NotesData::new_empty();
    import_payload(payload, &mut staging)?;
    Ok(std::mem::take(&mut staging.notes))
}

/// Ok(Some) = opened, Ok(None) = wrong words, Err = anything else.
fn try_open(bytes: &[u8], normalized_words: &str) -> Result<Option<Vec<Note>>, NotesError> {
    match open(bytes, normalized_words) {
        Ok((keys, payload)) => {
            drop(keys);
            Ok(Some(decode_notes(payload.as_slice())?))
        }
        Err(NotesError::WrongPassphraseOrHeader) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Tries each already-normalized candidate in order, then the legacy
/// passphrase. The legacy attempt is skipped when the header carries
/// FLAG_USER_PASSPHRASE: the flag is only a hint (a tampered flag breaks the
/// commitment, it can never make a wrong passphrase succeed).
#[allow(dead_code)] // used from R4
pub fn open_export_candidates(
    bytes: &[u8],
    candidates: &[&str],
) -> Result<ImportOutcome, NotesError> {
    let header = parse_header(bytes)?;
    for words in candidates {
        if let Some(notes) = try_open(bytes, words)? {
            return Ok(ImportOutcome::Opened(notes));
        }
    }
    if header.flags & FLAG_USER_PASSPHRASE == 0 {
        let legacy = normalize_passphrase(LEGACY_EXPORT_PASSPHRASE);
        if let Some(notes) = try_open(bytes, legacy.as_str())? {
            return Ok(ImportOutcome::LegacyOpened(notes));
        }
    }
    Ok(ImportOutcome::NeedsPassphrase)
}

/// Opens with words the user typed (already normalized). Never tries the
/// legacy passphrase.
#[allow(dead_code)] // used from R4
pub fn open_export_typed(
    bytes: &[u8],
    normalized_words: &str,
) -> Result<ImportOutcome, NotesError> {
    parse_header(bytes)?;
    Ok(match try_open(bytes, normalized_words)? {
        Some(notes) => ImportOutcome::Opened(notes),
        None => ImportOutcome::WrongPassphrase,
    })
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

/// Writes `bytes` to `path` atomically: a temporary file next to the target
/// is written, synced and read back, then renamed over the target. The
/// temporary file is removed on every failure.
#[allow(dead_code)] // used from R3b
pub fn write_export_file(path: &Path, bytes: &[u8]) -> Result<(), NotesError> {
    let temp = export_temp_path(path)?;
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err(NotesError::Io);
    }
    match std::fs::read(&temp) {
        Ok(read_back) if read_back == bytes => {}
        _ => {
            let _ = std::fs::remove_file(&temp);
            return Err(NotesError::Io);
        }
    }
    if std::fs::rename(&temp, path).is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err(NotesError::Io);
    }
    Ok(())
}

/// A temporary name next to the target: `<name>.<8 hex>.erxtmp`.
fn export_temp_path(path: &Path) -> Result<PathBuf, NotesError> {
    let name = path.file_name().ok_or(NotesError::Io)?;
    let mut random = [0u8; 4];
    getrandom::fill(&mut random).map_err(|_| NotesError::RandomUnavailable)?;
    let hex: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut temp_name = name.to_os_string();
    temp_name.push(format!(".{hex}.erxtmp"));
    Ok(path.with_file_name(temp_name))
}
