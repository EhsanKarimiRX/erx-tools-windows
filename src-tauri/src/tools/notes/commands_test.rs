//! Tests of the Tauri layer: error mapping, the window check, the managed
//! state, the logic functions and the wiring in `lib.rs`. No Tauri app is
//! started and every test uses its own temporary folder.

use std::sync::Arc;
use std::time::Duration;

use super::clock::{DateParts, TestClock};
use super::commands::{
    AutoLockedEvent, AutosaveEvent, NotesCommandError, NotesShared, TickEvents, check_window_label,
    events_for, exit_save, finish_create_impl, lock_impl, lock_shared, tick_once, with_session,
};
use super::crypto::CreateSettings;
use super::error::NotesError;
use super::session::{LockReport, NotesSession, TickOutcome, UnlockOutcome};
use super::store::NotesStore;
use super::test_support::{TempDir, real_store, scripted_store};

fn settings() -> CreateSettings {
    CreateSettings::for_tests()
}

fn test_clock() -> Arc<TestClock> {
    TestClock::new(
        1_700_000_000,
        Some(DateParts {
            year: 2023,
            month: 11,
            day: 14,
            hour: 22,
            minute: 13,
        }),
    )
}

/// A shared state with a fresh session on a temporary folder, plus its clock.
fn fresh_shared() -> (TempDir, Arc<NotesShared>, Arc<TestClock>) {
    let guard = TempDir::new();
    let clock = test_clock();
    let session = NotesSession::new(real_store(&guard.path), Box::new(Arc::clone(&clock)));
    (guard, Arc::new(NotesShared::new(Some(session))), clock)
}

/// Runs the create flow on `shared` and returns the words as one string.
fn create_unlocked(shared: &NotesShared) -> String {
    let view = with_session(shared, |session| session.generate_words(5)).unwrap();
    with_session(shared, |session| session.confirm_words_saved()).unwrap();
    let joined = view.words.join(" ");
    assert!(with_session(shared, |session| session.retype_words(&joined)).unwrap());
    finish_create_impl(shared, &settings()).unwrap();
    joined
}

fn status_unlocked(shared: &NotesShared) -> bool {
    with_session(shared, |session| Ok(session.status()))
        .unwrap()
        .unlocked
}

#[test]
fn e1_error_mapping() {
    let cases = [
        (NotesError::NotUnlocked, "not_unlocked"),
        (
            NotesError::WrongPassphraseOrHeader,
            "wrong_passphrase_or_header",
        ),
        (NotesError::DataTooNew, "data_too_new"),
        (NotesError::Io, "io"),
        (NotesError::Words("x".to_string()), "words"),
    ];
    for (error, code) in cases {
        let mapped = NotesCommandError::from(error.clone());
        assert_eq!(mapped.code, code);
        assert_eq!(mapped.message, error.to_string());
    }

    let mapped = NotesCommandError::from(NotesError::NotUnlocked);
    assert_eq!(
        serde_json::to_string(&mapped).unwrap(),
        "{\"code\":\"not_unlocked\",\"message\":\"The notes are locked.\"}"
    );
}

#[test]
fn e2_all_codes() {
    let errors = [
        NotesError::NotANotesFile,
        NotesError::UnsupportedVersion,
        NotesError::DamagedFile,
        NotesError::WrongPassphraseOrHeader,
        NotesError::InvalidKdfParams,
        NotesError::KeyDerivationFailed,
        NotesError::FileTooLarge,
        NotesError::NotesTooLarge,
        NotesError::RandomUnavailable,
        NotesError::Internal,
        NotesError::DataInvalid,
        NotesError::DataTooNew,
        NotesError::NoteTooLarge,
        NotesError::TitleTooLong,
        NotesError::TooManyNotes,
        NotesError::NoteNotFound,
        NotesError::Io,
        NotesError::NoNotesFile,
        NotesError::AlreadyExists,
        NotesError::VerifyFailed,
        NotesError::NotUnlocked,
        NotesError::AlreadyUnlocked,
        NotesError::WordsNotConfirmed,
        NotesError::WordsNotRetyped,
        NotesError::WrongImportPassphrase,
        NotesError::NoPendingImport,
        NotesError::Words("x".to_string()),
    ];
    let mut codes: Vec<String> = errors
        .into_iter()
        .map(|error| NotesCommandError::from(error).code)
        .collect();
    codes.push(NotesCommandError::unavailable().code);
    codes.push(NotesCommandError::internal().code);
    codes.push(
        NotesCommandError {
            code: "wrong_window".to_string(),
            message: "This window may not use Secure Notes.".to_string(),
        }
        .code,
    );

    for code in &codes {
        assert!(!code.is_empty());
        assert!(
            code.chars()
                .all(|character| character.is_ascii_lowercase() || character == '_')
        );
    }
    let mut sorted = codes.clone();
    sorted.sort();
    sorted.dedup();
    // `NotesError::Internal` and `NotesCommandError::internal()` share the
    // code "internal" by design; every other code is distinct.
    assert_eq!(codes.len() - sorted.len(), 1);
    assert_eq!(codes.iter().filter(|code| *code == "internal").count(), 2);
}

