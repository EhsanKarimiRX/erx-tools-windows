//! Web-profile folder tests: random names, the safety check, create, delete
//! with retries, and stale cleanup. Every test uses its own temporary folder.

use std::fs;
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::error::NotesError;
use super::test_support::TempDir;
use super::webprofile::*;

const GOOD_HEX: &str = "0123456789abcdef";
const GOOD_NAME: &str = "erx-notes-web-0123456789abcdef";
const SHORT_DELAY: Duration = Duration::from_millis(10);

fn good_path(base: &Path) -> PathBuf {
    profile_dir_path(base, GOOD_HEX)
}

fn make_junction(link: &Path, target: &Path) {
    let output = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "mklink /J failed (exit {:?}): stdout={} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn p1_random_hex() {
    let first = random_hex(8).unwrap();
    assert_eq!(first.len(), 16);
    assert!(
        first
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    let second = random_hex(8).unwrap();
    assert_ne!(first, second);
    assert_eq!(random_hex(0).unwrap(), "");
}

#[test]
fn p2_path_shape() {
    let guard = TempDir::new();
    assert_eq!(
        profile_dir_path(&guard.path, GOOD_HEX),
        guard.path.join(GOOD_NAME)
    );
}

#[test]
fn p3_safe_accepts() {
    let guard = TempDir::new();
    let path = good_path(&guard.path);
    assert!(is_safe_profile_dir(&path, &guard.path));
    fs::create_dir(&path).unwrap();
    assert!(is_safe_profile_dir(&path, &guard.path));
}

#[test]
fn p4_safe_rejects() {
    let guard = TempDir::new();
    let base = &guard.path;
    let sub = base.join("sub");
    fs::create_dir(&sub).unwrap();
    let cases: Vec<PathBuf> = vec![
        base.clone(),
        sub.join(GOOD_NAME),
        base.parent().unwrap().join(GOOD_NAME),
        PathBuf::from(GOOD_NAME),
        base.join("..\\erx-notes-web-0123456789abcdef"),
        base.join("erx-canary-lab-0123456789abcdef"),
        base.join("erx-notes-web-"),
        base.join("erx-notes-web-0123456789abcde"),
        base.join("erx-notes-web-0123456789abcdef0"),
        base.join("erx-notes-web-ABCDEF0123456789"),
        base.join("erx-notes-web-0123456789abcdeg"),
        base.join("erx-notes-test-0123456789abcdef"),
    ];
    for path in cases {
        assert!(
            !is_safe_profile_dir(&path, base),
            "accepted unsafe path: {}",
            path.display()
        );
    }
}

#[test]
fn p5_safe_rejects_junction() {
    let guard = TempDir::new();
    let target = TempDir::new();
    fs::write(target.path.join("target.txt"), b"target").unwrap();
    let link = good_path(&guard.path);
    make_junction(&link, &target.path);
    assert!(!is_safe_profile_dir(&link, &guard.path));
}

#[test]
fn p6_create_profile_dir() {
    let guard = TempDir::new();
    let first = create_profile_dir(&guard.path).unwrap();
    assert_eq!(first.parent(), Some(guard.path.as_path()));
    assert!(is_safe_profile_dir(&first, &guard.path));
    assert!(first.exists());
    assert!(first.is_dir());
    assert_eq!(fs::read_dir(&first).unwrap().count(), 0);

    let second = create_profile_dir(&guard.path).unwrap();
    assert_ne!(first, second);

    let missing = TempDir::new();
    assert_eq!(
        create_profile_dir(&missing.path.join("missing")),
        Err(NotesError::Io)
    );
}

#[test]
fn p7_delete_removes_tree() {
    let guard = TempDir::new();
    let folder = good_path(&guard.path);
    fs::create_dir_all(folder.join("nested").join("deeper")).unwrap();
    fs::write(folder.join("a.txt"), b"a").unwrap();
    fs::write(folder.join("nested").join("deeper").join("b.txt"), b"b").unwrap();
    assert_eq!(
        delete_profile_dir(&folder, &guard.path, 3, SHORT_DELAY),
        Ok(1)
    );
    assert!(!folder.exists());
}

#[test]
fn p8_delete_refuses_unsafe() {
    let guard = TempDir::new();

    let other = guard.path.join("other-folder");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("keep.txt"), b"keep").unwrap();
    assert_eq!(
        delete_profile_dir(&other, &guard.path, 3, SHORT_DELAY),
        Err(DeleteError::Refused)
    );
    assert!(other.join("keep.txt").exists());

    let sub = guard.path.join("sub");
    fs::create_dir(&sub).unwrap();
    let nested = sub.join(GOOD_NAME);
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("keep.txt"), b"keep").unwrap();
    assert_eq!(
        delete_profile_dir(&nested, &guard.path, 3, SHORT_DELAY),
        Err(DeleteError::Refused)
    );
    assert!(nested.join("keep.txt").exists());

    let target = TempDir::new();
    fs::write(target.path.join("target.txt"), b"target").unwrap();
    let link = good_path(&guard.path);
    make_junction(&link, &target.path);
    assert_eq!(
        delete_profile_dir(&link, &guard.path, 3, SHORT_DELAY),
        Err(DeleteError::Refused)
    );
    assert!(target.path.join("target.txt").exists());
}

