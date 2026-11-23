//! Tests of the Secure Notes window glue: the pure helpers first, then scans
//! of the sources that wire them in. No Tauri app, no window, no filesystem.

use std::path::PathBuf;

use super::commands::NotesCommandError;
use super::hardening::HardenError;
use super::openflow::OpenError;
use super::window::{
    check_opener_label, may_focus, open_error_to_command_error, release_and_hand_over,
};
use super::windowgate::WindowGate;

const PROFILE_DIR: &str = "erx-notes-web-0123456789abcdef";

#[test]
fn q1_opener_label() {
    assert!(check_opener_label("main"));
    for label in ["notes", "", "Main", "main ", "other"] {
        assert!(!check_opener_label(label), "{label:?}");
    }
}

#[test]
fn q2_may_focus() {
    let gate = WindowGate::new();
    assert!(!may_focus(&gate));

    gate.begin_open().unwrap();
    assert!(!may_focus(&gate));

    assert!(gate.attach_dir(PathBuf::from(PROFILE_DIR)));
    assert!(!may_focus(&gate));

    assert!(gate.finish_open());
    assert!(may_focus(&gate));
}

#[test]
fn q3_release_and_hand_over() {
    let dir = PathBuf::from(PROFILE_DIR);

    let gate = WindowGate::new();
    let mut calls = Vec::new();
    assert!(!release_and_hand_over(&gate, &mut |path| calls.push(path)));
    assert!(calls.is_empty());

    let gate = WindowGate::new();
    gate.begin_open().unwrap();
    let mut calls = Vec::new();
    assert!(!release_and_hand_over(&gate, &mut |path| calls.push(path)));
    assert!(calls.is_empty());

    let gate = WindowGate::new();
    gate.begin_open().unwrap();
    assert!(gate.attach_dir(dir.clone()));
    let mut calls = Vec::new();
    assert!(release_and_hand_over(&gate, &mut |path| calls.push(path)));
    assert_eq!(calls, vec![dir.clone()]);
    assert!(!gate.is_active());

    let gate = WindowGate::new();
    gate.begin_open().unwrap();
    assert!(gate.attach_dir(dir.clone()));
    assert!(gate.finish_open());
    let mut calls = Vec::new();
    assert!(release_and_hand_over(&gate, &mut |path| calls.push(path)));
    assert_eq!(calls, vec![dir.clone()]);
    assert!(!release_and_hand_over(&gate, &mut |path| calls.push(path)));
    assert_eq!(calls, vec![dir]);
}

#[test]
fn q4_error_mapping() {
    const PROFILE_MESSAGE: &str = "Could not prepare a private window for Secure Notes.";
    const BUILD_MESSAGE: &str = "Could not open the Secure Notes window.";
    const HARDEN_MESSAGE: &str =
        "The Secure Notes window could not be made private, so it was not opened.";
    const SHOW_MESSAGE: &str = "Could not show the Secure Notes window.";
    const INTERRUPTED_MESSAGE: &str = "The Secure Notes window was closed while it was opening.";

    let cases = [
        (
            OpenError::ProfileFailed,
            "window_profile_failed",
            PROFILE_MESSAGE,
        ),
        (OpenError::BuildFailed, "window_build_failed", BUILD_MESSAGE),
        (
            OpenError::NotHardened(HardenError::Unavailable),
            "window_not_hardened",
            HARDEN_MESSAGE,
        ),
        (
            OpenError::NotHardened(HardenError::NotApplied),
            "window_not_hardened",
            HARDEN_MESSAGE,
        ),
        (
            OpenError::NotHardened(HardenError::TimedOut),
            "window_not_hardened",
            HARDEN_MESSAGE,
        ),
        (OpenError::ShowFailed, "window_show_failed", SHOW_MESSAGE),
        (
            OpenError::Interrupted,
            "window_interrupted",
            INTERRUPTED_MESSAGE,
        ),
    ];

    let mut codes = Vec::new();
    for (error, code, message) in cases {
        let mapped = open_error_to_command_error(error);
        assert_eq!(
            mapped,
            NotesCommandError {
                code: code.to_string(),
                message: message.to_string(),
            }
        );
        assert_eq!(mapped.code, code);
        assert_eq!(mapped.message, message);
        assert!(
            mapped
                .code
                .chars()
                .all(|character| character.is_ascii_lowercase() || character == '_'),
            "{}",
            mapped.code
        );
        assert!(mapped.code.starts_with("window_"), "{}", mapped.code);
        assert!(!mapped.message.contains('\\'));
        assert!(!mapped.message.contains(':'));
        assert!(!mapped.message.contains("HRESULT"));
        codes.push(mapped.code);
    }
    let mut sorted = codes.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 5);
    assert_eq!(codes.len(), 7);
}

#[test]
fn q5_source_scan_window() {
    let source = include_str!("window.rs");
    assert_eq!(source.matches("#[tauri::command").count(), 2);
    assert_eq!(source.matches("pub async fn notes_open_window").count(), 1);
    assert_eq!(
        source.matches("pub async fn notes_close_window(").count(),
        1
    );
    assert_eq!(source.matches("State<'_, NotesWindowState>").count(), 1);
    assert_eq!(
        source.matches("check_opener_label(window.label())").count(),
        1
    );
    assert_eq!(
        source.matches("check_window_label(window.label())").count(),
        1
    );
    assert!(source.matches("spawn_blocking").count() >= 1);
    assert_eq!(source.matches("\"notes.html\"").count(), 1);
    for needle in [
        ".incognito(true)",
        ".content_protected(true)",
        ".visible(false)",
        ".general_autofill_enabled(false)",
        ".devtools(false)",
    ] {
        assert_eq!(source.matches(needle).count(), 1, "{needle}");
    }
    assert_eq!(source.matches(".data_directory(").count(), 1);
}

#[test]
fn q6_source_scan_lib() {
    let source = include_str!("../../lib.rs");
    for needle in [
        "tools::notes::window::notes_open_window,",
        "tools::notes::window::notes_close_window,",
        ".manage(tools::notes::window::NotesWindowState::default())",
        "tools::notes::window::init(",
        "tools::notes::window::on_destroyed(",
        "tools::notes::window::on_exit(",
    ] {
        assert_eq!(source.matches(needle).count(), 1, "{needle}");
    }
}

#[test]
fn q7_source_scan_commands() {
    let source = include_str!("commands.rs");
    assert_eq!(
        source
            .matches("pub const NOTES_WINDOW_LABEL: &str = \"notes\";")
            .count(),
        1
    );
    assert_eq!(source.matches("app_local_data_dir()").count(), 1);
    assert_eq!(source.matches("app_data_dir()").count(), 0);
}
