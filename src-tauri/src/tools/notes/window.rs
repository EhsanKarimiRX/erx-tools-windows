//! The Tauri glue of the Secure Notes window.
//!
//! It owns the window gate and the one-instance guard, opens the hardened
//! window through the order in `openflow`, and removes the WebView2 profile
//! folder when the window or the app goes away. Every failure leaves no window
//! behind, so a broken attempt can never show notes.
//!
//! The frontend page and the permission for this window arrive later; today
//! the commands exist and the wiring is in place, but nothing calls them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};

use super::commands::{NOTES_WINDOW_LABEL, NotesCommandError, check_window_label};
use super::hardening::{HardenError, SettingsReadback, harden_window};
use super::instance::{InstanceGuard, NOTES_INSTANCE_NAME, acquire_instance};
use super::openflow::{OpenError, OpenOps, OpenOutcome, open_with};
use super::webprofile::{
    CLOSE_DELETE_ATTEMPTS, CLOSE_DELETE_DELAY, EXIT_DELETE_ATTEMPTS, EXIT_DELETE_DELAY,
    STALE_DELETE_ATTEMPTS, STALE_DELETE_DELAY, cleanup_stale_profiles, create_profile_dir,
    delete_profile_dir,
};
use super::windowgate::WindowGate;

/// The only window that may open the Secure Notes window.
pub const OPENER_WINDOW_LABEL: &str = "main";

/// True only for the window that may open Secure Notes.
pub fn check_opener_label(label: &str) -> bool {
    label == OPENER_WINDOW_LABEL
}

/// The window may be focused only when it is fully open. A window that is
/// still opening must never be focused or shown early.
pub fn may_focus(gate: &WindowGate) -> bool {
    gate.is_open()
}

/// Releases the gate and hands the profile folder to `hand_over`. Returns
/// false when the gate held no folder.
pub fn release_and_hand_over(gate: &WindowGate, hand_over: &mut dyn FnMut(PathBuf)) -> bool {
    match gate.release() {
        Some(dir) => {
            hand_over(dir);
            true
        }
        None => false,
    }
}

/// Turns an open-flow failure into the error the frontend sees.
pub fn open_error_to_command_error(error: OpenError) -> NotesCommandError {
    let (code, message) = match error {
        OpenError::ProfileFailed => (
            "window_profile_failed",
            "Could not prepare a private window for Secure Notes.",
        ),
        OpenError::BuildFailed => (
            "window_build_failed",
            "Could not open the Secure Notes window.",
        ),
        OpenError::NotHardened(_) => (
            "window_not_hardened",
            "The Secure Notes window could not be made private, so it was not opened.",
        ),
        OpenError::ShowFailed => (
            "window_show_failed",
            "Could not show the Secure Notes window.",
        ),
        OpenError::Interrupted => (
            "window_interrupted",
            "The Secure Notes window was closed while it was opening.",
        ),
    };
    NotesCommandError {
        code: code.to_string(),
        message: message.to_string(),
    }
}

/// The managed state of the Secure Notes window.
#[derive(Default)]
pub struct NotesWindowState {
    gate: Arc<WindowGate>,
    instance: Mutex<Option<InstanceGuard>>,
}

/// What opening the window did.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenWindowResult {
    pub already_open: bool,
}

/// The real world behind the open flow: Tauri windows and the WebView2 profile.
struct TauriOpenOps {
    app: AppHandle,
    gate: Arc<WindowGate>,
    window: Option<WebviewWindow>,
}

impl TauriOpenOps {
    fn new(app: AppHandle, gate: Arc<WindowGate>) -> TauriOpenOps {
        TauriOpenOps {
            app,
            gate,
            window: None,
        }
    }
}

impl OpenOps for TauriOpenOps {
    fn create_profile_dir(&mut self) -> Result<PathBuf, super::error::NotesError> {
        create_profile_dir(&std::env::temp_dir())
    }

