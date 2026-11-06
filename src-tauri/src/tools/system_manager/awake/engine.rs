//! The Awake engine: how enable, disable, options and reconcile actually
//! behave, written against the `PowerBackend` trait so every rule of the
//! state machine (R1–R6 of docs/PROMPT_5_SPEC.md) is unit-testable with the
//! in-memory fake and a temp state directory. The Tauri command layer in
//! `commands.rs` only wires this to the app.

use std::path::Path;

use super::core::{
    AutoOffReason, AwakeOptions, AwakeStateFile, AwakeStatus, PowerKey, PowerValues,
    ReconcileDecision, RestoreAction, RestoreReport, reconcile_decision, restore_plan,
    timer_expired,
};
use super::power::PowerBackend;
use super::state_file;

/// Enables Awake (R1 + R2): read originals → write the state file → change
/// power values → activate → verify. On any failure everything already
/// changed is rolled back, the state file is deleted, and a plain-English
/// error is returned; a half-applied state is never left silently.
pub fn enable(
    backend: &dyn PowerBackend,
    state_dir: &Path,
    options: AwakeOptions,
    now_unix: i64,
) -> Result<AwakeStatus, String> {
    let path = state_file::state_path(state_dir);
    let (existing, _recovered) = load_recovering(&path)?;
    if existing.is_some() {
        return Err("Awake is already on.".into());
    }
    // A recovered damaged file was already reported by reconcile; enabling
    // proceeds fresh.
    let scheme = backend.active_scheme()?;

    // R1: read all six originals first. A key that cannot be read is
    // recorded as null and is never touched.
    let mut originals = PowerValues::default();
    for key in PowerKey::ALL {
        originals.set(key, backend.read(&scheme, key).ok());
    }

    // The applied plan: every readable key goes to Never (0); the two
    // display keys are skipped when "let the screen turn off normally" is set.
    let mut applied = PowerValues::default();
    for key in PowerKey::ALL {
        if originals.get(key).is_some() && (options.keep_screen_on || !key.is_display()) {
            applied.set(key, Some(0));
        }
    }
    if !applied.sleep_ac.is_some()
        && !applied.sleep_dc.is_some()
        && !applied.hibernate_ac.is_some()
        && !applied.hibernate_dc.is_some()
    {
        return Err(
            "Awake could not read any of the sleep or hibernate timeouts, so it cannot keep this PC awake. Nothing was changed."
                .into(),
        );
    }

    let state = AwakeStateFile {
        version: state_file::STATE_VERSION,
        scheme_guid: scheme.clone(),
        started_at_unix: now_unix,
        ends_at_unix: options.ends_at_unix,
        keep_screen_on: options.keep_screen_on,
        originals,
        applied,
    };

    // R1: the state file is written BEFORE the first power value changes.
    state_file::save(&path, &state)?;

    // R2: apply, activate once, then read every changed key back.
    let mut changed: Vec<PowerKey> = Vec::new();
    for key in PowerKey::ALL {
        if let Some(value) = state.applied.get(key) {
            if let Err(error) = backend.write(&scheme, key, value) {
                return Err(rollback_failed_enable(
                    backend, &path, &state, &changed, error,
                ));
            }
            changed.push(key);
        }
    }
    if let Err(error) = backend.activate(&scheme) {
        return Err(rollback_failed_enable(
            backend, &path, &state, &changed, error,
        ));
    }
    for key in &changed {
        let expected = state.applied.get(*key);
        match backend.read(&scheme, *key) {
            Ok(actual) if Some(actual) == expected => {}
            Ok(actual) => {
                let error = format!(
                    "verification read {} instead of Never for {}",
                    actual,
                    key.label()
                );
                return Err(rollback_failed_enable(
                    backend, &path, &state, &changed, error,
                ));
            }
            Err(error) => {
                return Err(rollback_failed_enable(
                    backend, &path, &state, &changed, error,
                ));
            }
        }
    }

    Ok(status_on(&state, false))
}

