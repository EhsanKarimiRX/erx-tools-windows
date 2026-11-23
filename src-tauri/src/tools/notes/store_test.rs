//! File store tests: atomic saves, backup rotation, recovery and the
//! passphrase change. Every test uses its own temporary folder.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, SystemTime};

use super::crypto::{self, CreateSettings, NotesKeys};
use super::error::NotesError;
use super::model::{MAX_NOTE_BODY_BYTES, NotesData};
use super::store::*;
use super::test_support::{ScriptedFs, TempDir, scripted_store};

const WORDS_A: &str = "alpha bravo charlie delta echo";
const WORDS_B: &str = "foxtrot golf hotel india juliet";
const PERSIAN: &str =
    "\u{0645}\u{06CC}\u{200C}\u{062E}\u{0648}\u{0627}\u{0647}\u{0645} \u{06F1}\u{06F2}\u{06F3}";

fn settings() -> CreateSettings {
    CreateSettings::for_tests()
}

fn data_with(body: &str) -> NotesData {
    let mut data = NotesData::new_empty();
    data.add_note("title", body, 1_700_000_000).unwrap();
    data
}

fn same_data(a: &NotesData, b: &NotesData) -> bool {
    a.version == b.version
        && a.settings == b.settings
        && a.notes.len() == b.notes.len()
        && a.notes.iter().zip(&b.notes).all(|(left, right)| {
            left.id == right.id
                && left.title == right.title
                && left.body == right.body
                && left.created == right.created
                && left.modified == right.modified
        })
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let path = entry.path();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(path).unwrap(),
            )
        })
        .collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn set_old(path: &Path) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(2 * 3600))
        .unwrap();
}

/// A store with a main file (version two) and a backup (version one).
fn setup_two_saves() -> (TempDir, NotesStore, NotesKeys, NotesData, NotesData) {
    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    let version_one = data_with("version one");
    let version_two = data_with("version two");
    let keys = store
        .create_new(WORDS_A, &settings(), &version_one)
        .unwrap();
    store.save(&keys, &version_two, true).unwrap();
    (guard, store, keys, version_one, version_two)
}

fn corrupt_body(path: &Path) {
    let mut bytes = fs::read(path).unwrap();
    let index = bytes.len() / 2;
    bytes[index] ^= 1;
    fs::write(path, bytes).unwrap();
}

#[test]
fn t1_create_new_then_load() {
    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    let data = data_with(PERSIAN);

    let keys = store.create_new(WORDS_A, &settings(), &data).unwrap();
    assert!(keys.header().is_ok());

    let loaded = store.load(WORDS_A).unwrap();
    assert_eq!(loaded.source, LoadSource::Main);
    assert!(same_data(&loaded.data, &data));

    assert_eq!(file_names(&guard.path), vec![MAIN_FILE_NAME.to_string()]);
    let length = fs::metadata(store.main_path()).unwrap().len();
    assert_eq!(length % 262_144, 0);
}

#[test]
fn t2_create_new_twice_is_refused() {
    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    store
        .create_new(WORDS_A, &settings(), &data_with("first"))
        .unwrap();
    let before = fs::read(store.main_path()).unwrap();

    assert_eq!(
        store
            .create_new(WORDS_A, &settings(), &data_with("second"))
            .unwrap_err(),
        NotesError::AlreadyExists
    );
    assert_eq!(fs::read(store.main_path()).unwrap(), before);

    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    fs::write(store.backup_path(), &before).unwrap();
    assert_eq!(
        store
            .create_new(WORDS_A, &settings(), &data_with("third"))
            .unwrap_err(),
        NotesError::AlreadyExists
    );
}

