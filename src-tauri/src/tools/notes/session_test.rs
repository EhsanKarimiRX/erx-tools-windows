//! Session tests: words flows, lock states, note operations, autosave, idle
//! lock and the final save. Every test uses its own temporary folder.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::clock::{Clock, DateParts, TestClock};
use super::crypto::CreateSettings;
use super::error::NotesError;
use super::model::{Note, PreviewMode};
use super::session::{
    AUTOSAVE_DELAY, AUTOSAVE_RETRY, LockReport, NoteView, NotesSession, NotesStatus,
    PREVIEW_MAX_CHARS, SaveState,
};
use super::store::{BACKUP_FILE_NAME, MAIN_FILE_NAME, NotesStore};
use super::test_support::{TempDir, real_store, scripted_store, scripted_store_counted};
use crate::tools::vault::passphrase::{NOTES_WORD_LIMITS, PassphraseSession, normalize_passphrase};

/// Persian "می‌خواهم" with a Persian yeh.
const PERSIAN_TITLE: &str = "\u{0645}\u{06CC}\u{200C}\u{062E}\u{0648}\u{0627}\u{0647}\u{0645}";
/// The same word with an Arabic yeh.
const ARABIC_QUERY: &str = "\u{0645}\u{064A}\u{200C}\u{062E}\u{0648}\u{0627}\u{0647}\u{0645}";

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

fn fresh_session(clock: Arc<TestClock>) -> (TempDir, NotesSession) {
    let guard = TempDir::new();
    let session = NotesSession::new(real_store(&guard.path), Box::new(clock));
    (guard, session)
}

fn create_unlocked_with(store: NotesStore, clock: Arc<TestClock>) -> (NotesSession, String) {
    let mut session = NotesSession::new(store, Box::new(clock));
    let view = session.generate_words(5).unwrap();
    session.confirm_words_saved().unwrap();
    let joined = view.words.join(" ");
    assert!(session.retype_words(&joined).unwrap());
    session.finish_create(&settings()).unwrap();
    (session, joined)
}

fn create_unlocked(clock: Arc<TestClock>) -> (TempDir, NotesSession, String) {
    let guard = TempDir::new();
    let store = real_store(&guard.path);
    let (session, words) = create_unlocked_with(store, clock);
    (guard, session, words)
}

/// `count` valid words that are not `different_from` (normalized text).
fn other_words(count: usize, different_from: &str) -> String {
    let mut other = PassphraseSession::new(NOTES_WORD_LIMITS);
    for _ in 0..20 {
        let joined = other.generate(count).unwrap().join(" ");
        if joined != different_from {
            return joined;
        }
    }
    panic!("could not generate different words");
}

fn id_of(session: &mut NotesSession, title: &str) -> String {
    session
        .list("")
        .unwrap()
        .into_iter()
        .find(|item| item.title == title)
        .expect("note in the list")
        .id
}

fn corrupt_body(path: &std::path::Path) {
    let mut bytes = fs::read(path).unwrap();
    let index = bytes.len() / 2;
    bytes[index] ^= 1;
    fs::write(path, bytes).unwrap();
}

#[test]
fn s1_create_flow() {
    let clock = test_clock();
    let (guard, mut session) = fresh_session(clock);
    assert_eq!(
        session.status(),
        NotesStatus {
            has_file: false,
            unlocked: false,
        }
    );

    let view = session.generate_words(5).unwrap();
    assert_eq!(view.words.len(), 5);
    assert_eq!(view.replacements_left, vec![3; 5]);

    assert_eq!(
        session.finish_create(&settings()).unwrap_err(),
        NotesError::WordsNotConfirmed
    );
    session.confirm_words_saved().unwrap();
    assert_eq!(
        session.finish_create(&settings()).unwrap_err(),
        NotesError::WordsNotRetyped
    );
    assert!(
        !session
            .retype_words("this is not the same passphrase")
            .unwrap()
    );
    assert_eq!(
        session.finish_create(&settings()).unwrap_err(),
        NotesError::WordsNotRetyped
    );

    let typed = format!(" \t{} \t ", view.words.join(" \t ").to_uppercase());
    assert!(session.retype_words(&typed).unwrap());
    session.finish_create(&settings()).unwrap();
    assert_eq!(
        session.status(),
        NotesStatus {
            has_file: true,
            unlocked: true,
        }
    );
    assert!(real_store(&guard.path).exists());
    assert!(matches!(
        session.retype_words(&typed),
        Err(NotesError::Words(_))
    ));
}

#[test]
fn s2_word_limits() {
    let (_guard, mut session) = fresh_session(test_clock());

    let message = "The passphrase must have between 5 and 12 words.".to_string();
    assert_eq!(
        session.generate_words(4).unwrap_err(),
        NotesError::Words(message.clone())
    );
    assert_eq!(
        session.generate_words(13).unwrap_err(),
        NotesError::Words(message)
    );
    assert_eq!(session.generate_words(5).unwrap().words.len(), 5);
    assert_eq!(session.generate_words(12).unwrap().words.len(), 12);

    let weak = session.generate_words(5).unwrap();
    assert!(weak.weak);
    assert!((weak.entropy_bits - 64.6).abs() < 0.1);
    let strong = session.generate_words(8).unwrap();
    assert!(!strong.weak);
}