#[test]
fn w1_window_label() {
    assert!(check_window_label("notes"));
    for label in ["main", "", "Notes", "notes ", "other"] {
        assert!(!check_window_label(label));
    }
}

#[test]
fn w2_source_scan_commands() {
    let source = include_str!("commands.rs");
    assert_eq!(source.matches("#[tauri::command").count(), 28);
    assert_eq!(source.matches("check_window(&window)").count(), 28);
}

#[test]
fn r3_source_scan_export() {
    let source = include_str!("commands.rs");
    assert_eq!(source.matches("encrypt_export").count(), 0);
    assert_eq!(source.matches("write_note_export").count(), 0);
    assert_eq!(
        source
            .matches("normalize_passphrase(LEGACY_EXPORT_PASSPHRASE)")
            .count(),
        1
    );
    assert_eq!(source.matches("write_export_file(&path").count(), 2);
    assert_eq!(source.matches("render_all_export(").count(), 1);
    assert_eq!(source.matches("resolve_export_words(").count(), 2);
    assert_eq!(source.matches("std::fs::write(").count(), 0);
}

#[test]
fn w3_source_scan_lib() {
    let source = include_str!("../../lib.rs");
    let names = [
        "notes_status",
        "notes_generate_words",
        "notes_refresh_word",
        "notes_confirm_words_saved",
        "notes_retype_words",
        "notes_clear_words",
        "notes_lock",
        "notes_activity",
        "notes_save_state",
        "notes_save_now",
        "notes_get_settings",
        "notes_set_settings",
        "notes_list",
        "notes_get_note",
        "notes_create_note",
        "notes_update_note",
        "notes_delete_note",
        "notes_finish_create",
        "notes_unlock",
        "notes_change_passphrase",
        "notes_export_generate_words",
        "notes_export_refresh_word",
        "notes_export_confirm_words_saved",
        "notes_export_retype_words",
        "notes_export_clear_words",
        "notes_export_note",
        "notes_export_all",
        "notes_import",
    ];
    for name in names {
        let needle = format!("tools::notes::commands::{name},");
        assert_eq!(source.matches(needle.as_str()).count(), 1);
    }
}

#[test]
fn a1_unavailable() {
    let shared = NotesShared::new(None);
    let error = with_session(&shared, |session| Ok(session.status())).unwrap_err();
    assert_eq!(error.code, "unavailable");
    assert_eq!(error.message, "Secure Notes is not available.");

    assert_eq!(lock_impl(&shared, false).unwrap_err().code, "unavailable");
    assert_eq!(lock_impl(&shared, true).unwrap_err().code, "unavailable");
    assert!(tick_once(&shared).is_none());
    assert!(exit_save(&shared).is_none());
}