#[test]
fn t3_rotation_keeps_the_previous_version() {
    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    let version_one = data_with("version one");
    let version_two = data_with("version two");
    let version_three = data_with("version three");

    let keys = store
        .create_new(WORDS_A, &settings(), &version_one)
        .unwrap();
    let main_one = fs::read(store.main_path()).unwrap();
    assert_eq!(file_names(&guard.path), vec![MAIN_FILE_NAME.to_string()]);

    store.save(&keys, &version_two, true).unwrap();
    let main_two = fs::read(store.main_path()).unwrap();
    assert_ne!(main_one, main_two);
    assert_eq!(fs::read(store.backup_path()).unwrap(), main_one);
    assert!(same_data(&store.load(WORDS_A).unwrap().data, &version_two));
    assert_eq!(
        file_names(&guard.path),
        vec![MAIN_FILE_NAME.to_string(), BACKUP_FILE_NAME.to_string()]
    );

    store.save(&keys, &version_three, true).unwrap();
    assert_eq!(fs::read(store.backup_path()).unwrap(), main_two);
    assert!(same_data(
        &store.load(WORDS_A).unwrap().data,
        &version_three
    ));
    assert_eq!(
        file_names(&guard.path),
        vec![MAIN_FILE_NAME.to_string(), BACKUP_FILE_NAME.to_string()]
    );
}

#[test]
fn t4_same_data_saves_different_bytes_with_the_same_header() {
    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    let data = data_with("same data");

    let keys = store.create_new(WORDS_A, &settings(), &data).unwrap();
    let first = fs::read(store.main_path()).unwrap();
    store.save(&keys, &data, true).unwrap();
    let second = fs::read(store.main_path()).unwrap();

    assert_ne!(first, second);
    assert_eq!(first[..192], second[..192]);
    assert!(same_data(&store.load(WORDS_A).unwrap().data, &data));
}

#[test]
fn t5_wrong_words_and_missing_files() {
    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    store
        .create_new(WORDS_A, &settings(), &data_with("wrong words"))
        .unwrap();
    assert_eq!(
        store.load(WORDS_B).unwrap_err(),
        NotesError::WrongPassphraseOrHeader
    );

    let empty = TempDir::new();
    let empty_store = NotesStore::new(empty.path.clone());
    assert_eq!(
        empty_store.load(WORDS_A).unwrap_err(),
        NotesError::NoNotesFile
    );

    let missing = empty.path.join("missing");
    assert_eq!(
        NotesStore::new(missing).load(WORDS_A).unwrap_err(),
        NotesError::NoNotesFile
    );
}

#[test]
fn t6_recovery_from_the_backup() {
    // (a) a changed body in the main file
    {
        let (_guard, store, _keys, version_one, _version_two) = setup_two_saves();
        corrupt_body(&store.main_path());
        let loaded = store.load(WORDS_A).unwrap();
        assert_eq!(loaded.source, LoadSource::Backup);
        assert!(same_data(&loaded.data, &version_one));
    }

    // (b) a changed UUID in the main file
    {
        let (_guard, store, _keys, version_one, _version_two) = setup_two_saves();
        let mut main = fs::read(store.main_path()).unwrap();
        main[12] ^= 1;
        fs::write(store.main_path(), main).unwrap();
        let loaded = store.load(WORDS_A).unwrap();
        assert_eq!(loaded.source, LoadSource::Backup);
        assert!(same_data(&loaded.data, &version_one));
    }

    // (c) the main file replaced by zero bytes
    {
        let (_guard, store, _keys, version_one, _version_two) = setup_two_saves();
        fs::write(store.main_path(), vec![0u8; 262_144]).unwrap();
        let loaded = store.load(WORDS_A).unwrap();
        assert_eq!(loaded.source, LoadSource::Backup);
        assert!(same_data(&loaded.data, &version_one));
    }

    // (d) both files damaged: an error and no writes
    {
        let (guard, store, _keys, _version_one, _version_two) = setup_two_saves();
        corrupt_body(&store.main_path());
        corrupt_body(&store.backup_path());
        let before = snapshot(&guard.path);
        let error = store.load(WORDS_A).unwrap_err();
        assert!(
            matches!(
                error,
                NotesError::WrongPassphraseOrHeader
                    | NotesError::DamagedFile
                    | NotesError::NotANotesFile
            ),
            "unexpected error {error:?}"
        );
        assert_eq!(snapshot(&guard.path), before);
    }

    // (e) main deleted, then both deleted
    {
        let (_guard, store, _keys, version_one, _version_two) = setup_two_saves();
        fs::remove_file(store.main_path()).unwrap();
        let loaded = store.load(WORDS_A).unwrap();
        assert_eq!(loaded.source, LoadSource::Backup);
        assert!(same_data(&loaded.data, &version_one));

        fs::remove_file(store.backup_path()).unwrap();
        assert_eq!(store.load(WORDS_A).unwrap_err(), NotesError::NoNotesFile);
    }
}

