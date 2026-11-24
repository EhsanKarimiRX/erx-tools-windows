//! Export and import tests: the plain text shapes, the two payload shapes
//! and the id handling when notes are appended.

use super::error::NotesError;
use super::export::{
    export_payload_all, export_payload_single, import_payload, is_encrypted_container,
    note_to_plain,
};
use super::model::NotesData;
use super::session::NoteView;

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
