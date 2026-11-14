use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::engine::EncryptSettings;
use super::error::VaultError;
use super::files::{default_output_path, encrypt_file};
use super::kdf::KdfParams;
use super::parts::{PartSize, random_token, temp_path};
use super::parts_test::TestDir;
use super::runtime::{
    BusyGuard, DecryptJob, DecryptOutcome, EncryptJob, EncryptOutcome, KDF_MEMORY_KIB, Phase,
    Throttle, VaultEvent, cached_kdf, cleanup_stale_temp, count_parts, is_temp_name,
    params_for_pass_time, parse_part_size, plan_decrypt, plan_encrypt, run_decrypt, run_encrypt,
    tune_kdf,
};
use super::verify::verify_file;
use super::wordlist;

fn phrase_from(skip: usize) -> String {
    wordlist::words()
        .iter()
        .skip(skip)
        .take(8)
        .copied()
        .collect::<Vec<&str>>()
        .join(" ")
}

fn passphrase() -> String {
    phrase_from(0)
}

/// Deterministic, practically incompressible test data.
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    let mut out = Vec::with_capacity(len + 8);
    while out.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.extend_from_slice(&state.to_le_bytes());
    }
    out.truncate(len);
    out
}

fn names(dir: &Path) -> Vec<String> {
    let mut all: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    all.sort();
    all
}

/// Writes a source file and builds an (unsplit) encryption job for it.
fn make_job(dir: &TestDir, name: &str, size: usize, verify: bool) -> (EncryptJob, Vec<u8>) {
    let data = noise(size, 21);
    let source = dir.path().join(name);
    fs::write(&source, &data).unwrap();
    let job = EncryptJob {
        source,
        out_dir: dir.path().to_path_buf(),
        part_size: PartSize::Off,
        overwrite: false,
        verify,
    };
    (job, data)
}

fn finished(job: &EncryptJob, cancel: &AtomicBool) -> EncryptOutcome {
    run_encrypt(
        job,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        cancel,
        &mut |_event: VaultEvent| {},
    )
    .unwrap()
}

#[test]
fn events_have_the_shape_the_ui_expects() {
    assert_eq!(
        serde_json::to_string(&VaultEvent::Progress { done: 1, total: 2 }).unwrap(),
        r#"{"event":"progress","done":1,"total":2}"#
    );
    assert_eq!(
        serde_json::to_string(&VaultEvent::Phase {
            phase: Phase::Deriving
        })
        .unwrap(),
        r#"{"event":"phase","phase":"deriving"}"#
    );
    assert_eq!(
        serde_json::to_string(&VaultEvent::Phase {
            phase: Phase::Tuning
        })
        .unwrap(),
        r#"{"event":"phase","phase":"tuning"}"#
    );
}

#[test]
fn part_size_names_are_mapped() {
    assert_eq!(parse_part_size("off"), Some(PartSize::Off));
    assert_eq!(parse_part_size("500mb"), Some(PartSize::Mib500));
    assert_eq!(parse_part_size("1gb"), Some(PartSize::Gib1));
    assert_eq!(parse_part_size("2gb"), Some(PartSize::Gib2));
    assert_eq!(parse_part_size("3gb"), None);
    assert_eq!(parse_part_size(""), None);
}

#[test]
fn pass_count_follows_the_measured_speed_within_bounds() {
    let cases = [
        (Duration::from_millis(10), 10),
        (Duration::from_millis(100), 7),
        (Duration::from_millis(250), 3),
        (Duration::from_millis(400), 2),
        (Duration::from_millis(700), 2),
        (Duration::from_secs(5), 2),
        (Duration::ZERO, 10),
    ];
    for (one_pass, expected) in cases {
        let params = params_for_pass_time(one_pass);
        assert_eq!(params.t_cost, expected, "pass time {one_pass:?}");
        assert_eq!(params.m_cost_kib, KDF_MEMORY_KIB);
        assert_eq!(params.p_cost, 1);
        assert!(EncryptSettings::new(true, params).is_ok());
    }
}