/// Rolls a failed enable back to the original values (best effort per key),
/// deletes the state file, and returns the plain-English error message.
fn rollback_failed_enable(
    backend: &dyn PowerBackend,
    path: &Path,
    state: &AwakeStateFile,
    changed: &[PowerKey],
    cause: String,
) -> String {
    let mut rollback_failures: Vec<String> = Vec::new();
    for key in changed {
        if let Some(original) = state.originals.get(*key)
            && let Err(error) = backend.write(&state.scheme_guid, *key, original)
        {
            rollback_failures.push(format!("{}: {error}", key.label()));
        }
    }
    // Best effort re-apply so the partially written values take effect.
    let _ = backend.activate(&state.scheme_guid);
    let _ = state_file::delete(path);

    let cause = cause.trim_end_matches('.').trim_end_matches(')');
    let mut message = format!(
        "Awake could not be turned on: {cause}. Your original power settings were restored."
    );
    if !rollback_failures.is_empty() {
        message.push_str(&format!(
            " However, these keys could not be rolled back automatically: {}. Please check your power settings.",
            rollback_failures.join("; ")
        ));
    }
    message
}

/// Disables Awake (R3): per-key restore that never overwrites a manual
/// change. When some keys fail to restore, the state file is KEPT so the
/// next reconcile retries; the returned status then still says "on".
pub fn disable(
    backend: &dyn PowerBackend,
    state_dir: &Path,
) -> Result<(AwakeStatus, RestoreReport), String> {
    let path = state_file::state_path(state_dir);
    let (state, _recovered) = load_recovering(&path)?;
    let Some(state) = state else {
        return Ok((status_off(), RestoreReport::default()));
    };

    let report = restore_stored(backend, &state)?;
    if report.failed.is_empty() {
        state_file::delete(&path)?;
        Ok((status_off(), report))
    } else {
        // Keep the state file; the next reconcile retries the failed keys.
        Ok((status_on(&state, false), report))
    }
}

/// Restores the stored scheme's values per R3. Always targets the scheme
/// stored in the state file, never whatever is active right now.
fn restore_stored(
    backend: &dyn PowerBackend,
    state: &AwakeStateFile,
) -> Result<RestoreReport, String> {
    let mut report = RestoreReport::default();
    let scheme = state.scheme_guid.as_str();

    match backend.scheme_exists(scheme) {
        Ok(true) => {}
        Ok(false) => {
            report.scheme_gone = true;
            return Ok(report);
        }
        Err(error) => {
            return Err(format!(
                "Awake could not verify that the power scheme still exists: {error} Nothing was changed; please try again."
            ));
        }
    }

    let mut current = PowerValues::default();
    for key in PowerKey::ALL {
        current.set(key, backend.read(scheme, key).ok());
    }

    for action in restore_plan(&state.originals, &state.applied, &current) {
        match action {
            RestoreAction::Restore { key, original } => {
                if let Err(error) = backend.write(scheme, key, original) {
                    report
                        .failed
                        .push(format!("Could not restore {}: {error}", key.label()));
                } else {
                    report.restored += 1;
                }
            }
            RestoreAction::KeepManual { .. } => report.kept += 1,
            RestoreAction::Untouched { .. } => {}
        }
    }

    // Make the restored values take effect — but only if the stored scheme
    // is still the active one. Activating a scheme the user deliberately
    // switched away from would overwrite their choice, and their manual
    // changes are always kept (R3).
    if let Ok(active) = backend.active_scheme()
        && active.eq_ignore_ascii_case(scheme)
        && let Err(error) = backend.activate(scheme)
        && report.failed.is_empty()
    {
        report
            .failed
            .push(format!("The restore may need a scheme re-apply: {error}"));
    }
    Ok(report)
}

