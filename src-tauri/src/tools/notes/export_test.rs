//! Export and import tests: the plain text shapes, the two payload shapes
//! and the id handling when notes are appended.

use super::crypto::{CreateSettings, create, open};
use super::error::NotesError;
use super::export::{
    ExportKind, ImportOutcome, LEGACY_EXPORT_PASSPHRASE, export_bytes, export_payload_all,
    export_payload_single, import_payload, is_encrypted_container, note_to_plain,
    open_export_candidates, open_export_typed,
};
use super::format::FLAG_USER_PASSPHRASE;
use super::model::{NOTE_FORMAT_TEXT, Note, NotesData, new_note_id};
use super::session::NoteView;
use crate::tools::vault::passphrase::normalize_passphrase;

fn note_view(id: &str, title: &str, body: &str) -> NoteView {
    NoteView {
        id: id.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        created: 1_700_000_000,
        modified: 1_700_000_100,
    }
}

const VALID_ID: &str = "0123456789abcdef0123456789abcdef";
const OTHER_ID: &str = "fedcba9876543210fedcba9876543210";

#[test]
fn e1_note_to_plain_txt() {
    let note = note_view(VALID_ID, "Title", "Body");
    assert_eq!(note_to_plain(&note, "txt").unwrap(), "Title\n\nBody");
}

#[test]
fn e2_note_to_plain_md() {
    let note = note_view(VALID_ID, "Title", "Body");
    assert_eq!(note_to_plain(&note, "md").unwrap(), "# Title\n\nBody");
}

#[test]
fn e3_note_to_plain_invalid_format() {
    let note = note_view(VALID_ID, "Title", "Body");
    assert_eq!(
        note_to_plain(&note, "pdf").unwrap_err(),
        NotesError::DataInvalid
    );
}

#[test]
fn e4_export_payload_all_roundtrips() {
    let mut source = NotesData::new_empty();
    source.add_note("first", "body one", 10).unwrap();
    source.add_note("second", "body two", 20).unwrap();

    let bytes = export_payload_all(&source).unwrap();
    let mut target = NotesData::new_empty();
    assert_eq!(import_payload(&bytes, &mut target).unwrap(), 2);
    assert_eq!(target.notes.len(), 2);
    for index in 0..2 {
        assert_eq!(target.notes[index].id, source.notes[index].id);
        assert_eq!(target.notes[index].title, source.notes[index].title);
        assert_eq!(target.notes[index].body, source.notes[index].body);
        assert_eq!(target.notes[index].created, source.notes[index].created);
        assert_eq!(target.notes[index].modified, source.notes[index].modified);
    }
}

#[test]
fn e5_export_payload_single_roundtrips() {
    let note = note_view(VALID_ID, "Only", "The body");
    let bytes = export_payload_single(&note).unwrap();

    let mut target = NotesData::new_empty();
    assert_eq!(import_payload(&bytes, &mut target).unwrap(), 1);
    assert_eq!(target.notes.len(), 1);
    assert_eq!(target.notes[0].id, VALID_ID);
    assert_eq!(target.notes[0].title, "Only");
    assert_eq!(target.notes[0].body, "The body");
    assert_eq!(target.notes[0].created, 1_700_000_000);
    assert_eq!(target.notes[0].modified, 1_700_000_100);
}

#[test]
fn e6_import_regenerates_duplicate_id() {
    let mut target = NotesData::new_empty();
    target.add_note("first", "body one", 10).unwrap();
    let existing_id = target.notes[0].id.clone();

    let note = note_view(&existing_id, "second", "body two");
    let bytes = export_payload_single(&note).unwrap();
    assert_eq!(import_payload(&bytes, &mut target).unwrap(), 1);

    assert_eq!(target.notes.len(), 2);
    assert_ne!(target.notes[1].id, existing_id);
    assert_eq!(target.notes[1].id.len(), 32);
    assert_eq!(target.notes[1].title, "second");
    assert_eq!(target.notes[1].body, "body two");
    assert_eq!(target.notes[1].created, 1_700_000_000);
    assert_eq!(target.notes[1].modified, 1_700_000_100);
}

#[test]
fn e7_is_encrypted_container() {
    assert!(is_encrypted_container(b"ERXNOTES"));
    assert!(is_encrypted_container(b"ERXNOTES and more"));
    assert!(!is_encrypted_container(b"plain text"));
    assert!(!is_encrypted_container(b"ERXNOTE"));
    assert!(!is_encrypted_container(b""));
}

