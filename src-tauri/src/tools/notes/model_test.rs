//! Tests for the notes data model and the JSON payload.

use std::collections::HashSet;

use super::error::NotesError;
use super::format::MAX_PAYLOAD_BYTES;
use super::model::*;

const ID_A: &str = "0123456789abcdef0123456789abcdef";

fn note_with_id(id: &str) -> Note {
    Note::new(
        id.to_string(),
        "t".to_string(),
        "b".to_string(),
        1_700_000_000,
    )
}

fn data_with_ids(count: usize) -> NotesData {
    let mut data = NotesData::new_empty();
    data.notes = (0..count)
        .map(|i| {
            Note::new(
                format!("{i:032x}"),
                format!("title {i}"),
                "body".to_string(),
                1_700_000_000,
            )
        })
        .collect();
    data
}

#[test]
fn d1_constants_and_defaults() {
    assert_eq!(PAYLOAD_VERSION, 1);
    assert_eq!(NOTE_FORMAT_TEXT, "text");
    assert_eq!(MAX_NOTES, 10_000);
    assert_eq!(MAX_NOTE_BODY_BYTES, 4 * 1024 * 1024);
    assert_eq!(MAX_TITLE_BYTES, 1024);
    assert_eq!(ID_LEN, 32);
    assert_eq!(DEFAULT_AUTO_LOCK_MINUTES, 10);
    assert_eq!(MIN_AUTO_LOCK_MINUTES, 1);
    assert_eq!(MAX_AUTO_LOCK_MINUTES, 240);

    let settings = Settings::default();
    assert_eq!(settings.preview, PreviewMode::Titles);
    assert_eq!(settings.auto_lock_minutes, 10);

    let data = NotesData::new_empty();
    assert_eq!(data.version, 1);
    assert!(data.notes.is_empty());
}

#[test]
fn d2_json_shape_is_exact() {
    let mut note = Note::new(
        ID_A.to_string(),
        "t".to_string(),
        "b".to_string(),
        1_700_000_000,
    );
    note.modified = 1_700_000_001;
    let mut data = NotesData::new_empty();
    data.notes.push(note);

    let payload = data.to_payload().expect("valid payload");
    let text = String::from_utf8(payload.to_vec()).expect("UTF-8");
    assert_eq!(
        text,
        "{\"version\":1,\"settings\":{\"preview\":\"titles\",\"autoLockMinutes\":10},\"notes\":[{\"id\":\"0123456789abcdef0123456789abcdef\",\"title\":\"t\",\"body\":\"b\",\"format\":\"text\",\"created\":1700000000,\"modified\":1700000001}]}"
    );
}

#[test]
fn d3_round_trip_keeps_all_text_and_settings() {
    let text = "\u{0645}\u{06CC}\u{200C}\u{062E}\u{0648}\u{0627}\u{0647}\u{0645} \u{06F1}\u{06F2}\u{06F3} Latin \"quoted\" \\back\nline\r\nnext \u{1F600}";
    let mut data = NotesData::new_empty();
    data.settings.preview = PreviewMode::FirstLine;
    data.settings.auto_lock_minutes = 45;
    data.notes.push(Note::new(
        ID_A.to_string(),
        text.to_string(),
        text.to_string(),
        0,
    ));

    let payload = data.to_payload().expect("valid payload");
    let back = NotesData::from_payload(&payload).expect("valid payload");
    assert_eq!(back.version, data.version);
    assert_eq!(back.settings, data.settings);
    assert_eq!(back.notes.len(), 1);
    assert_eq!(back.notes[0].id, data.notes[0].id);
    assert_eq!(back.notes[0].title, data.notes[0].title);
    assert_eq!(back.notes[0].body, data.notes[0].body);
    assert_eq!(back.notes[0].format, data.notes[0].format);
    assert_eq!(back.notes[0].created, data.notes[0].created);
    assert_eq!(back.notes[0].modified, data.notes[0].modified);
    assert_eq!(back, data);
}

#[test]
fn d4_from_payload_rejects_bad_input() {
    assert_eq!(
        NotesData::from_payload(b"not json").unwrap_err(),
        NotesError::DataInvalid
    );
    assert_eq!(
        NotesData::from_payload(b"{}").unwrap_err(),
        NotesError::DataInvalid
    );
    assert_eq!(
        NotesData::from_payload(b"{\"version\":0,\"settings\":{},\"notes\":[]}").unwrap_err(),
        NotesError::DataInvalid
    );
    assert_eq!(
        NotesData::from_payload(
            b"{\"version\":2,\"settings\":{\"preview\":\"future-mode\"},\"notes\":[]}"
        )
        .unwrap_err(),
        NotesError::DataTooNew
    );
    assert_eq!(
        NotesData::from_payload(b"{\"version\":1,\"settings\":{\"preview\":\"x\"},\"notes\":[]}")
            .unwrap_err(),
        NotesError::DataInvalid
    );
    assert_eq!(
        NotesData::from_payload(
            b"{\"version\":1,\"settings\":{\"preview\":\"titles\",\"autoLockMinutes\":10},\"notes\":[{\"id\":\"0123456789abcdef0123456789abcdef\",\"title\":\"t\",\"body\":\"b\",\"format\":\"text\",\"created\":1,\"modified\":1,\"pinned\":true}]}"
        )
        .unwrap_err(),
        NotesError::DataInvalid
    );
    assert_eq!(
        NotesData::from_payload(
            b"{\"version\":1,\"settings\":{\"preview\":\"titles\",\"autoLockMinutes\":10},\"notes\":[],\"extra\":true}"
        )
        .unwrap_err(),
        NotesError::DataInvalid
    );

    let data = NotesData::from_payload(b"{\"version\":1,\"notes\":[]}").expect("defaults apply");
    assert_eq!(data.settings, Settings::default());
}

