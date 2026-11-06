//! Tauri command layer for Awake: the managed runtime (R8: one Mutex every
//! command and the background thread lock), the five commands, the 5-second
//! reconcile thread (R4/R6), the close interception (R7) and best-effort
//! native notifications. All decisions live in `engine.rs`; this file only
//! wires them to the app and keeps its word to the UI (no fake status).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use super::core::{AutoOffReason, AwakeOptions, AwakeStatus, RestoreReport};
use super::engine::{self, ReconcileOutcome};
use super::power::PowerBackend;
use super::state_file;
use crate::tools::ToolError;

#[cfg(windows)]
use super::power::windows::WindowsPower;

// ---------------------------------------------------------------------------
// Runtime (R8)
// ---------------------------------------------------------------------------

/// Managed state: the one lock every Awake operation goes through.
#[derive(Default)]
pub struct AwakeRuntime {
    inner: Mutex<RuntimeInner>,
}

#[derive(Default)]
struct RuntimeInner {
    /// Resolved once in `init`; None means Awake support never started.
    state_dir: Option<PathBuf>,
    /// The app-start reconcile found a still-valid Awake (R4).
    resumed_from_previous_session: bool,
    /// An auto-off that happened before the UI could hear the event (at app
    /// start, or with the window minimized). Delivered once via get_status.
    pending_auto_off: Option<PendingAutoOff>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAutoOff {
    pub reason: AutoOffReason,
    pub message: String,
}

impl AwakeRuntime {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, RuntimeInner>, String> {
        self.inner.lock().map_err(|_| {
            "Awake's internal state is unavailable. Please restart ERX Tools.".to_string()
        })
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| i64::try_from(since.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

/// The real backend, or an honest error off-Windows.
fn backend() -> Result<Box<dyn PowerBackend>, String> {
    #[cfg(windows)]
    return Ok(Box::new(WindowsPower));
    #[cfg(not(windows))]
    return Err("ERX Tools System Manager needs Windows.".into());
}

/// Called once from the app setup: resolve the state directory, run the
/// app-start reconcile (R4) and start the 5-second background thread.
pub fn init(app: &AppHandle) {
    let state_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("Awake is disabled: cannot resolve the app data folder: {error}");
            return;
        }
    };

    let runtime = app.state::<AwakeRuntime>();
    let mut inner = match runtime.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            eprintln!("Awake is disabled: its state is unavailable at startup");
            return;
        }
    };
    inner.state_dir = Some(state_dir);

    // R4: the app-start reconcile decides between "resume as on" and
    // "switch off with a message" before the UI asks for the first status.
    if let Some(dir) = inner.state_dir.clone() {
        let start_result = match backend() {
            Ok(backend) => engine::reconcile(backend.as_ref(), &dir, now_unix(), true),
            Err(error) => Err(error),
        };
        match start_result {
            Ok(ReconcileOutcome::NotOn) => {}
            Ok(ReconcileOutcome::StillOn { resumed, .. }) => {
                inner.resumed_from_previous_session = resumed;
            }
            Ok(ReconcileOutcome::AutoOff {
                reason, message, ..
            }) => {
                inner.pending_auto_off = Some(PendingAutoOff { reason, message });
            }
            Err(error) => eprintln!("Awake app-start reconcile failed: {error}"),
        }
    }
    drop(inner);

    let handle = app.clone();
    std::thread::Builder::new()
        .name("awake-reconcile".into())
        .spawn(move || background_loop(handle))
        .map_err(|error| eprintln!("Awake background thread could not start: {error}"))
        .ok();
}

