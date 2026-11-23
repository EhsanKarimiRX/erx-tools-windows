use std::path::PathBuf;

use super::windowgate::{GateBusy, WindowGate};

fn dir() -> PathBuf {
    PathBuf::from("erx-notes-web-0123456789abcdef")
}

#[test]
fn g1_new_is_closed() {
    let gate = WindowGate::new();
    assert!(!gate.is_active());
    assert!(!gate.is_open());
    assert_eq!(gate.release(), None);
}

#[test]
fn g2_begin_open_reserves() {
    let gate = WindowGate::new();
    assert_eq!(gate.begin_open(), Ok(()));
    assert!(gate.is_active());
    assert!(!gate.is_open());
    assert_eq!(gate.begin_open(), Err(GateBusy));
}

#[test]
fn g3_attach_and_finish() {
    let gate = WindowGate::new();
    assert_eq!(gate.begin_open(), Ok(()));
    assert!(gate.attach_dir(dir()));
    assert!(gate.finish_open());
    assert!(gate.is_open());
    assert_eq!(gate.begin_open(), Err(GateBusy));
}

#[test]
fn g4_attach_needs_opening() {
    let gate = WindowGate::new();
    assert!(!gate.attach_dir(dir()));
    assert!(!gate.is_active());
    assert_eq!(gate.release(), None);

    assert_eq!(gate.begin_open(), Ok(()));
    assert!(gate.attach_dir(dir()));
    let second = PathBuf::from("erx-notes-web-fedcba9876543210");
    assert!(!gate.attach_dir(second));
    assert_eq!(gate.release(), Some(dir()));
}

#[test]
fn g5_finish_needs_dir() {
    let gate = WindowGate::new();
    assert_eq!(gate.begin_open(), Ok(()));
    assert!(!gate.finish_open());
    assert!(!gate.is_open());
    assert!(gate.is_active());
}

#[test]
fn g6_release_returns_dir() {
    let gate = WindowGate::new();
    assert_eq!(gate.release(), None);
    assert!(!gate.is_active());

    let open = WindowGate::new();
    assert_eq!(open.begin_open(), Ok(()));
    assert!(open.attach_dir(dir()));
    assert!(open.finish_open());
    assert_eq!(open.release(), Some(dir()));
    assert!(!open.is_active());
    assert_eq!(open.begin_open(), Ok(()));

    let attached = WindowGate::new();
    assert_eq!(attached.begin_open(), Ok(()));
    assert!(attached.attach_dir(dir()));
    assert_eq!(attached.release(), Some(dir()));
    assert!(!attached.is_active());

    let opening = WindowGate::new();
    assert_eq!(opening.begin_open(), Ok(()));
    assert_eq!(opening.release(), None);
    assert!(!opening.is_active());
}

#[test]
fn g7_release_twice() {
    let gate = WindowGate::new();
    assert_eq!(gate.begin_open(), Ok(()));
    assert!(gate.attach_dir(dir()));
    assert!(gate.finish_open());
    assert_eq!(gate.release(), Some(dir()));
    assert_eq!(gate.release(), None);
}

#[test]
fn g8_poison_recovery() {
    let gate = WindowGate::new();
    gate.poison_for_test();
    assert_eq!(gate.begin_open(), Ok(()));
    assert!(gate.attach_dir(dir()));
    assert!(gate.finish_open());
    assert!(gate.is_open());
    assert_eq!(gate.release(), Some(dir()));
    assert!(!gate.is_active());
}