#[test]
fn d5_validate_errors_and_boundaries() {
    let mut data = NotesData::new_empty();
    data.notes.push(note_with_id(ID_A));
    data.validate().expect("valid note");

    let mut short = data.clone();
    short.notes[0].id = "0123456789abcdef0123456789abcde".to_string();
    assert_eq!(short.validate().unwrap_err(), NotesError::DataInvalid);

    let mut upper = data.clone();
    upper.notes[0].id = "0123456789ABCDEF0123456789abcdef".to_string();
    assert_eq!(upper.validate().unwrap_err(), NotesError::DataInvalid);

    let mut non_hex = data.clone();
    non_hex.notes[0].id = "0123456789abcdef0123456789abcdeg".to_string();
    assert_eq!(non_hex.validate().unwrap_err(), NotesError::DataInvalid);

    let mut duplicate = data.clone();
    duplicate.notes.push(note_with_id(ID_A));
    assert_eq!(duplicate.validate().unwrap_err(), NotesError::DataInvalid);

    let mut wrong_format = data.clone();
    wrong_format.notes[0].format = "markdown".to_string();
    assert_eq!(
        wrong_format.validate().unwrap_err(),
        NotesError::DataInvalid
    );

    let mut long_title = data.clone();
    long_title.notes[0].title = "a".repeat(MAX_TITLE_BYTES + 1);
    assert_eq!(long_title.validate().unwrap_err(), NotesError::TitleTooLong);
    long_title.notes[0].title = "a".repeat(MAX_TITLE_BYTES);
    long_title.validate().expect("title at the limit");

    let mut long_body = data.clone();
    long_body.notes[0].body = "b".repeat(MAX_NOTE_BODY_BYTES + 1);
    assert_eq!(long_body.validate().unwrap_err(), NotesError::NoteTooLarge);
    long_body.notes[0].body = "b".repeat(MAX_NOTE_BODY_BYTES);
    long_body.validate().expect("body at the limit");

    data_with_ids(MAX_NOTES)
        .validate()
        .expect("notes at the limit");
    assert_eq!(
        data_with_ids(MAX_NOTES + 1).validate().unwrap_err(),
        NotesError::TooManyNotes
    );

    let mut lock = NotesData::new_empty();
    lock.settings.auto_lock_minutes = 0;
    assert_eq!(lock.validate().unwrap_err(), NotesError::DataInvalid);
    lock.settings.auto_lock_minutes = 241;
    assert_eq!(lock.validate().unwrap_err(), NotesError::DataInvalid);
    lock.settings.auto_lock_minutes = 1;
    lock.validate().expect("minimum auto lock");
    lock.settings.auto_lock_minutes = 240;
    lock.validate().expect("maximum auto lock");

    let mut bad_time = data.clone();
    bad_time.notes[0].created = -1;
    assert_eq!(bad_time.validate().unwrap_err(), NotesError::DataInvalid);

    let mut bad_version = data.clone();
    bad_version.version = 2;
    assert_eq!(bad_version.validate().unwrap_err(), NotesError::DataInvalid);
}

#[test]
fn d6_to_payload_refuses_invalid_or_oversized_data() {
    let mut bad = NotesData::new_empty();
    bad.notes.push(note_with_id("not-a-valid-id"));
    assert_eq!(bad.to_payload().unwrap_err(), NotesError::DataInvalid);

    let mut huge = data_with_ids(17);
    for note in &mut huge.notes {
        note.body = "b".repeat(MAX_NOTE_BODY_BYTES);
    }
    assert_eq!(huge.to_payload().unwrap_err(), NotesError::NotesTooLarge);
    const { assert!(MAX_PAYLOAD_BYTES < 17 * MAX_NOTE_BODY_BYTES) };
}