#[test]
fn s3_states() {
    let clock = test_clock();
    let (guard, mut session, words) = create_unlocked(clock);
    session.create_note("keep", "body").unwrap();
    let report = session.lock();
    assert!(report.was_unlocked);

    assert_eq!(
        session.generate_words(5).unwrap_err(),
        NotesError::NotUnlocked
    );
    assert_eq!(session.list("").unwrap_err(), NotesError::NotUnlocked);
    assert_eq!(
        session
            .get_note("00000000000000000000000000000000")
            .unwrap_err(),
        NotesError::NotUnlocked
    );
    assert_eq!(
        session.create_note("title", "body").unwrap_err(),
        NotesError::NotUnlocked
    );
    assert_eq!(session.settings().unwrap_err(), NotesError::NotUnlocked);
    assert_eq!(session.save_now().unwrap_err(), NotesError::NotUnlocked);

    let wrong = other_words(5, &words);
    assert_eq!(
        session.unlock(&wrong).unwrap_err(),
        NotesError::WrongPassphraseOrHeader
    );
    assert!(matches!(
        session.unlock("zzz1 zzz2 zzz3 zzz4 zzz5"),
        Err(NotesError::Words(_))
    ));

    let noisy = format!("\t{}\n", words.to_uppercase().replace(' ', "   "));
    assert_eq!(
        session.unlock(&noisy).unwrap(),
        super::session::UnlockOutcome { used_backup: false }
    );
    assert_eq!(
        session.unlock(&words).unwrap_err(),
        NotesError::AlreadyUnlocked
    );
    assert_eq!(
        session.finish_create(&settings()).unwrap_err(),
        NotesError::AlreadyUnlocked
    );

    let (_guard, mut empty) = fresh_session(test_clock());
    assert_eq!(empty.unlock(&words).unwrap_err(), NotesError::NoNotesFile);

    // The guard keeps the folder alive until the end of the test.
    assert!(guard.path.exists());
}

#[test]
fn s4_notes_crud_and_default_title() {
    let clock = test_clock();
    let (_guard, mut session, _) = create_unlocked(clock.clone());
    let unix = clock.unix();

    let blank = session.create_note("", "first body").unwrap();
    assert_eq!(blank.title, "2023-11-14 22:13");
    assert_eq!(blank.created, unix);
    assert_eq!(blank.modified, unix);
    let spaces = session.create_note("   ", "second body").unwrap();
    assert_eq!(spaces.title, "2023-11-14 22:13");
    let named = session.create_note("My title", "third body").unwrap();
    assert_eq!(named.title, "My title");

    let named_id = named.id.clone();
    let named_created = named.created;
    clock.advance(Duration::from_secs(10));
    session
        .update_note(&named_id, "Renamed", "new body")
        .unwrap();
    let updated = session.get_note(&named_id).unwrap();
    assert_eq!(updated.id, named_id);
    assert_eq!(updated.title, "Renamed");
    assert_eq!(updated.body, "new body");
    assert_eq!(updated.created, named_created);
    assert_eq!(updated.modified, unix + 10);

    session.delete_note(&named_id).unwrap();
    assert_eq!(
        session.get_note(&named_id).unwrap_err(),
        NotesError::NoteNotFound
    );
    let unknown = "ffffffffffffffffffffffffffffffff";
    assert_eq!(
        session.get_note(unknown).unwrap_err(),
        NotesError::NoteNotFound
    );
    assert_eq!(
        session.update_note(unknown, "t", "b").unwrap_err(),
        NotesError::NoteNotFound
    );
    assert_eq!(
        session.delete_note(unknown).unwrap_err(),
        NotesError::NoteNotFound
    );

    let before = session.list("").unwrap().len();
    let too_long = "x".repeat(1025);
    assert_eq!(
        session.create_note(&too_long, "body").unwrap_err(),
        NotesError::TitleTooLong
    );
    assert_eq!(session.list("").unwrap().len(), before);
}

#[test]
fn s4_default_title_uses_created_time_and_utc_fallback() {
    let clock = TestClock::new(1_700_000_000, None);
    let (_guard, mut session, _) = create_unlocked(clock.clone());
    let view = session.create_note("", "body").unwrap();
    assert_eq!(view.title, "2023-11-14 22:13");
    let id = view.id.clone();

    clock.set_unix(1_700_003_600);
    session.update_note(&id, "  ", "changed").unwrap();
    let after = session.get_note(&id).unwrap();
    assert_eq!(after.title, "2023-11-14 22:13");
    assert_eq!(after.created, 1_700_000_000);
    assert_eq!(after.modified, 1_700_003_600);
    assert_eq!(after.body, "changed");
}