#[test]
fn real_tuning_gives_allowed_parameters_and_is_cached() {
    let params = tune_kdf().unwrap();
    assert_eq!(params.m_cost_kib, KDF_MEMORY_KIB);
    assert!((2..=10).contains(&params.t_cost));
    assert!(EncryptSettings::new(true, params).is_ok());

    let cache = Mutex::new(None);
    let first = cached_kdf(&cache).unwrap();
    assert_eq!(*cache.lock().unwrap(), Some(first));
    assert_eq!(cached_kdf(&cache).unwrap(), first);

    let prefilled = Mutex::new(Some(KdfParams::for_tests()));
    assert_eq!(cached_kdf(&prefilled).unwrap(), KdfParams::for_tests());
}

#[test]
fn only_one_operation_can_hold_the_busy_flag() {
    let flag = Arc::new(AtomicBool::new(false));
    let first = BusyGuard::acquire(&flag);
    assert!(first.is_some());
    assert!(flag.load(Ordering::SeqCst));
    assert!(BusyGuard::acquire(&flag).is_none());
    drop(first);
    assert!(!flag.load(Ordering::SeqCst));
    assert!(BusyGuard::acquire(&flag).is_some());
    assert!(!flag.load(Ordering::SeqCst));
}

#[test]
fn throttle_lets_one_event_through_per_interval() {
    let mut slow = Throttle::new(Duration::from_secs(3600));
    assert!(slow.ready());
    assert!(!slow.ready());
    let mut open = Throttle::new(Duration::ZERO);
    assert!(open.ready());
    assert!(open.ready());
}

#[test]
fn temp_names_are_recognized_strictly() {
    assert!(is_temp_name("doc.bin.erx.0123abcd.erxtmp"));
    assert!(is_temp_name("doc.bin.erx.001.ffffffff.erxtmp"));
    assert!(is_temp_name("restored.txt.DEADBEEF.erxtmp"));
    assert!(!is_temp_name("doc.bin.erx"));
    assert!(!is_temp_name("x.erxtmp"));
    assert!(!is_temp_name("doc.0123abc.erxtmp"));
    assert!(!is_temp_name("doc.0123abcg.erxtmp"));
    assert!(!is_temp_name(".0123abcd.erxtmp"));
    assert!(!is_temp_name("notes.txt"));

    // Names made by the writer itself must be recognized.
    let token = random_token().unwrap();
    let path = temp_path(Path::new("doc.bin.erx.001"), &token);
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(is_temp_name(&name));
}

#[test]
fn cleanup_only_removes_old_files_that_look_like_temp_files() {
    let dir = TestDir::new();
    fs::write(dir.path().join("a.erx.0123abcd.erxtmp"), b"x").unwrap();
    fs::write(dir.path().join("keep.txt"), b"x").unwrap();
    fs::write(dir.path().join("notes.erxtmp"), b"x").unwrap();
    fs::create_dir(dir.path().join("dir.0123abcd.erxtmp")).unwrap();

    // Too young: nothing is removed.
    assert_eq!(cleanup_stale_temp(dir.path(), Duration::from_secs(3600)), 0);
    assert_eq!(names(dir.path()).len(), 4);

    // Old enough: only the matching regular file goes.
    assert_eq!(cleanup_stale_temp(dir.path(), Duration::ZERO), 1);
    assert_eq!(
        names(dir.path()),
        ["dir.0123abcd.erxtmp", "keep.txt", "notes.erxtmp"]
    );

    // A missing folder is not an error.
    let missing = dir.path().join("missing");
    assert_eq!(cleanup_stale_temp(&missing, Duration::ZERO), 0);
}