#[test]
fn d7_add_note() {
    let mut data = NotesData::new_empty();
    let mut ids = HashSet::new();
    for i in 0..100 {
        let index = data.add_note("title", "body", 1_700_000_000).expect("add");
        assert_eq!(index, i);
        let note = &data.notes[index];
        assert_eq!(note.id.len(), ID_LEN);
        assert!(
            note.id
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert!(ids.insert(note.id.clone()));
        assert_eq!(note.created, 1_700_000_000);
        assert_eq!(note.modified, note.created);
        assert_eq!(note.format, NOTE_FORMAT_TEXT);
    }

    let mut full = data_with_ids(MAX_NOTES);
    assert_eq!(
        full.add_note("t", "b", 0).unwrap_err(),
        NotesError::TooManyNotes
    );
    assert_eq!(full.notes.len(), MAX_NOTES);

    let mut data = NotesData::new_empty();
    assert_eq!(
        data.add_note(&"a".repeat(MAX_TITLE_BYTES + 1), "b", 0)
            .unwrap_err(),
        NotesError::TitleTooLong
    );
    assert_eq!(
        data.add_note("t", &"b".repeat(MAX_NOTE_BODY_BYTES + 1), 0)
            .unwrap_err(),
        NotesError::NoteTooLarge
    );
    assert_eq!(
        data.add_note("t", "b", -5).unwrap_err(),
        NotesError::DataInvalid
    );
    assert!(data.notes.is_empty());
}

#[test]
fn d8_update_delete_and_find() {
    let mut data = data_with_ids(3);
    let before = data.notes[1].clone();

    data.update_note(&before.id, "new title", "new body", 1_800_000_000)
        .expect("update");
    let after = &data.notes[1];
    assert_eq!(after.title, "new title");
    assert_eq!(after.body, "new body");
    assert_eq!(after.modified, 1_800_000_000);
    assert_eq!(after.id, before.id);
    assert_eq!(after.created, before.created);
    assert_eq!(after.format, before.format);

    assert_eq!(
        data.update_note("ffffffffffffffffffffffffffffffff", "t", "b", 0)
            .unwrap_err(),
        NotesError::NoteNotFound
    );
    let unchanged = data.notes[1].clone();
    assert_eq!(
        data.update_note(&before.id, "t", &"b".repeat(MAX_NOTE_BODY_BYTES + 1), 0)
            .unwrap_err(),
        NotesError::NoteTooLarge
    );
    assert_eq!(data.notes[1], unchanged);

    let remove_id = data.notes[1].id.clone();
    let expected: Vec<String> = data
        .notes
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 1)
        .map(|(_, n)| n.id.clone())
        .collect();
    data.delete_note(&remove_id).expect("delete");
    let actual: Vec<String> = data.notes.iter().map(|n| n.id.clone()).collect();
    assert_eq!(actual, expected);
    assert_eq!(
        data.delete_note(&remove_id).unwrap_err(),
        NotesError::NoteNotFound
    );

    assert_eq!(data.find_index(&expected[1]), Some(1));
    assert_eq!(data.find_index("ffffffffffffffffffffffffffffffff"), None);
}

#[test]
fn d9_wipe_clears_everything() {
    let mut note = Note::new(ID_A.to_string(), "title".to_string(), "body".to_string(), 5);
    note.modified = 6;
    note.wipe();
    assert!(note.id.is_empty());
    assert!(note.title.is_empty());
    assert!(note.body.is_empty());
    assert_eq!(note.created, 0);
    assert_eq!(note.modified, 0);
}

#[test]
fn d10_debug_redacts_note_text() {
    let note = Note::new(
        ID_A.to_string(),
        "hidden-title-xyz".to_string(),
        "hidden-body-xyz".to_string(),
        0,
    );
    let debug = format!("{:?}", note);
    assert!(debug.contains(ID_A));
    assert!(!debug.contains("hidden-title-xyz"));
    assert!(!debug.contains("hidden-body-xyz"));

    let data = data_with_ids(2);
    assert_eq!(format!("{:?}", data), "NotesData { notes: 2, .. }");
}

#[test]
fn d11_default_title() {
    assert_eq!(default_title(2026, 11, 23, 14, 5), "2026-11-23 14:05");
    assert_eq!(default_title(2026, 1, 2, 3, 4), "2026-01-02 03:04");
}

#[test]
fn d12_preview_line() {
    assert_eq!(preview_line("first line\nsecond", 100), "first line");
    assert_eq!(preview_line("\n\r\n   \nfirst", 100), "first");
    assert_eq!(preview_line("  spaced  \r\nnext", 100), "spaced");
    assert_eq!(preview_line("", 100), "");
    assert_eq!(preview_line("   \n \n", 100), "");

    assert_eq!(preview_line("abcdefghij", 5), "abcde\u{2026}");
    assert_eq!(preview_line("abcdefghij", 10), "abcdefghij");

    let persian = preview_line(&"\u{0645}".repeat(10), 4);
    assert_eq!(persian.chars().count(), 5);
    assert_eq!(persian, format!("{}\u{2026}", "\u{0645}".repeat(4)));

    let emoji = preview_line("ab\u{1F600}cd", 2);
    assert_eq!(emoji, "ab\u{2026}");
    assert_eq!(emoji.chars().count(), 3);
    let emoji_kept = preview_line("ab\u{1F600}cd", 3);
    assert_eq!(emoji_kept, "ab\u{1F600}\u{2026}");
}