#[test]
fn t7_newer_versions_never_fall_back_to_the_backup() {
    // (a) a newer container version in the main file
    {
        let (_guard, store, _keys, _version_one, _version_two) = setup_two_saves();
        let mut main = fs::read(store.main_path()).unwrap();
        main[8] = 2;
        main[9] = 0;
        fs::write(store.main_path(), main).unwrap();
        assert_eq!(
            store.load(WORDS_A).unwrap_err(),
            NotesError::UnsupportedVersion
        );
    }

    // (b) a valid container with a newer payload version
    {
        let (_guard, store, _keys, _version_one, _version_two) = setup_two_saves();
        let (file, _keys) = crypto::create(
            WORDS_A,
            &settings(),
            br#"{"version":2,"settings":{},"notes":[]}"#,
        )
        .unwrap();
        fs::write(store.main_path(), file).unwrap();
        assert_eq!(store.load(WORDS_A).unwrap_err(), NotesError::DataTooNew);
    }
}

#[test]
fn t8_saving_after_a_backup_load_does_not_touch_the_backup() {
    // The next save after a backup load keeps the good backup.
    {
        let (guard, store, _keys, version_one, _version_two) = setup_two_saves();
        corrupt_body(&store.main_path());
        let loaded = store.load(WORDS_A).unwrap();
        assert_eq!(loaded.source, LoadSource::Backup);
        assert!(same_data(&loaded.data, &version_one));

        let backup_before = fs::read(store.backup_path()).unwrap();
        let version_three = data_with("version three");
        store.save(&loaded.keys, &version_three, false).unwrap();
        assert_eq!(fs::read(store.backup_path()).unwrap(), backup_before);

        let reloaded = store.load(WORDS_A).unwrap();
        assert_eq!(reloaded.source, LoadSource::Main);
        assert!(same_data(&reloaded.data, &version_three));
        assert_eq!(
            file_names(&guard.path),
            vec![MAIN_FILE_NAME.to_string(), BACKUP_FILE_NAME.to_string()]
        );
    }

    // With main_is_good = true the damaged main file overwrites the backup.
    {
        let (_guard, store, _keys, _version_one, _version_two) = setup_two_saves();
        corrupt_body(&store.main_path());
        let loaded = store.load(WORDS_A).unwrap();
        assert_eq!(loaded.source, LoadSource::Backup);

        let backup_before = fs::read(store.backup_path()).unwrap();
        store
            .save(&loaded.keys, &data_with("version four"), true)
            .unwrap();
        assert_ne!(fs::read(store.backup_path()).unwrap(), backup_before);
    }
}