#[test]
fn encrypt_plan_reports_conflicts_and_advice() {
    let dir = TestDir::new();
    let source = dir.path().join("doc.bin");
    fs::write(&source, noise(1000, 1)).unwrap();

    let plan = plan_encrypt(&source, None, PartSize::Off).unwrap();
    assert_eq!(plan.file_name, "doc.bin");
    assert_eq!(plan.size_bytes, 1000);
    assert_eq!(plan.out_dir.as_path(), dir.path());
    assert_eq!(plan.base_path, dir.path().join("doc.erx"));
    assert!(plan.conflicts.is_empty());
    assert!(plan.compress_by_default);
    assert!(!plan.suggest_split);
    assert!(plan.required_bytes > 1000);

    fs::write(dir.path().join("doc.erx"), b"old").unwrap();
    let plan = plan_encrypt(&source, None, PartSize::Gib1).unwrap();
    assert_eq!(plan.conflicts, vec![dir.path().join("doc.erx")]);

    // Another output folder has no conflict and is used for the name.
    let other = TestDir::new();
    let plan = plan_encrypt(&source, Some(other.path()), PartSize::Off).unwrap();
    assert_eq!(plan.out_dir.as_path(), other.path());
    assert_eq!(plan.base_path, other.path().join("doc.erx"));
    assert!(plan.conflicts.is_empty());

    // Already compressed formats are not compressed by default.
    let photo = dir.path().join("pic.jpg");
    fs::write(&photo, noise(500, 2)).unwrap();
    assert!(
        !plan_encrypt(&photo, None, PartSize::Off)
            .unwrap()
            .compress_by_default
    );

    // Bad input.
    let none = dir.path().join("none.txt");
    assert_eq!(
        plan_encrypt(&none, None, PartSize::Off).unwrap_err(),
        VaultError::CannotOpenFile
    );
    assert_eq!(
        plan_encrypt(dir.path(), None, PartSize::Off).unwrap_err(),
        VaultError::CannotOpenFile
    );
    let missing = dir.path().join("nope");
    assert_eq!(
        plan_encrypt(&source, Some(missing.as_path()), PartSize::Off).unwrap_err(),
        VaultError::BadFolder
    );
}