#[test]
fn s5_list_preview_and_search() {
    let clock = test_clock();
    let (_guard, mut session, _) = create_unlocked(clock.clone());

    let first = session
        .create_note("first", "first body")
        .unwrap()
        .id
        .clone();
    clock.advance(Duration::from_secs(10));
    let second = session
        .create_note("second", "second body")
        .unwrap()
        .id
        .clone();
    clock.advance(Duration::from_secs(10));
    let third = session
        .create_note("third", "third body")
        .unwrap()
        .id
        .clone();

    let order: Vec<String> = session
        .list("")
        .unwrap()
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert_eq!(order, vec![third.clone(), second.clone(), first.clone()]);

    clock.advance(Duration::from_secs(10));
    session
        .update_note(&first, "first", "first body again")
        .unwrap();
    assert_eq!(session.list("").unwrap()[0].id, first);

    assert!(
        session
            .list("")
            .unwrap()
            .into_iter()
            .all(|item| item.preview.is_none())
    );

    session.set_settings(PreviewMode::FirstLine, 10).unwrap();
    let previewed = session
        .create_note("preview", "\n  \n  line one\nline two")
        .unwrap();
    let preview_id = previewed.id.clone();
    let rows = session.list("").unwrap();
    let row = rows.iter().find(|item| item.id == preview_id).unwrap();
    assert_eq!(row.preview.as_deref(), Some("line one"));

    let long = session
        .create_note("long", &format!("{}\nsecond line", "x".repeat(130)))
        .unwrap();
    let long_id = long.id.clone();
    let expected = format!("{}\u{2026}", "x".repeat(PREVIEW_MAX_CHARS));
    let rows = session.list("").unwrap();
    let row = rows.iter().find(|item| item.id == long_id).unwrap();
    assert_eq!(row.preview.as_deref(), Some(expected.as_str()));

    let persian = session
        .create_note(PERSIAN_TITLE, "body")
        .unwrap()
        .id
        .clone();
    let found = session.list(ARABIC_QUERY).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, persian);

    session
        .create_note("only in the body", "unique needle")
        .unwrap();
    assert!(session.list("unique needle").unwrap().is_empty());
    assert!(session.list("nothing matches this").unwrap().is_empty());
}

#[test]
fn s6_autosave_timing() {
    let clock = test_clock();
    let (guard, mut session, words) = create_unlocked(clock.clone());
    session.create_note("note", "body").unwrap();
    assert_eq!(session.save_state(), SaveState::Pending);

    let tick = session.tick();
    assert!(!tick.autosaved);
    clock.advance(Duration::from_millis(1900));
    let tick = session.tick();
    assert!(!tick.autosaved);
    assert_eq!(session.save_state(), SaveState::Pending);
    clock.advance(Duration::from_millis(100));
    let tick = session.tick();
    assert!(tick.autosaved);
    assert!(!tick.autosave_failed);
    assert_eq!(session.save_state(), SaveState::Saved);

    session.lock();
    session.unlock(&words).unwrap();
    assert_eq!(session.list("").unwrap().len(), 1);

    // The timer restarts on every edit.
    let clock = test_clock();
    let (_guard, mut session, _) = create_unlocked(clock.clone());
    session.create_note("timer", "body").unwrap();
    clock.advance(Duration::from_millis(1500));
    let timer_id = id_of(&mut session, "timer");
    session.update_note(&timer_id, "timer", "edited").unwrap();
    clock.advance(Duration::from_millis(700));
    assert!(!session.tick().autosaved);
    clock.advance(Duration::from_millis(1400));
    assert!(session.tick().autosaved);
    assert!(guard.path.exists());
}

#[test]
fn s7_save_failure_and_retry() {
    let clock = test_clock();
    let guard = TempDir::new();
    // Rename call 1 is the creation; call 2 is the first save.
    let store = scripted_store(&guard.path, vec![2], false);
    let (mut session, words) = create_unlocked_with(store, clock.clone());

    session.create_note("note", "body").unwrap();
    clock.advance(AUTOSAVE_DELAY);
    let tick = session.tick();
    assert!(!tick.autosaved);
    assert!(tick.autosave_failed);
    assert_eq!(session.save_state(), SaveState::Failed);
    assert_eq!(session.list("").unwrap().len(), 1);

    clock.advance(Duration::from_secs(4));
    let tick = session.tick();
    assert!(!tick.autosaved);
    assert!(!tick.autosave_failed);

    clock.advance(AUTOSAVE_RETRY - Duration::from_secs(4));
    let tick = session.tick();
    assert!(tick.autosaved);
    assert!(!tick.autosave_failed);
    assert_eq!(session.save_state(), SaveState::Saved);

    session.lock();
    session.unlock(&words).unwrap();
    assert_eq!(session.list("").unwrap().len(), 1);
}