/// Applies drawer changes while Awake is on (R5). Writes the power values
/// first, then the state file, then verifies — a failed verification rolls
/// the power values back and leaves the state file untouched.
pub fn set_options(
    backend: &dyn PowerBackend,
    state_dir: &Path,
    options: AwakeOptions,
) -> Result<AwakeStatus, String> {
    let path = state_file::state_path(state_dir);
    let (state, _recovered) = load_recovering(&path)?;
    let state =
        state.ok_or_else(|| "Awake is not on, so there are no options to change.".to_string())?;
    let scheme = state.scheme_guid.as_str();

    let mut new_state = state.clone();
    new_state.ends_at_unix = options.ends_at_unix;

    if options.keep_screen_on != state.keep_screen_on {
        if options.keep_screen_on {
            // The display goes back to Never: write 0 where the original was
            // readable, record it as applied, save, then verify.
            let mut new_applied = state.applied;
            for key in PowerKey::DISPLAY {
                if state.originals.get(key).is_some() {
                    let previous = backend.read(scheme, key).ok();
                    backend
                        .write(scheme, key, 0)
                        .map_err(|error| format!("Awake could not keep the screen on: {error}"))?;
                    new_applied.set(key, Some(0));
                    new_state.keep_screen_on = true;
                    new_state.applied = new_applied;
                    state_file::save(&path, &new_state)?;
                    // Verify; on failure roll this key back to its previous value.
                    match backend.read(scheme, key) {
                        Ok(0) => {}
                        Ok(actual) => {
                            let _ = previous.map(|value| backend.write(scheme, key, value));
                            return Err(format!(
                                "Awake could not verify that the screen stays on (read {actual})."
                            ));
                        }
                        Err(error) => {
                            let _ = previous.map(|value| backend.write(scheme, key, value));
                            return Err(format!(
                                "Awake could not verify the screen setting: {error}"
                            ));
                        }
                    }
                }
            }
            new_state.keep_screen_on = true;
            new_state.applied = new_applied;
        } else {
            // The screen may turn off normally: per R3, restore each display
            // key only if it still holds the applied value, then record it
            // as untouched.
            for key in PowerKey::DISPLAY {
                if let Some(applied_value) = state.applied.get(key) {
                    let current = backend.read(scheme, key).ok();
                    if current == Some(applied_value)
                        && let Some(original) = state.originals.get(key)
                    {
                        backend.write(scheme, key, original).map_err(|error| {
                            format!("Awake could not let the screen turn off: {error}")
                        })?;
                    }
                }
                new_state.applied.set(key, None);
            }
            new_state.keep_screen_on = false;
        }
    }

    state_file::save(&path, &new_state)?;
    Ok(status_on(&new_state, false))
}

/// One reconcile pass (R4/R6). Compares the state file with reality; on a
/// manual change, a vanished scheme or an expired timer, Awake switches
/// itself off (restore per R3) and reports why.
pub fn reconcile(
    backend: &dyn PowerBackend,
    state_dir: &Path,
    now_unix: i64,
    at_app_start: bool,
) -> Result<ReconcileOutcome, String> {
    let path = state_file::state_path(state_dir);
    let (state, recovered) = load_recovering(&path)?;
    if recovered {
        return Ok(ReconcileOutcome::AutoOff {
            reason: AutoOffReason::StateFileDamaged,
            message: auto_off_message(AutoOffReason::StateFileDamaged, &RestoreReport::default()),
            report: RestoreReport::default(),
        });
    }
    let Some(state) = state else {
        return Ok(ReconcileOutcome::NotOn);
    };

    let active = backend.active_scheme()?;
    let stored_scheme_exists = if active.eq_ignore_ascii_case(&state.scheme_guid) {
        true
    } else {
        backend.scheme_exists(&state.scheme_guid)?
    };

    let mut current = PowerValues::default();
    for key in PowerKey::ALL {
        current.set(key, backend.read(&state.scheme_guid, key).ok());
    }

    match reconcile_decision(
        &state.scheme_guid,
        &state.applied,
        &current,
        &active,
        stored_scheme_exists,
    ) {
        ReconcileDecision::StillOn => {
            if timer_expired(now_unix, state.ends_at_unix) {
                let reason = if at_app_start {
                    AutoOffReason::TimerExpiredWhileClosed
                } else {
                    AutoOffReason::TimerExpired
                };
                let report = finish_auto_off(backend, &state, &path)?;
                Ok(ReconcileOutcome::AutoOff {
                    reason,
                    message: auto_off_message(reason, &report),
                    report,
                })
            } else {
                Ok(ReconcileOutcome::StillOn {
                    resumed: at_app_start,
                    status: status_on(&state, at_app_start),
                })
            }
        }
        ReconcileDecision::Changed => {
            let report = finish_auto_off(backend, &state, &path)?;
            Ok(ReconcileOutcome::AutoOff {
                reason: AutoOffReason::ManualChange,
                message: auto_off_message(AutoOffReason::ManualChange, &report),
                report,
            })
        }
        ReconcileDecision::SchemeGone => {
            // Nothing to restore: the scheme itself is gone.
            state_file::delete(&path)?;
            let report = RestoreReport {
                scheme_gone: true,
                ..RestoreReport::default()
            };
            Ok(ReconcileOutcome::AutoOff {
                reason: AutoOffReason::SchemeGone,
                message: auto_off_message(AutoOffReason::SchemeGone, &report),
                report,
            })
        }
    }
}