#[test]
fn t9_verify_failure_leaves_everything_unchanged() {
    let (guard, store, _keys, _version_one, _version_two) = setup_two_saves();
    let main_before = fs::read(store.main_path()).unwrap();
    let backup_before = fs::read(store.backup_path()).unwrap();
    let bytes = b"verify test bytes".to_vec();

    let mut verify_false = |_read: &[u8]| false;
    assert_eq!(
        store
            .write_atomic(&bytes, &mut verify_false, BackupPolicy::KeepBackup)
            .unwrap_err(),
        NotesError::VerifyFailed
    );
    assert_eq!(fs::read(store.main_path()).unwrap(), main_before);
    assert_eq!(fs::read(store.backup_path()).unwrap(), backup_before);
    assert_eq!(
        file_names(&guard.path),
        vec![MAIN_FILE_NAME.to_string(), BACKUP_FILE_NAME.to_string()]
    );

    let new_bytes = b"accepted bytes".to_vec();
    let mut seen = Vec::new();
    {
        let mut verify_true = |read: &[u8]| {
            seen = read.to_vec();
            true
        };
        store
            .write_atomic(&new_bytes, &mut verify_true, BackupPolicy::KeepBackup)
            .unwrap();
    }
    assert_eq!(seen, new_bytes);
    assert_eq!(fs::read(store.main_path()).unwrap(), new_bytes);
}

#[test]
fn t10_scripted_failures() {
    // (a) moving the main file to the backup fails
    {
        let (_guard, real, keys, _version_one, _version_two) = setup_two_saves();
        let main_before = fs::read(real.main_path()).unwrap();
        let backup_before = fs::read(real.backup_path()).unwrap();
        let scripted = scripted_store(real.dir(), vec![1], false);

        assert_eq!(
            scripted
                .save(&keys, &data_with("new version"), true)
                .unwrap_err(),
            NotesError::Io
        );
        assert_eq!(fs::read(real.main_path()).unwrap(), main_before);
        assert_eq!(fs::read(real.backup_path()).unwrap(), backup_before);
        assert_eq!(
            file_names(real.dir()),
            vec![MAIN_FILE_NAME.to_string(), BACKUP_FILE_NAME.to_string()]
        );
    }

    // (b) moving the new file into place fails: the rollback restores main
    {
        let (_guard, real, keys, _version_one, version_two) = setup_two_saves();
        let main_before = fs::read(real.main_path()).unwrap();
        let scripted = scripted_store(real.dir(), vec![2], false);

        assert_eq!(
            scripted
                .save(&keys, &data_with("new version"), true)
                .unwrap_err(),
            NotesError::Io
        );
        assert_eq!(fs::read(real.main_path()).unwrap(), main_before);
        let loaded = real.load(WORDS_A).unwrap();
        assert_eq!(loaded.source, LoadSource::Main);
        assert!(same_data(&loaded.data, &version_two));
        assert_eq!(file_names(real.dir()), vec![MAIN_FILE_NAME.to_string()]);
    }

    // (c) KeepBackup: moving the new file into place fails
    {
        let (_guard, real, keys, _version_one, _version_two) = setup_two_saves();
        let main_before = fs::read(real.main_path()).unwrap();
        let backup_before = fs::read(real.backup_path()).unwrap();
        let scripted = scripted_store(real.dir(), vec![1], false);

        assert_eq!(
            scripted
                .save(&keys, &data_with("new version"), false)
                .unwrap_err(),
            NotesError::Io
        );
        assert_eq!(fs::read(real.main_path()).unwrap(), main_before);
        assert_eq!(fs::read(real.backup_path()).unwrap(), backup_before);
        assert_eq!(
            file_names(real.dir()),
            vec![MAIN_FILE_NAME.to_string(), BACKUP_FILE_NAME.to_string()]
        );
    }
}

#[test]
fn t11_renames_are_retried() {
    let (_guard, real, keys, _version_one, _version_two) = setup_two_saves();
    let main_before = fs::read(real.main_path()).unwrap();
    let version_three = data_with("version three");
    let scripted = NotesStore::with_ops(
        real.dir().to_path_buf(),
        Box::new(ScriptedFs {
            calls: AtomicUsize::new(0),
            fail_rename_calls: vec![1, 2],
            fail_remove: false,
        }),
        RetryPolicy {
            attempts: 5,
            delay: Duration::ZERO,
        },
    );

    let report = scripted.save(&keys, &version_three, true).unwrap();
    assert!(report.old_backup_gone);
    assert_eq!(fs::read(real.backup_path()).unwrap(), main_before);
    let loaded = real.load(WORDS_A).unwrap();
    assert_eq!(loaded.source, LoadSource::Main);
    assert!(same_data(&loaded.data, &version_three));
}