#[test]
fn a2_flow_on_disk() {
    let (_guard, shared, _clock) = fresh_shared();
    let view = with_session(&shared, |session| session.generate_words(5)).unwrap();
    with_session(&shared, |session| session.confirm_words_saved()).unwrap();
    let words = view.words.join(" ");
    assert!(with_session(&shared, |session| session.retype_words(&words)).unwrap());
    finish_create_impl(&shared, &settings()).unwrap();
    assert!(status_unlocked(&shared));

    let note = with_session(&shared, |session| session.create_note("t", "b")).unwrap();
    assert_eq!(note.title, "t");

    let report = lock_impl(&shared, false).unwrap();
    assert_eq!(
        report,
        LockReport {
            was_unlocked: true,
            final_save_failed: false,
        }
    );
    assert!(!status_unlocked(&shared));

    let outcome = with_session(&shared, |session| session.unlock(&words)).unwrap();
    assert_eq!(outcome, UnlockOutcome { used_backup: false });
    let list = with_session(&shared, |session| session.list("")).unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn a3_lock_with_failing_save() {
    let guard = TempDir::new();
    // The first save (the create flow) succeeds; every later rename fails.
    let store = scripted_store(&guard.path, (2..=100).collect(), false);
    let shared = Arc::new(NotesShared::new(Some(NotesSession::new(
        store,
        Box::new(test_clock()),
    ))));
    let _words = create_unlocked(&shared);
    with_session(&shared, |session| session.create_note("t", "b")).unwrap();

    let error = lock_impl(&shared, false).unwrap_err();
    assert_eq!(error.code, "io");
    assert!(status_unlocked(&shared));

    let report = lock_impl(&shared, true).unwrap();
    assert_eq!(
        report,
        LockReport {
            was_unlocked: true,
            final_save_failed: true,
        }
    );
    assert!(!status_unlocked(&shared));
}

#[test]
fn a4_lock_of_locked_session() {
    let (_guard, shared, _clock) = fresh_shared();
    assert_eq!(
        lock_impl(&shared, false).unwrap(),
        LockReport {
            was_unlocked: false,
            final_save_failed: false,
        }
    );
    assert_eq!(
        lock_impl(&shared, true).unwrap(),
        LockReport {
            was_unlocked: false,
            final_save_failed: false,
        }
    );
}

#[test]
fn a5_tick_and_events() {
    let (_guard, shared, clock) = fresh_shared();
    let _words = create_unlocked(&shared);
    with_session(&shared, |session| session.create_note("t", "b")).unwrap();

    clock.advance(Duration::from_secs(3));
    let outcome = tick_once(&shared).expect("a tick");
    assert!(outcome.autosaved);
    assert!(!outcome.autosave_failed);
    assert!(outcome.locked.is_none());

    let minutes = with_session(&shared, |session| session.settings())
        .unwrap()
        .auto_lock_minutes;
    clock.advance(Duration::from_secs(u64::from(minutes + 1) * 60));
    let outcome = tick_once(&shared).expect("a tick");
    assert!(!outcome.autosaved);
    assert!(!outcome.autosave_failed);
    let report = outcome.locked.expect("a lock");
    assert!(report.was_unlocked);
    assert!(!report.final_save_failed);

    let autosaved = TickOutcome {
        autosaved: true,
        autosave_failed: false,
        locked: None,
    };
    let events = events_for(&autosaved);
    assert_eq!(events.autosave, Some(AutosaveEvent { ok: true }));
    assert_eq!(events.auto_locked, None);

    let failed = TickOutcome {
        autosaved: false,
        autosave_failed: true,
        locked: None,
    };
    assert_eq!(
        events_for(&failed).autosave,
        Some(AutosaveEvent { ok: false })
    );

    let locked = TickOutcome {
        autosaved: false,
        autosave_failed: false,
        locked: Some(LockReport {
            was_unlocked: true,
            final_save_failed: true,
        }),
    };
    assert_eq!(
        events_for(&locked).auto_locked,
        Some(AutoLockedEvent {
            final_save_failed: true
        })
    );

    let nothing = TickOutcome {
        autosaved: false,
        autosave_failed: false,
        locked: None,
    };
    assert_eq!(
        events_for(&nothing),
        TickEvents {
            autosave: None,
            auto_locked: None,
        }
    );
}

#[test]
fn a6_tick_never_blocks() {
    let (_guard, shared, _clock) = fresh_shared();
    let guard = lock_shared(&shared);
    assert!(tick_once(&shared).is_none());
    drop(guard);
    assert!(tick_once(&shared).is_some());
}

#[test]
fn a7_poison_recovery() {
    let (_guard, shared, _clock) = fresh_shared();
    let clone = Arc::clone(&shared);
    let thread = std::thread::spawn(move || {
        let _guard = lock_shared(&clone);
        panic!("poison the notes lock on purpose");
    });
    assert!(thread.join().is_err());

    assert!(!status_unlocked(&shared));
    assert!(exit_save(&shared).is_some());
}

#[test]
fn a8_exit_save() {
    let (guard, shared, _clock) = fresh_shared();
    let words = create_unlocked(&shared);
    with_session(&shared, |session| session.create_note("t", "b")).unwrap();

    let report = exit_save(&shared).expect("a report");
    assert_eq!(
        report,
        LockReport {
            was_unlocked: true,
            final_save_failed: false,
        }
    );

    let store = NotesStore::new(guard.path.clone());
    let loaded = store.load(&words).unwrap();
    assert_eq!(loaded.data.notes.len(), 1);
    assert_eq!(loaded.data.notes[0].title, "t");
}