/// Restore + state file cleanup for the auto-off paths.
fn finish_auto_off(
    backend: &dyn PowerBackend,
    state: &AwakeStateFile,
    path: &Path,
) -> Result<RestoreReport, String> {
    let report = restore_stored(backend, state)?;
    if report.failed.is_empty() {
        state_file::delete(path)?;
    }
    Ok(report)
}

/// Why Awake switched itself off, as one honest sentence (the wording for
/// manual change and the two timer cases is fixed by the spec).
pub fn auto_off_message(reason: AutoOffReason, report: &RestoreReport) -> String {
    let mut message = match reason {
        AutoOffReason::ManualChange => {
            "Awake was switched off because the sleep or screen settings were changed manually in Windows. Your changes were kept.".to_string()
        }
        AutoOffReason::SchemeGone => {
            "Awake was switched off because the Windows power plan it changed no longer exists. Your other settings are untouched.".to_string()
        }
        AutoOffReason::TimerExpired => {
            "Awake timer finished. Your settings were restored.".to_string()
        }
        AutoOffReason::TimerExpiredWhileClosed => {
            "The Awake timer finished while ERX Tools was closed. Your settings were restored.".to_string()
        }
        AutoOffReason::StateFileDamaged => {
            "Awake's state file was damaged and has been reset. Your power settings were not touched by Awake, but if the sleep settings look wrong, set them again in Windows.".to_string()
        }
    };
    if !report.failed.is_empty() {
        message.push_str(" Some settings could not be restored automatically; turning Awake off again will retry.");
    }
    message
}

/// Loads the state file, recovering from an unreadable one: the file is
/// deleted so a damaged file can never permanently lock the Awake section.
/// The `recovered` flag lets callers say so honestly (reconcile reports it
/// as `StateFileDamaged`); the power settings themselves are not touched.
fn load_recovering(path: &Path) -> Result<(Option<AwakeStateFile>, bool), String> {
    match state_file::load(path) {
        Ok(state) => Ok((state, false)),
        Err(error) => {
            eprintln!("Awake state file was unreadable and has been removed: {error}");
            state_file::delete(path)?;
            Ok((None, true))
        }
    }
}

/// The outcome of one reconcile pass, for the command layer to act on.
#[derive(Debug, Clone, PartialEq)]
pub enum ReconcileOutcome {
    /// No state file: Awake is off.
    NotOn,
    /// Still genuinely on. `resumed` is true only for the app-start pass.
    StillOn { resumed: bool, status: AwakeStatus },
    /// Awake switched itself off; the command layer notifies the user.
    AutoOff {
        reason: AutoOffReason,
        message: String,
        report: RestoreReport,
    },
}

/// Status of an off Awake.
pub fn status_off() -> AwakeStatus {
    AwakeStatus {
        state: "off",
        started_at_unix: None,
        ends_at_unix: None,
        keep_screen_on: true,
        resumed_from_previous_session: false,
    }
}

/// Status of an on Awake, derived from the state file only.
pub fn status_on(state: &AwakeStateFile, resumed: bool) -> AwakeStatus {
    AwakeStatus {
        state: "on",
        started_at_unix: Some(state.started_at_unix),
        ends_at_unix: state.ends_at_unix,
        keep_screen_on: state.keep_screen_on,
        resumed_from_previous_session: resumed,
    }
}