#[test]
fn t12_change_passphrase() {
    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    let version_one = data_with("version one");
    let version_two = data_with("version two");

    let keys_a = store
        .create_new(WORDS_A, &settings(), &version_one)
        .unwrap();
    store.save(&keys_a, &version_two, true).unwrap();

    let (keys_b, report) = store
        .change_passphrase(WORDS_B, &settings(), &version_two)
        .unwrap();
    assert!(report.old_backup_gone);

    let loaded_b = store.load(WORDS_B).unwrap();
    assert_eq!(loaded_b.source, LoadSource::Main);
    assert!(same_data(&loaded_b.data, &version_two));
    assert_eq!(
        store.load(WORDS_A).unwrap_err(),
        NotesError::WrongPassphraseOrHeader
    );

    let main_after = fs::read(store.main_path()).unwrap();
    let backup_after = fs::read(store.backup_path()).unwrap();
    assert_eq!(backup_after, main_after);
    assert!(crypto::open(&backup_after, WORDS_B).is_ok());
    assert_eq!(
        crypto::open(&backup_after, WORDS_A).err(),
        Some(NotesError::WrongPassphraseOrHeader)
    );

    let version_three = data_with("version three");
    store.save(&keys_b, &version_three, true).unwrap();
    assert_eq!(fs::read(store.backup_path()).unwrap(), main_after);
    assert!(same_data(
        &store.load(WORDS_B).unwrap().data,
        &version_three
    ));
}

#[test]
fn t13_change_passphrase_failures() {
    // (a) the backup replacement fails, the old backup is removed
    {
        let (_guard, real, _keys, _version_one, version_two) = setup_two_saves();
        let scripted = scripted_store(real.dir(), vec![2], false);
        let (_keys_b, report) = scripted
            .change_passphrase(WORDS_B, &settings(), &version_two)
            .unwrap();
        assert!(report.old_backup_gone);
        assert!(!real.backup_path().exists());
        assert!(crypto::open(&fs::read(real.main_path()).unwrap(), WORDS_B).is_ok());
        assert_eq!(real.load(WORDS_B).unwrap().source, LoadSource::Main);
    }

    // (b) both the replacement and the removal fail: the old backup stays
    {
        let (_guard, real, _keys, _version_one, version_two) = setup_two_saves();
        let old_backup = fs::read(real.backup_path()).unwrap();
        let scripted = scripted_store(real.dir(), vec![2], true);
        let (_keys_b, report) = scripted
            .change_passphrase(WORDS_B, &settings(), &version_two)
            .unwrap();
        assert!(!report.old_backup_gone);
        let backup = fs::read(real.backup_path()).unwrap();
        assert_eq!(backup, old_backup);
        assert!(crypto::open(&backup, WORDS_A).is_ok());
        assert_eq!(
            crypto::open(&backup, WORDS_B).err(),
            Some(NotesError::WrongPassphraseOrHeader)
        );
    }

    // (c) the new main file cannot be put in place
    {
        let (_guard, real, _keys, _version_one, version_two) = setup_two_saves();
        let main_before = fs::read(real.main_path()).unwrap();
        let backup_before = fs::read(real.backup_path()).unwrap();
        let scripted = scripted_store(real.dir(), vec![1], false);
        assert_eq!(
            scripted
                .change_passphrase(WORDS_B, &settings(), &version_two)
                .unwrap_err(),
            NotesError::Io
        );
        assert_eq!(fs::read(real.main_path()).unwrap(), main_before);
        assert_eq!(fs::read(real.backup_path()).unwrap(), backup_before);
        assert_eq!(
            file_names(real.dir()),
            vec![MAIN_FILE_NAME.to_string(), BACKUP_FILE_NAME.to_string()]
        );
    }
}