/// The 5-second background thread: reconcile (R4) + timer expiry (R6).
fn background_loop(app: AppHandle) {
    loop {
        std::thread::sleep(Duration::from_secs(5));
        let runtime = app.state::<AwakeRuntime>();
        let outcome = {
            let Ok(inner) = runtime.inner.lock() else {
                continue;
            };
            let Some(dir) = inner.state_dir.clone() else {
                continue;
            };
            let Ok(backend) = backend() else { continue };
            engine::reconcile(backend.as_ref(), &dir, now_unix(), false)
        };
        match outcome {
            Ok(ReconcileOutcome::AutoOff {
                reason,
                message,
                report,
            }) => {
                {
                    let Ok(mut inner) = runtime.inner.lock() else {
                        continue;
                    };
                    inner.resumed_from_previous_session = false;
                    inner.pending_auto_off = Some(PendingAutoOff {
                        reason,
                        message: message.clone(),
                    });
                }
                let _ = app.emit("awake://auto-off", PendingAutoOff { reason, message });
                notify(&app, &auto_off_notification_body(reason));
                for failure in &report.failed {
                    eprintln!("Awake auto-off restore failure: {failure}");
                }
            }
            Ok(_) => {}
            Err(error) => eprintln!("Awake reconcile failed: {error}"),
        }
    }
}

/// Notification text for the two timer cases (the manual-change case happens
/// with the app open, where the in-app banner is the required channel).
fn auto_off_notification_body(reason: AutoOffReason) -> String {
    match reason {
        AutoOffReason::TimerExpired => "Awake timer finished. Your settings were restored.".into(),
        AutoOffReason::TimerExpiredWhileClosed => {
            "The Awake timer finished while ERX Tools was closed. Your settings were restored."
                .into()
        }
        AutoOffReason::ManualChange => {
            "Awake was switched off because the sleep or screen settings were changed manually in Windows. Your changes were kept.".into()
        }
        AutoOffReason::SchemeGone => {
            "Awake was switched off because the Windows power plan it changed no longer exists."
                .into()
        }
        AutoOffReason::StateFileDamaged => {
            "Awake's state file was damaged and has been reset. Your power settings were not touched by Awake."
                .into()
        }
    }
}

/// Best-effort native Windows notification; failures are logged, never fatal.
fn notify(app: &AppHandle, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(error) = app
        .notification()
        .builder()
        .title("ERX Tools")
        .body(body)
        .show()
    {
        eprintln!("Awake notification could not be shown: {error}");
    }
}