#[test]
fn s8_idle_lock() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock.clone());

    clock.advance(Duration::from_secs(9 * 60 + 59));
    assert!(session.tick().locked.is_none());

    session.list("").unwrap();
    clock.advance(Duration::from_secs(9 * 60 + 59));
    assert!(session.tick().locked.is_none());
    assert!(session.status().unlocked);

    clock.advance(Duration::from_secs(10 * 60));
    assert_eq!(
        session.tick().locked,
        Some(LockReport {
            was_unlocked: true,
            final_save_failed: false,
        })
    );
    assert!(!session.status().unlocked);

    // The final save on lock keeps the note.
    let clock = test_clock();
    let (_guard, mut session, words) = create_unlocked(clock.clone());
    session.create_note("save me", "body").unwrap();
    clock.advance(Duration::from_secs(10 * 60));
    assert!(session.tick().locked.is_some());
    session.unlock(&words).unwrap();
    assert_eq!(session.list("").unwrap().len(), 1);

    // A one-minute auto lock, and settings that survive a lock.
    let clock = test_clock();
    let (_guard, mut session, words) = create_unlocked(clock.clone());
    session.set_settings(PreviewMode::FirstLine, 1).unwrap();
    clock.advance(Duration::from_secs(59));
    assert!(session.tick().locked.is_none());
    clock.advance(Duration::from_secs(1));
    assert!(session.tick().locked.is_some());
    session.unlock(&words).unwrap();
    let settings = session.settings().unwrap();
    assert_eq!(settings.preview, PreviewMode::FirstLine);
    assert_eq!(settings.auto_lock_minutes, 1);

    assert_eq!(
        session.set_settings(PreviewMode::FirstLine, 0).unwrap_err(),
        NotesError::DataInvalid
    );
    assert_eq!(
        session
            .set_settings(PreviewMode::FirstLine, 241)
            .unwrap_err(),
        NotesError::DataInvalid
    );
}

#[test]
fn s9_lock_with_failing_save() {
    let clock = test_clock();
    let guard = TempDir::new();
    let store = scripted_store(&guard.path, vec![2], false);
    let (mut session, _words) = create_unlocked_with(store, clock);
    session.create_note("lost edit", "body").unwrap();

    let report = session.lock();
    assert_eq!(
        report,
        LockReport {
            was_unlocked: true,
            final_save_failed: true,
        }
    );
    assert!(!session.status().unlocked);
    assert_eq!(session.list("").unwrap_err(), NotesError::NotUnlocked);
}

#[test]
fn s10_change_passphrase() {
    let clock = test_clock();
    let (guard, mut session, old_words) = create_unlocked(clock.clone());
    session.create_note("one", "first").unwrap();
    session.create_note("two", "second").unwrap();

    let new_view = session.generate_words(6).unwrap();
    let new_words = new_view.words.join(" ");
    assert_eq!(
        session
            .change_passphrase(&old_words, &settings())
            .unwrap_err(),
        NotesError::WordsNotConfirmed
    );
    session.confirm_words_saved().unwrap();
    assert_eq!(
        session
            .change_passphrase(&old_words, &settings())
            .unwrap_err(),
        NotesError::WordsNotRetyped
    );
    session.retype_words(&new_words).unwrap();

    let wrong = other_words(5, &old_words);
    assert_eq!(
        session.change_passphrase(&wrong, &settings()).unwrap_err(),
        NotesError::WrongPassphraseOrHeader
    );
    assert!(real_store(&guard.path).load(&old_words).is_ok());

    let report = session.change_passphrase(&old_words, &settings()).unwrap();
    assert!(report.old_backup_gone);

    session.lock();
    assert_eq!(
        session
            .change_passphrase(&old_words, &settings())
            .unwrap_err(),
        NotesError::NotUnlocked
    );
    assert_eq!(
        session.unlock(&old_words).unwrap_err(),
        NotesError::WrongPassphraseOrHeader
    );
    session.unlock(&new_words).unwrap();
    assert_eq!(session.list("").unwrap().len(), 2);
}

#[test]
fn s11_backup_load() {
    let clock = test_clock();
    let (guard, mut session, words) = create_unlocked(clock.clone());

    session.create_note("one", "first").unwrap();
    clock.advance(AUTOSAVE_DELAY);
    assert!(session.tick().autosaved);
    session.create_note("two", "second").unwrap();
    clock.advance(AUTOSAVE_DELAY);
    assert!(session.tick().autosaved);

    let main_path = guard.path.join(MAIN_FILE_NAME);
    let backup_path = guard.path.join(BACKUP_FILE_NAME);
    assert!(main_path.exists());
    assert!(backup_path.exists());
    let backup_before = fs::read(&backup_path).unwrap();

    session.lock();
    corrupt_body(&main_path);
    let outcome = session.unlock(&words).unwrap();
    assert!(outcome.used_backup);

    session.create_note("three", "third").unwrap();
    clock.advance(AUTOSAVE_DELAY);
    assert!(session.tick().autosaved);
    assert_eq!(fs::read(&backup_path).unwrap(), backup_before);

    session.lock();
    let outcome = session.unlock(&words).unwrap();
    assert!(!outcome.used_backup);

    session.create_note("four", "fourth").unwrap();
    clock.advance(AUTOSAVE_DELAY);
    assert!(session.tick().autosaved);
    assert_ne!(fs::read(&backup_path).unwrap(), backup_before);
}

#[test]
fn s12_initial_settings() {
    let (_guard, mut session, _) = create_unlocked(test_clock());
    let settings = session.settings().unwrap();
    assert_eq!(settings.preview, PreviewMode::Titles);
    assert_eq!(settings.auto_lock_minutes, 10);
}