#[test]
fn t14_stale_temporary_files_are_removed() {
    let guard = TempDir::new();
    let old_temp = guard.path.join("secure-notes.erxnotes.aaaaaaaa.erxtmp");
    let fresh_temp = guard.path.join("secure-notes.erxnotes.bbbbbbbb.erxtmp");
    let other = guard.path.join("other.erxtmp");
    fs::write(&old_temp, b"old").unwrap();
    fs::write(&fresh_temp, b"fresh").unwrap();
    fs::write(&other, b"other").unwrap();
    set_old(&old_temp);
    set_old(&other);

    let store = NotesStore::new(guard.path.clone());
    assert_eq!(store.cleanup_stale_temp(), 1);
    assert!(!old_temp.exists());
    assert!(fresh_temp.exists());
    assert!(other.exists());

    let data = data_with("cleanup save");
    let keys = store.create_new(WORDS_A, &settings(), &data).unwrap();
    fs::write(&old_temp, b"old again").unwrap();
    set_old(&old_temp);
    store
        .save(&keys, &data_with("cleanup save two"), true)
        .unwrap();
    assert!(!old_temp.exists());
}

#[test]
fn t15_too_large_notes_change_nothing() {
    let (guard, store, keys, _version_one, _version_two) = setup_two_saves();
    let before = snapshot(&guard.path);

    let mut huge = NotesData::new_empty();
    let body = "x".repeat(MAX_NOTE_BODY_BYTES);
    for index in 0..17 {
        huge.add_note(&format!("note {index}"), &body, 1_700_000_000)
            .unwrap();
    }

    assert_eq!(
        store.save(&keys, &huge, true).unwrap_err(),
        NotesError::NotesTooLarge
    );
    assert_eq!(snapshot(&guard.path), before);
}

#[test]
fn t16_exists_checks_both_names() {
    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    assert!(!store.exists());

    let missing = guard.path.join("missing");
    assert!(!NotesStore::new(missing).exists());

    fs::write(store.main_path(), b"main").unwrap();
    assert!(store.exists());

    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    fs::write(store.backup_path(), b"backup").unwrap();
    assert!(store.exists());
}

#[test]
fn t17_load_never_writes() {
    // A normal load from the main file.
    {
        let (guard, store, _keys, _version_one, _version_two) = setup_two_saves();
        let before = snapshot(&guard.path);
        assert_eq!(store.load(WORDS_A).unwrap().source, LoadSource::Main);
        assert_eq!(snapshot(&guard.path), before);
    }

    // A load from the backup.
    {
        let (guard, store, _keys, _version_one, _version_two) = setup_two_saves();
        corrupt_body(&store.main_path());
        let before = snapshot(&guard.path);
        assert_eq!(store.load(WORDS_A).unwrap().source, LoadSource::Backup);
        assert_eq!(snapshot(&guard.path), before);
    }

    // A wrong-words attempt.
    {
        let (guard, store, _keys, _version_one, _version_two) = setup_two_saves();
        let before = snapshot(&guard.path);
        assert_eq!(
            store.load(WORDS_B).unwrap_err(),
            NotesError::WrongPassphraseOrHeader
        );
        assert_eq!(snapshot(&guard.path), before);
    }
}

#[test]
fn t18_no_temporary_files_are_left() {
    let guard = TempDir::new();
    let store = NotesStore::new(guard.path.clone());
    let keys = store
        .create_new(WORDS_A, &settings(), &data_with("one"))
        .unwrap();
    for index in 0..3 {
        store
            .save(&keys, &data_with(&format!("save {index}")), true)
            .unwrap();
    }
    let _ = store
        .change_passphrase(WORDS_B, &settings(), &data_with("after"))
        .unwrap();

    assert_eq!(
        file_names(&guard.path),
        vec![MAIN_FILE_NAME.to_string(), BACKUP_FILE_NAME.to_string()]
    );
}