#[test]
fn decrypt_plan_counts_parts_and_detects_existing_output() {
    let dir = TestDir::new();
    let source = dir.path().join("doc.bin");
    fs::write(&source, noise(50_000, 3)).unwrap();
    let base = default_output_path(&source, dir.path()).unwrap();
    let summary = encrypt_file(
        &source,
        &base,
        Some(4096),
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    assert_eq!(summary.parts.len(), 13);

    let plan = plan_decrypt(&summary.parts[0], None).unwrap();
    assert_eq!(plan.file_name, "doc.bin.erx.001");
    assert_eq!(plan.part_count, 13);
    assert_eq!(plan.total_bytes, summary.output_bytes - 13 * 28);
    assert_eq!(plan.output_path, dir.path().join("doc.bin"));
    // The original is still next to the encrypted parts.
    assert!(plan.output_exists);

    let other = TestDir::new();
    let plan = plan_decrypt(&summary.parts[0], Some(other.path())).unwrap();
    assert_eq!(plan.output_path, other.path().join("doc.bin"));
    assert!(!plan.output_exists);

    assert_eq!(count_parts(&summary.parts[0]), 13);
    assert_eq!(count_parts(Path::new("whatever.erx")), 1);
    assert_eq!(
        plan_decrypt(&summary.parts[1], None).unwrap_err(),
        VaultError::NotFirstPart
    );
}

#[test]
fn encrypt_and_decrypt_jobs_round_trip_with_events() {
    let dir = TestDir::new();
    let out = TestDir::new();
    let (job, data) = make_job(&dir, "doc.bin", 600_000, true);
    let cancel = AtomicBool::new(false);

    let mut events: Vec<VaultEvent> = Vec::new();
    let outcome = run_encrypt(
        &job,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        &cancel,
        &mut |event: VaultEvent| events.push(event),
    )
    .unwrap();
    let EncryptOutcome::Done {
        parts,
        input_bytes,
        output_bytes,
        verified,
    } = outcome
    else {
        panic!("encryption did not finish");
    };
    assert_eq!(parts, vec![dir.path().join("doc.erx")]);
    assert_eq!(input_bytes, 600_000);
    assert_eq!(output_bytes, fs::metadata(&parts[0]).unwrap().len());
    assert!(verified);
    assert_eq!(
        events.first(),
        Some(&VaultEvent::Phase {
            phase: Phase::Deriving
        })
    );
    assert!(events.contains(&VaultEvent::Phase {
        phase: Phase::Verifying
    }));
    assert!(events.contains(&VaultEvent::Progress {
        done: 600_000,
        total: 600_000
    }));

    let decrypt_job = DecryptJob {
        first_part: parts[0].clone(),
        out_dir: out.path().to_path_buf(),
        overwrite: false,
    };
    let mut events: Vec<VaultEvent> = Vec::new();
    let outcome = run_decrypt(
        &decrypt_job,
        &passphrase(),
        &cancel,
        &mut |event: VaultEvent| events.push(event),
    )
    .unwrap();
    assert_eq!(
        outcome,
        DecryptOutcome::Done {
            output_path: out.path().join("doc.bin"),
            output_bytes: 600_000
        }
    );
    assert_eq!(fs::read(out.path().join("doc.bin")).unwrap(), data);
    assert_eq!(
        events.first(),
        Some(&VaultEvent::Phase {
            phase: Phase::Deriving
        })
    );
    assert!(matches!(
        events.last(),
        Some(VaultEvent::Progress { done, total }) if done == total
    ));
}

#[test]
fn cancelled_jobs_leave_nothing_behind() {
    let dir = TestDir::new();
    let (job, _data) = make_job(&dir, "big.bin", 2_300_000, false);

    let cancel = AtomicBool::new(true);
    assert_eq!(finished(&job, &cancel), EncryptOutcome::Cancelled);
    assert_eq!(names(dir.path()), ["big.bin"]);

    // Cancel while decrypting a multi-chunk file.
    cancel.store(false, Ordering::Relaxed);
    let EncryptOutcome::Done { parts, .. } = finished(&job, &cancel) else {
        panic!("encryption did not finish");
    };
    cancel.store(true, Ordering::Relaxed);
    let out = TestDir::new();
    let decrypt_job = DecryptJob {
        first_part: parts[0].clone(),
        out_dir: out.path().to_path_buf(),
        overwrite: false,
    };
    let outcome = run_decrypt(
        &decrypt_job,
        &passphrase(),
        &cancel,
        &mut |_event: VaultEvent| {},
    )
    .unwrap();
    assert_eq!(outcome, DecryptOutcome::Cancelled);
    assert!(names(out.path()).is_empty());
}

#[test]
fn wrong_words_fail_without_leaving_output() {
    let dir = TestDir::new();
    let out = TestDir::new();
    let (job, _data) = make_job(&dir, "doc.bin", 100_000, false);
    let cancel = AtomicBool::new(false);
    let EncryptOutcome::Done { parts, .. } = finished(&job, &cancel) else {
        panic!("encryption did not finish");
    };
    let decrypt_job = DecryptJob {
        first_part: parts[0].clone(),
        out_dir: out.path().to_path_buf(),
        overwrite: false,
    };
    let error = run_decrypt(
        &decrypt_job,
        &phrase_from(100),
        &cancel,
        &mut |_event: VaultEvent| {},
    )
    .unwrap_err();
    assert_eq!(error, VaultError::WrongPassphraseOrHeader);
    assert!(names(out.path()).is_empty());
}

#[test]
fn verify_accepts_good_files_and_rejects_changed_ones() {
    let dir = TestDir::new();
    let (job, _data) = make_job(&dir, "doc.bin", 100_000, false);
    let cancel = AtomicBool::new(false);
    let EncryptOutcome::Done {
        parts, verified, ..
    } = finished(&job, &cancel)
    else {
        panic!("encryption did not finish");
    };
    assert!(!verified);
    assert!(
        verify_file(&parts[0], &passphrase(), &mut |_done: u64, _total: u64| {
            true
        })
        .is_ok()
    );
    assert_eq!(
        verify_file(
            &parts[0],
            &phrase_from(100),
            &mut |_done: u64, _total: u64| true
        )
        .unwrap_err(),
        VaultError::WrongPassphraseOrHeader
    );

    let mut bytes = fs::read(&parts[0]).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    fs::write(&parts[0], &bytes).unwrap();
    assert_eq!(
        verify_file(&parts[0], &passphrase(), &mut |_done: u64, _total: u64| {
            true
        })
        .unwrap_err(),
        VaultError::CorruptedData
    );
}