#[test]
fn s13_debug_redaction() {
    let (_guard, mut session, _) = create_unlocked(test_clock());
    assert_eq!(
        format!("{session:?}"),
        "NotesSession { unlocked: true, .. }"
    );
    let view: NoteView = session
        .create_note("secret title", "secret body text")
        .unwrap();
    let expected = format!("NoteView {{ id: {}, .. }}", view.id);
    assert_eq!(format!("{view:?}"), expected);
    let printed = format!("{view:?}");
    assert!(!printed.contains("secret title"));
    assert!(!printed.contains("secret body text"));

    session.lock();
    assert_eq!(
        format!("{session:?}"),
        "NotesSession { unlocked: false, .. }"
    );
}

#[test]
fn s14_locked_activity_and_tick() {
    let (_guard, mut session) = fresh_session(test_clock());
    session.activity();
    let tick = session.tick();
    assert!(!tick.autosaved);
    assert!(!tick.autosave_failed);
    assert!(tick.locked.is_none());
    assert_eq!(session.save_state(), SaveState::Saved);
}

#[test]
fn s15_save_now() {
    let (guard, mut session, _) = create_unlocked(test_clock());
    session.create_note("note", "body").unwrap();
    assert!(session.save_now().is_ok());
    assert_eq!(session.save_state(), SaveState::Saved);

    let main = guard.path.join(MAIN_FILE_NAME);
    let before = fs::read(&main).unwrap();
    assert!(session.save_now().is_ok());
    assert_eq!(fs::read(&main).unwrap(), before);
}

#[test]
fn s16_autosave_retry_gate() {
    let clock = test_clock();
    let guard = TempDir::new();
    let (creator, words) = create_unlocked_with(real_store(&guard.path), clock.clone());
    drop(creator);

    // Every rename of this store fails, so every save attempt fails.
    let (store, fs_calls) = scripted_store_counted(&guard.path, (1..=50).collect(), false);
    let mut session = NotesSession::new(store, Box::new(clock.clone()));
    session.unlock(&words).unwrap();
    session.create_note("note", "body").unwrap();

    clock.advance(AUTOSAVE_DELAY);
    let tick = session.tick();
    assert!(tick.autosave_failed);
    assert!(!tick.autosaved);
    let after_failure = fs_calls.calls.load(Ordering::SeqCst);
    assert!(after_failure > 0);

    // Only 3 s after the failure: less than AUTOSAVE_RETRY, no attempt.
    clock.advance(Duration::from_secs(3));
    let tick = session.tick();
    assert!(!tick.autosaved);
    assert!(!tick.autosave_failed);
    assert!(tick.locked.is_none());
    assert_eq!(fs_calls.calls.load(Ordering::SeqCst), after_failure);

    // 5 s after the failure: a new attempt happens.
    clock.advance(Duration::from_secs(2));
    let tick = session.tick();
    assert!(tick.autosave_failed);
    assert!(fs_calls.calls.load(Ordering::SeqCst) > after_failure);
}

#[test]
fn s17_activity_refreshed() {
    type Operation = Box<dyn Fn(&mut NotesSession, &str)>;
    let operations: Vec<(&str, Operation)> = vec![
        ("activity", Box::new(|session, _id| session.activity())),
        (
            "settings",
            Box::new(|session, _id| {
                session.settings().unwrap();
            }),
        ),
        (
            "set_settings",
            Box::new(|session, _id| {
                session.set_settings(PreviewMode::Titles, 10).unwrap();
            }),
        ),
        (
            "list",
            Box::new(|session, _id| {
                session.list("").unwrap();
            }),
        ),
        (
            "get_note",
            Box::new(|session, id| {
                session.get_note(id).unwrap();
            }),
        ),
        (
            "create_note",
            Box::new(|session, _id| {
                session.create_note("second", "body two").unwrap();
            }),
        ),
        (
            "update_note",
            Box::new(|session, id| {
                session.update_note(id, "edited", "body three").unwrap();
            }),
        ),
        (
            "delete_note",
            Box::new(|session, id| {
                session.delete_note(id).unwrap();
            }),
        ),
    ];

    for (name, operation) in operations {
        let clock = test_clock();
        let (guard, mut session, _words) = create_unlocked(clock.clone());
        let id = session.create_note("note", "body").unwrap().id.clone();

        clock.advance(Duration::from_secs(9 * 60));
        operation(&mut session, &id);
        clock.advance(Duration::from_secs(9 * 60));
        assert!(
            session.tick().locked.is_none(),
            "{name} did not refresh the idle timer"
        );

        clock.advance(Duration::from_secs(2 * 60));
        assert!(
            session.tick().locked.is_some(),
            "{name} kept the session unlocked after the idle limit"
        );
        drop(guard);
    }
}