#[test]
fn p9_delete_missing() {
    let guard = TempDir::new();
    let missing = profile_dir_path(&guard.path, "fedcba9876543210");
    assert_eq!(
        delete_profile_dir(&missing, &guard.path, 0, Duration::ZERO),
        Ok(0)
    );
}

#[test]
fn p10_delete_attempt_counts() {
    let guard = TempDir::new();
    let folder = good_path(&guard.path);
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("file.txt"), b"file").unwrap();

    assert_eq!(
        delete_profile_dir(&folder, &guard.path, 0, Duration::ZERO),
        Err(DeleteError::NoAttempts)
    );
    assert!(folder.exists());

    assert_eq!(
        delete_profile_dir(&folder, &guard.path, 1, Duration::ZERO),
        Ok(1)
    );
    assert!(!folder.exists());
}

#[test]
fn p11_delete_retries_until_released() {
    let guard = TempDir::new();
    let folder = good_path(&guard.path);
    fs::create_dir(&folder).unwrap();
    let held = folder.join("held.txt");
    fs::write(&held, b"held").unwrap();
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&held)
        .unwrap();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        drop(handle);
    });

    let result = delete_profile_dir(&folder, &guard.path, 20, Duration::from_millis(50));
    releaser.join().unwrap();

    match result {
        Ok(attempt) => assert!(attempt >= 2, "succeeded on attempt {attempt}"),
        other => panic!("expected a retried success, got {other:?}"),
    }
    assert!(!folder.exists());
}

#[test]
fn p12_delete_fails_when_held() {
    let guard = TempDir::new();
    let folder = good_path(&guard.path);
    fs::create_dir(&folder).unwrap();
    let held = folder.join("held.txt");
    fs::write(&held, b"held").unwrap();
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&held)
        .unwrap();

    let result = delete_profile_dir(&folder, &guard.path, 3, SHORT_DELAY);
    assert_eq!(result, Err(DeleteError::Failed));
    assert!(folder.exists());
    drop(handle);
}

#[test]
fn p13_stale_cleanup() {
    let guard = TempDir::new();
    let base = &guard.path;
    let removed = create_profile_dir(base).unwrap();
    fs::write(removed.join("file.txt"), b"file").unwrap();
    let kept = create_profile_dir(base).unwrap();
    let bad_hex = base.join("erx-notes-web-xyz");
    fs::create_dir(&bad_hex).unwrap();
    let canary = base.join("erx-canary-lab-0123456789abcdef");
    fs::create_dir(&canary).unwrap();
    let plain_file = profile_dir_path(base, "fedcba9876543210");
    fs::write(&plain_file, b"file").unwrap();

    let report = cleanup_stale_profiles(base, std::slice::from_ref(&kept), 3, SHORT_DELAY);
    assert_eq!(
        report,
        StaleReport {
            removed: 1,
            failed: 0
        }
    );
    assert!(!removed.exists());
    assert!(kept.exists());
    assert!(bad_hex.exists());
    assert!(canary.exists());
    assert!(plain_file.exists());
}

#[test]
fn p14_stale_cleanup_junction() {
    let guard = TempDir::new();
    let target = TempDir::new();
    fs::write(target.path.join("target.txt"), b"target").unwrap();
    let link = good_path(&guard.path);
    make_junction(&link, &target.path);

    let report = cleanup_stale_profiles(&guard.path, &[], 3, SHORT_DELAY);
    assert_eq!(
        report,
        StaleReport {
            removed: 0,
            failed: 0
        }
    );
    assert!(target.path.join("target.txt").exists());
    assert!(link.exists());
}

#[test]
fn p15_stale_cleanup_missing_base() {
    let guard = TempDir::new();
    let report = cleanup_stale_profiles(&guard.path.join("missing"), &[], 3, SHORT_DELAY);
    assert_eq!(
        report,
        StaleReport {
            removed: 0,
            failed: 0
        }
    );
}

#[test]
fn p16_stale_cleanup_counts_failures() {
    let guard = TempDir::new();
    let removable = create_profile_dir(&guard.path).unwrap();
    fs::write(removable.join("ok.txt"), b"ok").unwrap();
    let held = create_profile_dir(&guard.path).unwrap();
    let held_file = held.join("held.txt");
    fs::write(&held_file, b"held").unwrap();
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&held_file)
        .unwrap();

    let report = cleanup_stale_profiles(&guard.path, &[], 2, SHORT_DELAY);
    assert_eq!(
        report,
        StaleReport {
            removed: 1,
            failed: 1
        }
    );
    assert!(held.exists());
    drop(handle);
}

#[test]
fn p17_delete_constants() {
    assert_eq!(CLOSE_DELETE_ATTEMPTS, 20);
    assert_eq!(CLOSE_DELETE_DELAY, Duration::from_millis(500));
    assert_eq!(EXIT_DELETE_ATTEMPTS, 10);
    assert_eq!(EXIT_DELETE_DELAY, Duration::from_millis(300));
    assert_eq!(STALE_DELETE_ATTEMPTS, 3);
    assert_eq!(STALE_DELETE_DELAY, Duration::from_millis(300));
}