#[test]
fn e8_import_single_note_with_other_id_keeps_it() {
    let mut target = NotesData::new_empty();
    target.add_note("first", "body one", 10).unwrap();
    assert_ne!(target.notes[0].id, OTHER_ID);

    let note = note_view(OTHER_ID, "second", "body two");
    let bytes = export_payload_single(&note).unwrap();
    assert_eq!(import_payload(&bytes, &mut target).unwrap(), 1);
    assert_eq!(target.notes[1].id, OTHER_ID);
}

const R1_WORDS: &str = "alpha bravo charlie delta echo foxtrot";
const R1_OTHER: &str = "golf hotel india juliet kilo lima";

fn r1_data() -> NotesData {
    let mut data = NotesData::new_empty();
    data.notes.push(Note {
        id: new_note_id().unwrap(),
        title: "t".to_string(),
        body: "b".to_string(),
        format: NOTE_FORMAT_TEXT.to_string(),
        created: 1,
        modified: 1,
    });
    data
}

fn r1_new_export() -> Vec<u8> {
    let data = r1_data();
    export_bytes(
        ExportKind::All(&data),
        R1_WORDS,
        &CreateSettings::for_tests(),
    )
    .unwrap()
}

fn r1_legacy_file() -> Vec<u8> {
    let payload = export_payload_all(&r1_data()).unwrap();
    let words = normalize_passphrase(LEGACY_EXPORT_PASSPHRASE);
    let (file, _keys) = create(
        words.as_str(),
        &CreateSettings::for_tests(),
        payload.as_slice(),
    )
    .unwrap();
    file
}

#[test]
fn r1_user_passphrase_round_trip() {
    let file = r1_new_export();
    assert_eq!(&file[10..12], &[1u8, 0u8]);
    match open_export_candidates(&file, &[R1_WORDS]).unwrap() {
        ImportOutcome::Opened(notes) => assert_eq!(notes.len(), 1),
        other => panic!("unexpected outcome: {other:?}"),
    }
    match open_export_typed(&file, R1_WORDS).unwrap() {
        ImportOutcome::Opened(notes) => assert_eq!(notes.len(), 1),
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn r1_wrong_passphrase_opens_nothing() {
    let file = r1_new_export();
    assert!(matches!(
        open_export_candidates(&file, &[R1_OTHER]).unwrap(),
        ImportOutcome::NeedsPassphrase
    ));
    assert!(matches!(
        open_export_typed(&file, R1_OTHER).unwrap(),
        ImportOutcome::WrongPassphrase
    ));
}

#[test]
fn r1_legacy_file_opens_via_legacy_constant() {
    let file = r1_legacy_file();
    assert_eq!(&file[10..12], &[0u8, 0u8]);
    match open_export_candidates(&file, &[]).unwrap() {
        ImportOutcome::LegacyOpened(notes) => assert_eq!(notes.len(), 1),
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn r1_new_export_does_not_open_with_legacy_constant() {
    let file = r1_new_export();
    let legacy = normalize_passphrase(LEGACY_EXPORT_PASSPHRASE);
    assert!(open(&file, legacy.as_str()).is_err());
    assert!(matches!(
        open_export_candidates(&file, &[]).unwrap(),
        ImportOutcome::NeedsPassphrase
    ));
    assert!(matches!(
        open_export_typed(&file, legacy.as_str()).unwrap(),
        ImportOutcome::WrongPassphrase
    ));
}

#[test]
fn r1_flipping_the_flag_never_opens_a_file() {
    // New export with the flag cleared: commitment fails for every passphrase.
    let mut cleared = r1_new_export();
    cleared[10] = 0;
    assert!(matches!(
        open_export_candidates(&cleared, &[R1_WORDS]).unwrap(),
        ImportOutcome::NeedsPassphrase
    ));
    // Legacy file with the flag set: legacy words must not open it either.
    let mut flagged = r1_legacy_file();
    flagged[10] = 1;
    assert!(matches!(
        open_export_candidates(&flagged, &[]).unwrap(),
        ImportOutcome::NeedsPassphrase
    ));
    let legacy = normalize_passphrase(LEGACY_EXPORT_PASSPHRASE);
    assert!(matches!(
        open_export_typed(&flagged, legacy.as_str()).unwrap(),
        ImportOutcome::WrongPassphrase
    ));
}

#[test]
fn r1_unknown_flag_and_short_files_are_errors() {
    let mut file = r1_new_export();
    file[10] = 2;
    assert_eq!(
        open_export_candidates(&file, &[R1_WORDS]).unwrap_err(),
        NotesError::UnsupportedVersion
    );
    assert_eq!(
        open_export_candidates(&file[..100], &[]).unwrap_err(),
        NotesError::DamagedFile
    );
}