#[test]
fn s18_lock_clears_words() {
    let clock = test_clock();
    let (guard, mut session, words) = create_unlocked(clock);

    let view = session.generate_words(5).unwrap();
    session.confirm_words_saved().unwrap();
    let joined = view.words.join(" ");
    assert!(session.retype_words(&joined).unwrap());

    session.lock();
    session.unlock(&words).unwrap();

    let main = guard.path.join(MAIN_FILE_NAME);
    let backup = guard.path.join(BACKUP_FILE_NAME);
    let main_before = fs::read(&main).unwrap();
    let backup_before = fs::read(&backup).ok();

    // The generated words were wiped by the lock, so there is nothing to use.
    let error = session.change_passphrase(&words, &settings()).unwrap_err();
    assert!(matches!(error, NotesError::Words(_)));
    assert_eq!(fs::read(&main).unwrap(), main_before);
    assert_eq!(fs::read(&backup).ok(), backup_before);

    // New words, confirmed but not retyped: still refused.
    session.generate_words(5).unwrap();
    session.confirm_words_saved().unwrap();
    assert_eq!(
        session.change_passphrase(&words, &settings()).unwrap_err(),
        NotesError::WordsNotRetyped
    );
    assert_eq!(fs::read(&main).unwrap(), main_before);
    assert_eq!(fs::read(&backup).ok(), backup_before);
}

#[test]
fn s19_change_passphrase_needs_retype() {
    let clock = test_clock();
    let (guard, mut session, words) = create_unlocked(clock);
    session.generate_words(5).unwrap();
    session.confirm_words_saved().unwrap();

    let main = guard.path.join(MAIN_FILE_NAME);
    let backup = guard.path.join(BACKUP_FILE_NAME);
    let main_before = fs::read(&main).unwrap();
    let backup_before = fs::read(&backup).ok();

    assert_eq!(
        session.change_passphrase(&words, &settings()).unwrap_err(),
        NotesError::WordsNotRetyped
    );
    assert_eq!(fs::read(&main).unwrap(), main_before);
    assert_eq!(fs::read(&backup).ok(), backup_before);
    assert_eq!(session.list("").unwrap().len(), 0);
}

#[test]
fn s20_change_passphrase_resets_main_is_good() {
    let clock = test_clock();
    let (guard, mut session, words) = create_unlocked(clock.clone());
    session.create_note("one", "first").unwrap();
    clock.advance(AUTOSAVE_DELAY);
    assert!(session.tick().autosaved);
    session.create_note("two", "second").unwrap();
    clock.advance(AUTOSAVE_DELAY);
    assert!(session.tick().autosaved);

    let main = guard.path.join(MAIN_FILE_NAME);
    let backup = guard.path.join(BACKUP_FILE_NAME);
    assert!(main.exists());
    assert!(backup.exists());

    session.lock();
    corrupt_body(&main);
    assert!(session.unlock(&words).unwrap().used_backup);

    let view = session.generate_words(6).unwrap();
    session.confirm_words_saved().unwrap();
    let new_words = view.words.join(" ");
    assert!(session.retype_words(&new_words).unwrap());
    session.change_passphrase(&words, &settings()).unwrap();

    // change_passphrase writes the same bytes to main and backup. Make the
    // backup differ so the next save shows whether the old main was rotated
    // into it.
    let snapshot = fs::read(&main).unwrap();
    corrupt_body(&backup);

    let id = session.list("").unwrap().first().unwrap().id.clone();
    session.update_note(&id, "one", "edited").unwrap();
    session.save_now().unwrap();
    assert_eq!(fs::read(&backup).unwrap(), snapshot);
}

#[test]
fn s21_words_text_without_words() {
    let clock = test_clock();
    let (_guard, session) = fresh_session(clock);
    assert!(matches!(session.words_text(), Err(NotesError::Words(_))));
}

#[test]
fn s22_words_text_matches_view() {
    let clock = test_clock();
    let (_guard, mut session) = fresh_session(clock);
    let view = session.generate_words(5).unwrap();
    let joined = view.words.join(" ");
    let status = session.status();
    let save_state = session.save_state();

    let first = session.words_text().unwrap();
    let second = session.words_text().unwrap();
    assert_eq!(first.as_str(), joined.as_str());
    assert_eq!(second.as_str(), joined.as_str());
    assert_eq!(session.status(), status);
    assert_eq!(session.save_state(), save_state);
}

#[test]
fn s23_words_text_follows_refresh() {
    let clock = test_clock();
    let (_guard, mut session) = fresh_session(clock);
    let view = session.generate_words(5).unwrap();
    let refreshed = session.refresh_word(0).unwrap();

    let mut expected = view.words;
    expected[0] = refreshed.word;
    assert_eq!(
        session.words_text().unwrap().as_str(),
        expected.join(" ").as_str()
    );
}

#[test]
fn s24_words_text_after_confirm_and_retype() {
    let clock = test_clock();
    let (_guard, mut session) = fresh_session(clock);
    let view = session.generate_words(5).unwrap();
    let before = session.words_text().unwrap();

    session.confirm_words_saved().unwrap();
    let joined = view.words.join(" ");
    assert!(session.retype_words(&joined).unwrap());
    assert_eq!(session.words_text().unwrap().as_str(), before.as_str());
}

#[test]
fn s25_words_text_cleared() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);

    session.generate_words(5).unwrap();
    session.clear_words();
    assert!(matches!(session.words_text(), Err(NotesError::Words(_))));

    session.generate_words(5).unwrap();
    session.lock();
    assert!(matches!(session.words_text(), Err(NotesError::Words(_))));
}