fn emit_changed(app: &AppHandle, status: &AwakeStatus) {
    if let Err(error) = app.emit("awake://changed", status) {
        eprintln!("Awake status event could not be sent: {error}");
    }
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwakeOptionsDto {
    pub keep_screen_on: bool,
    pub ends_at_unix: Option<i64>,
}

impl From<AwakeOptionsDto> for AwakeOptions {
    fn from(dto: AwakeOptionsDto) -> Self {
        AwakeOptions {
            keep_screen_on: dto.keep_screen_on,
            ends_at_unix: dto.ends_at_unix,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub status: AwakeStatus,
    pub pending_auto_off: Option<PendingAutoOff>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisableResponse {
    pub status: AwakeStatus,
    pub report: RestoreReport,
}

fn internal(message: String) -> ToolError {
    ToolError::Internal(message)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Current status plus any auto-off message the UI has not seen yet (R4).
/// Runs a reconcile so manual changes made while the window had no focus are
/// caught when it regains focus.
#[tauri::command]
pub fn awake_get_status(runtime: State<AwakeRuntime>) -> Result<StatusResponse, ToolError> {
    let mut inner = runtime.lock().map_err(internal)?;
    let dir = state_dir(&inner)?;
    let backend = backend().map_err(internal)?;

    let outcome = engine::reconcile(backend.as_ref(), &dir, now_unix(), false).map_err(internal)?;
    let (status, resumed) = match outcome {
        ReconcileOutcome::NotOn => (engine::status_off(), false),
        ReconcileOutcome::StillOn { resumed, status } => (status, resumed),
        ReconcileOutcome::AutoOff {
            reason, message, ..
        } => {
            inner.pending_auto_off = Some(PendingAutoOff { reason, message });
            (engine::status_off(), false)
        }
    };
    let resumed_flag = resumed || inner.resumed_from_previous_session;
    inner.resumed_from_previous_session = resumed_flag && status.state == "on";
    let pending_auto_off = inner.pending_auto_off.take();
    Ok(StatusResponse {
        status: AwakeStatus {
            resumed_from_previous_session: resumed_flag,
            ..status
        },
        pending_auto_off,
    })
}

fn state_dir(inner: &RuntimeInner) -> Result<PathBuf, ToolError> {
    inner.state_dir.clone().ok_or_else(|| {
        internal("System Manager could not start Awake support (no app data folder).".into())
    })
}

/// Turns Awake on (R1 + R2); the options come from the drawer.
#[tauri::command]
pub fn awake_enable(
    app: AppHandle,
    runtime: State<AwakeRuntime>,
    options: AwakeOptionsDto,
) -> Result<AwakeStatus, ToolError> {
    let mut inner = runtime.lock().map_err(internal)?;
    let dir = state_dir(&inner)?;
    let backend = backend().map_err(internal)?;
    let status =
        engine::enable(backend.as_ref(), &dir, options.into(), now_unix()).map_err(internal)?;
    inner.resumed_from_previous_session = false;
    inner.pending_auto_off = None;
    drop(inner);
    emit_changed(&app, &status);
    Ok(status)
}

/// Turns Awake off (R3) and returns the honest restore report. A partial
/// failure keeps the state file, so the status stays "on" and the next
/// reconcile retries the keys that failed.
#[tauri::command]
pub fn awake_disable(
    app: AppHandle,
    runtime: State<AwakeRuntime>,
) -> Result<DisableResponse, ToolError> {
    let mut inner = runtime.lock().map_err(internal)?;
    let dir = state_dir(&inner)?;
    let backend = backend().map_err(internal)?;
    let (status, report) = engine::disable(backend.as_ref(), &dir).map_err(internal)?;
    if status.state == "off" {
        inner.resumed_from_previous_session = false;
    }
    drop(inner);
    emit_changed(&app, &status);
    Ok(DisableResponse { status, report })
}

/// Applies drawer changes while Awake is on (R5); the frontend calls this on
/// every change, no Apply button.
#[tauri::command]
pub fn awake_set_options(
    app: AppHandle,
    runtime: State<AwakeRuntime>,
    options: AwakeOptionsDto,
) -> Result<AwakeStatus, ToolError> {
    let inner = runtime.lock().map_err(internal)?;
    let dir = state_dir(&inner)?;
    let backend = backend().map_err(internal)?;
    let status = engine::set_options(backend.as_ref(), &dir, options.into()).map_err(internal)?;
    drop(inner);
    emit_changed(&app, &status);
    Ok(status)
}

/// The close-dialog decision (R7). Only the close flow uses this.
#[tauri::command]
pub fn awake_exit(
    app: AppHandle,
    runtime: State<AwakeRuntime>,
    choice: String,
) -> Result<(), ToolError> {
    match choice.as_str() {
        "restore_and_close" => {
            let mut restore_incomplete: Option<String> = None;
            {
                let inner = runtime.lock().map_err(internal)?;
                let dir = state_dir(&inner)?;
                let backend = backend().map_err(internal)?;
                match engine::disable(backend.as_ref(), &dir) {
                    Ok((status, _report)) if status.state == "off" => {}
                    Ok((_status, report)) => {
                        restore_incomplete = Some(report.failed.join(" "));
                    }
                    Err(error) => restore_incomplete = Some(error),
                }
            }
            if let Some(detail) = restore_incomplete {
                // The user asked to close; the state file stays so the next
                // launch retries the restore (R4). Say so before exiting.
                notify(
                    &app,
                    "Awake could not restore every setting right now. ERX Tools will finish the restore the next time it starts.",
                );
                eprintln!("Awake restore on exit incomplete: {detail}");
            }
            app.exit(0);
            Ok(())
        }
        "keep_and_close" => {
            app.exit(0);
            Ok(())
        }
        "cancel" => Ok(()),
        other => Err(internal(format!("Unknown close choice '{other}'."))),
    }
}

/// Quick "is Awake on?" check for the window-close interception (R7): a
/// state file means Awake is on (reconcile handles stale files later).
pub fn is_on_for_close_check(app: &AppHandle) -> bool {
    let runtime = app.state::<AwakeRuntime>();
    let Ok(inner) = runtime.inner.lock() else {
        return false;
    };
    let Some(dir) = inner.state_dir.clone() else {
        return false;
    };
    matches!(state_file::load(&state_file::state_path(&dir)), Ok(Some(_)))
}