    fn build_hidden_window(&mut self, dir: &Path) -> Result<(), ()> {
        let built = tauri::WebviewWindowBuilder::new(
            &self.app,
            NOTES_WINDOW_LABEL,
            tauri::WebviewUrl::App("notes.html".into()),
        )
        .title("Secure Notes")
        .inner_size(900.0, 650.0)
        .min_inner_size(480.0, 360.0)
        .center()
        .visible(false)
        .incognito(true)
        .data_directory(dir.to_path_buf())
        .general_autofill_enabled(false)
        .content_protected(true)
        .devtools(false)
        .build();
        match built {
            Ok(window) => {
                self.window = Some(window);
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    fn harden(&mut self) -> Result<SettingsReadback, HardenError> {
        match &self.window {
            Some(window) => harden_window(window),
            None => Err(HardenError::Unavailable),
        }
    }

    fn show_window(&mut self) -> Result<(), ()> {
        let Some(window) = &self.window else {
            return Err(());
        };
        window.show().map_err(|_| ())?;
        // Focus failures do not undo a shown window.
        let _ = window.set_focus();
        Ok(())
    }

    fn destroy_window(&mut self) {
        if let Some(window) = self.window.take() {
            let _ = window.destroy();
        }
    }

    fn focus_existing(&mut self) {
        if may_focus(&self.gate)
            && let Some(window) = self.app.get_webview_window(NOTES_WINDOW_LABEL)
        {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }

    fn schedule_profile_delete(&mut self, dir: PathBuf) {
        spawn_delete(dir, CLOSE_DELETE_ATTEMPTS, CLOSE_DELETE_DELAY);
    }
}

/// Takes the one-instance guard and removes stale profile folders.
///
/// Only the first running instance may use Secure Notes; every other start
/// returns false and the commands then answer "unavailable".
pub fn init(app: &AppHandle) -> bool {
    let guard = match acquire_instance(NOTES_INSTANCE_NAME) {
        Ok(Some(guard)) => guard,
        Ok(None) | Err(_) => return false,
    };
    let state = app.state::<NotesWindowState>();
    *state
        .instance
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(guard);
    // This runs once at startup, before any window exists, so blocking here
    // is fine and keeps the first open free of old folders.
    let _ = cleanup_stale_profiles(
        &std::env::temp_dir(),
        &[],
        STALE_DELETE_ATTEMPTS,
        STALE_DELETE_DELAY,
    );
    true
}

/// Handles a window that is gone (destroyed or closed).
pub fn on_destroyed(app: &AppHandle, label: &str) {
    if label == NOTES_WINDOW_LABEL {
        let gate = Arc::clone(&app.state::<NotesWindowState>().gate);
        release_and_hand_over(&gate, &mut |dir| {
            spawn_delete(dir, CLOSE_DELETE_ATTEMPTS, CLOSE_DELETE_DELAY);
        });
    } else if label == OPENER_WINDOW_LABEL {
        // Closing the main window must end the app, so the separate notes
        // window is destroyed here too.
        if let Some(window) = app.get_webview_window(NOTES_WINDOW_LABEL) {
            let _ = window.destroy();
        }
    }
    // A late Destroyed event of an old window could release the gate of a
    // quick second open attempt. That only makes the second attempt fail
    // closed (it sees no profile folder to show), which is accepted.
}

/// Deletes a profile folder on its own thread.
///
/// The WebView2 process may still hold files right after a destroy, so the
/// delete retries with a delay. Failing to start the thread is ignored: a
/// leftover folder is removed at the next start.
fn spawn_delete(dir: PathBuf, attempts: u32, delay: Duration) {
    let _ = std::thread::Builder::new()
        .name("notes-web-cleanup".to_string())
        .spawn(move || {
            let _ = delete_profile_dir(&dir, &std::env::temp_dir(), attempts, delay);
        });
}

/// Removes the profile folder when the app exits.
///
/// This is the last chance to remove the folder, so it blocks until the
/// delete finishes. A missing state (early exit) is not an error.
pub fn on_exit(app: &AppHandle) {
    let Some(state) = app.try_state::<NotesWindowState>() else {
        return;
    };
    release_and_hand_over(&state.gate, &mut |dir| {
        let _ = delete_profile_dir(
            &dir,
            &std::env::temp_dir(),
            EXIT_DELETE_ATTEMPTS,
            EXIT_DELETE_DELAY,
        );
    });
}

/// Opens the hardened Secure Notes window, or focuses it when it is open.
#[tauri::command]
pub async fn notes_open_window(
    app: AppHandle,
    window: tauri::Window,
    state: State<'_, NotesWindowState>,
) -> Result<OpenWindowResult, NotesCommandError> {
    if !check_opener_label(window.label()) {
        return Err(NotesCommandError {
            code: "wrong_window".to_string(),
            message: "This window may not use Secure Notes.".to_string(),
        });
    }
    if state
        .instance
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_none()
    {
        return Err(NotesCommandError::unavailable());
    }
    let gate = Arc::clone(&state.gate);
    let app = app.clone();
    // Building a window and talking to WebView2 must not run on the main UI
    // thread, so the whole open flow waits on a blocking thread.
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let mut ops = TauriOpenOps::new(app, Arc::clone(&gate));
        open_with(&gate, &mut ops)
    })
    .await;
    match joined {
        Err(_) => Err(NotesCommandError::internal()),
        Ok(Ok(outcome)) => Ok(OpenWindowResult {
            already_open: outcome == OpenOutcome::AlreadyOpen,
        }),
        Ok(Err(error)) => Err(open_error_to_command_error(error)),
    }
}

/// Closes the Secure Notes window. The profile folder is removed when the
/// Destroyed event arrives.
#[tauri::command]
pub async fn notes_close_window(window: tauri::Window) -> Result<(), NotesCommandError> {
    if !check_window_label(window.label()) {
        return Err(NotesCommandError {
            code: "wrong_window".to_string(),
            message: "This window may not use Secure Notes.".to_string(),
        });
    }
    window.destroy().map_err(|_| NotesCommandError::internal())
}