#[test]
fn s26_words_text_gone_after_finish_create() {
    let clock = test_clock();
    let (_guard, mut session) = fresh_session(clock);
    let view = session.generate_words(5).unwrap();
    session.confirm_words_saved().unwrap();
    let joined = view.words.join(" ");
    assert!(session.retype_words(&joined).unwrap());
    session.finish_create(&settings()).unwrap();

    assert!(matches!(session.words_text(), Err(NotesError::Words(_))));
}

#[test]
fn s27_words_text_gone_after_change_passphrase() {
    let clock = test_clock();
    let (_guard, mut session, old_words) = create_unlocked(clock);
    let view = session.generate_words(5).unwrap();
    session.confirm_words_saved().unwrap();
    let joined = view.words.join(" ");
    assert!(session.retype_words(&joined).unwrap());
    session.change_passphrase(&old_words, &settings()).unwrap();

    assert!(matches!(session.words_text(), Err(NotesError::Words(_))));
}

#[test]
fn s28_words_text_works_when_unlocked() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    let view = session.generate_words(5).unwrap();
    assert_eq!(
        session.words_text().unwrap().as_str(),
        view.words.join(" ").as_str()
    );
}

#[test]
fn sn_export_data_requires_unlock() {
    let clock = test_clock();
    let (_guard, session) = fresh_session(clock);
    assert_eq!(session.export_data().unwrap_err(), NotesError::NotUnlocked);
}

#[test]
fn sn_import_appends() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    assert_eq!(session.list("").unwrap().len(), 0);

    let imported = [
        Note::new(
            "0123456789abcdef0123456789abcdef".to_string(),
            "first".to_string(),
            "body one".to_string(),
            1_700_000_100,
        ),
        Note::new(
            "fedcba9876543210fedcba9876543210".to_string(),
            "second".to_string(),
            "body two".to_string(),
            1_700_000_200,
        ),
    ];
    assert_eq!(session.import_notes(&imported).unwrap(), 2);
    assert_eq!(session.list("").unwrap().len(), 2);
    assert_eq!(session.save_state(), SaveState::Pending);

    let titles: Vec<String> = session
        .list("")
        .unwrap()
        .into_iter()
        .map(|item| item.title)
        .collect();
    assert!(titles.contains(&"first".to_string()));
    assert!(titles.contains(&"second".to_string()));
}

#[test]
fn sn_import_regenerates_colliding_id() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    session.create_note("existing", "body").unwrap();
    assert_eq!(session.list("").unwrap().len(), 1);
    let existing_id = id_of(&mut session, "existing");

    let imported = [Note::new(
        existing_id.clone(),
        "imported".to_string(),
        "other body".to_string(),
        1_700_000_300,
    )];
    assert_eq!(session.import_notes(&imported).unwrap(), 1);
    assert_eq!(session.list("").unwrap().len(), 2);

    let imported_id = id_of(&mut session, "imported");
    assert_ne!(imported_id, existing_id);
    assert_eq!(imported_id.len(), 32);
    assert_eq!(id_of(&mut session, "existing"), existing_id);
}

#[test]
fn sn_export_words_require_unlock() {
    let clock = test_clock();
    let (_guard, mut session) = fresh_session(clock);
    assert_eq!(
        session.generate_export_words(5).err(),
        Some(NotesError::NotUnlocked)
    );
    assert_eq!(
        session.refresh_export_word(0).err(),
        Some(NotesError::NotUnlocked)
    );
}

#[test]
fn sn_export_words_count_limits() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    assert!(matches!(
        session.generate_export_words(4).err(),
        Some(NotesError::Words(_))
    ));
    assert!(matches!(
        session.generate_export_words(13).err(),
        Some(NotesError::Words(_))
    ));
    assert!(session.generate_export_words(5).is_ok());
    assert!(session.generate_export_words(12).is_ok());
}

#[test]
fn sn_export_words_weak_flag() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    let five = session.generate_export_words(5).unwrap();
    assert_eq!(five.words.len(), 5);
    assert_eq!(five.replacements_left.len(), 5);
    assert!(five.weak);
    let eight = session.generate_export_words(8).unwrap();
    assert_eq!(eight.words.len(), 8);
    assert!(!eight.weak);
}

#[test]
fn sn_export_words_kdf_needs_confirm_and_retype() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    let view = session.generate_export_words(8).unwrap();
    let joined = view.words.join(" ");

    // Not confirmed yet.
    assert_eq!(
        session.export_words_for_kdf().err(),
        Some(NotesError::WordsNotConfirmed)
    );
    assert_eq!(
        session.retype_export_words(&joined).err(),
        Some(NotesError::WordsNotConfirmed)
    );

    session.confirm_export_words_saved().unwrap();

    // Confirmed but not retyped.
    assert_eq!(
        session.export_words_for_kdf().err(),
        Some(NotesError::WordsNotRetyped)
    );

    // Wrong words: no match, still refused.
    let wrong = other_words(8, &joined);
    assert_eq!(session.retype_export_words(&wrong).unwrap(), false);
    assert_eq!(
        session.export_words_for_kdf().err(),
        Some(NotesError::WordsNotRetyped)
    );

    // Right words: now allowed.
    assert_eq!(session.retype_export_words(&joined).unwrap(), true);
    let kdf = session.export_words_for_kdf().unwrap();
    assert_eq!(kdf.as_str().split_whitespace().count(), 8);
}

#[test]
fn sn_export_words_generate_again_resets_confirmation() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    let view = session.generate_export_words(5).unwrap();
    let joined = view.words.join(" ");
    session.confirm_export_words_saved().unwrap();
    assert!(session.retype_export_words(&joined).unwrap());
    assert!(session.export_words_for_kdf().is_ok());

    session.generate_export_words(5).unwrap();
    assert!(session.export_words_for_kdf().is_err());
}

#[test]
fn sn_export_words_copy_text_matches_view() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    let view = session.generate_export_words(6).unwrap();
    let copied = session.export_words_for_copy().unwrap();
    assert_eq!(copied.as_str(), view.words.join(" "));
}

#[test]
fn sn_export_words_refresh_slot() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    session.generate_export_words(5).unwrap();
    let refreshed = session.refresh_export_word(0).unwrap();
    assert!(!refreshed.word.is_empty());
    assert!(session.refresh_export_word(99).is_err());
}

#[test]
fn sn_export_words_clear_wipes_everything() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    let view = session.generate_export_words(5).unwrap();
    let joined = view.words.join(" ");
    session.confirm_export_words_saved().unwrap();
    assert!(session.retype_export_words(&joined).unwrap());
    assert!(session.export_words_for_kdf().is_ok());

    session.clear_export_words();
    assert!(session.export_words_for_kdf().is_err());
    assert!(session.export_words_for_copy().is_err());
    // The notebook itself is untouched.
    assert!(session.list("").is_ok());
}

#[test]
fn sn_export_words_wiped_on_lock() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    let view = session.generate_export_words(5).unwrap();
    let joined = view.words.join(" ");
    session.confirm_export_words_saved().unwrap();
    assert!(session.retype_export_words(&joined).unwrap());

    session.lock();
    assert!(session.export_words_for_kdf().is_err());
    assert!(session.export_words_for_copy().is_err());
    assert_eq!(
        session.generate_export_words(5).err(),
        Some(NotesError::NotUnlocked)
    );
}

#[test]
fn sn_verify_current_requires_unlock() {
    let clock = test_clock();
    let (_guard, session) = fresh_session(clock);
    assert_eq!(
        session.verify_current_words("anything").err(),
        Some(NotesError::NotUnlocked)
    );
}

#[test]
fn sn_verify_current_ok() {
    let clock = test_clock();
    let (_guard, session, words) = create_unlocked(clock);
    let verified = session.verify_current_words(&words).unwrap();
    assert_eq!(verified.as_str(), normalize_passphrase(&words).as_str());
}

#[test]
fn sn_verify_current_wrong_words() {
    let clock = test_clock();
    let (_guard, session, words) = create_unlocked(clock);
    let wrong = other_words(5, &words);
    assert_eq!(
        session.verify_current_words(&wrong).err(),
        Some(NotesError::WrongPassphraseOrHeader)
    );
}

#[test]
fn sn_verify_current_invalid_words() {
    let clock = test_clock();
    let (_guard, session, _words) = create_unlocked(clock);
    assert!(matches!(
        session.verify_current_words("hello").err(),
        Some(NotesError::Words(_))
    ));
}

#[test]
fn sn_verify_current_keeps_state() {
    let clock = test_clock();
    let (_guard, mut session, words) = create_unlocked(clock);
    session.generate_export_words(8).unwrap();
    let wrong = other_words(5, &words);
    assert!(session.verify_current_words(&words).is_ok());
    assert!(session.verify_current_words(&wrong).is_err());
    assert!(session.status().unlocked);
    assert!(session.export_words_for_copy().is_ok());
}

#[test]
fn sn_take_export_words_ok() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    let view = session.generate_export_words(8).unwrap();
    let joined = view.words.join(" ");
    session.confirm_export_words_saved().unwrap();
    assert!(session.retype_export_words(&joined).unwrap());
    let taken = session.take_export_words().unwrap();
    assert_eq!(taken.as_str(), normalize_passphrase(&joined).as_str());
    assert!(session.export_words_for_copy().is_err());
}

#[test]
fn sn_take_export_words_needs_confirm() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    session.generate_export_words(8).unwrap();
    assert_eq!(
        session.take_export_words().err(),
        Some(NotesError::WordsNotConfirmed)
    );
    assert!(session.export_words_for_copy().is_err());
}

#[test]
fn sn_take_export_words_needs_retype() {
    let clock = test_clock();
    let (_guard, mut session, _words) = create_unlocked(clock);
    let view = session.generate_export_words(8).unwrap();
    let joined = view.words.join(" ");
    session.confirm_export_words_saved().unwrap();
    let wrong = other_words(8, &joined);
    assert!(!session.retype_export_words(&wrong).unwrap());
    assert_eq!(
        session.take_export_words().err(),
        Some(NotesError::WordsNotRetyped)
    );
    assert!(session.export_words_for_copy().is_err());
}

#[test]
fn sn_take_export_words_not_unlocked() {
    let clock = test_clock();
    let (_guard, mut session) = fresh_session(clock);
    assert_eq!(
        session.take_export_words().err(),
        Some(NotesError::NotUnlocked)
    );
}
