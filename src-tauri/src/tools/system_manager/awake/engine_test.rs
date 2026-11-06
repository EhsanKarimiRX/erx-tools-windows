//! Engine tests: the whole Awake state machine against FakePower and temp
//! state directories. No Windows API is touched. Live verification with the
//! real backend happens in `live_test.rs` (#[ignore], run on purpose).

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::core::{AutoOffReason, PowerKey, PowerValues, RestoreAction};
use super::engine::{self, ReconcileOutcome, enable, reconcile, set_options};
use super::power::PowerBackend;
use super::power::fake::{DEFAULT_SCHEME, FakePower, OTHER_SCHEME};
use super::state_file;

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "erx-awake-engine-{}-{}-{}",
        std::process::id(),
        tag,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn options(keep_screen_on: bool, ends_at_unix: Option<i64>) -> super::core::AwakeOptions {
    super::core::AwakeOptions {
        keep_screen_on,
        ends_at_unix,
    }
}

/// The "original" value FakePower::with_originals gives a key.
fn original_of(key: PowerKey) -> u32 {
    1800 + key.as_str().len() as u32
}

/// Wraps a backend and records whether the state file already existed when
/// the FIRST power write happened (the R1 order guarantee).
struct StateBeforeWriteSpy<'a> {
    inner: &'a FakePower,
    state_path: PathBuf,
    state_existed_before_first_write: Cell<Option<bool>>,
    writes_seen: Cell<usize>,
}

impl<'a> StateBeforeWriteSpy<'a> {
    fn new(inner: &'a FakePower, state_path: PathBuf) -> Self {
        StateBeforeWriteSpy {
            inner,
            state_path,
            state_existed_before_first_write: Cell::new(None),
            writes_seen: Cell::new(0),
        }
    }
}

impl PowerBackend for StateBeforeWriteSpy<'_> {
    fn active_scheme(&self) -> Result<String, String> {
        self.inner.active_scheme()
    }
    fn read(&self, scheme: &str, key: PowerKey) -> Result<u32, String> {
        self.inner.read(scheme, key)
    }
    fn write(&self, scheme: &str, key: PowerKey, value: u32) -> Result<(), String> {
        if self.writes_seen.get() == 0 {
            self.state_existed_before_first_write
                .set(Some(self.state_path.exists()));
        }
        self.writes_seen.set(self.writes_seen.get() + 1);
        self.inner.write(scheme, key, value)
    }
    fn activate(&self, scheme: &str) -> Result<(), String> {
        self.inner.activate(scheme)
    }
    fn scheme_exists(&self, scheme: &str) -> Result<bool, String> {
        self.inner.scheme_exists(scheme)
    }
}

#[test]
fn enable_happy_path_sets_everything_to_never() {
    let dir = temp_dir("happy");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    let status = enable(&fake, &dir, options(true, None), 1000).expect("enable");

    assert_eq!(status.state, "on");
    assert_eq!(status.started_at_unix, Some(1000));
    assert_eq!(status.ends_at_unix, None);
    assert!(status.keep_screen_on);
    assert!(!status.resumed_from_previous_session);

    // All six values are Never in the backend…
    for key in PowerKey::ALL {
        assert_eq!(fake.get(DEFAULT_SCHEME, key), Some(0), "{key:?}");
    }
    // …the state file records what was there and what we set…
    let state = state_file::load(&state_file::state_path(&dir))
        .expect("load")
        .expect("state file exists");
    assert_eq!(state.scheme_guid, DEFAULT_SCHEME);
    assert!(state.applied.sleep_ac == Some(0) && state.applied.display_dc == Some(0));
    assert!(state.originals.sleep_ac.is_some());
    // …and the state file was written before the first power write (R1).
    let spy = StateBeforeWriteSpy::new(&fake, state_file::state_path(&dir));
    let _ = engine::disable(&spy, &dir);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn enable_writes_state_file_before_first_write() {
    let dir = temp_dir("order");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    let spy = StateBeforeWriteSpy::new(&fake, state_file::state_path(&dir));
    enable(&spy, &dir, options(true, None), 1000).expect("enable");
    assert_eq!(
        spy.state_existed_before_first_write.get(),
        Some(true),
        "state file must exist before the first power write"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn enable_with_failing_write_rolls_back_everything() {
    let dir = temp_dir("rollback-write");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    // The display-battery key fails; four keys were already changed.
    fake.fail_write.set(Some(PowerKey::DisplayDc));

    let error = enable(&fake, &dir, options(true, None), 1000).expect_err("must fail");
    assert!(
        error.contains("original power settings were restored"),
        "{error}"
    );

    // Every key is back to its original value, the state file is gone.
    for key in PowerKey::ALL {
        assert_eq!(
            fake.get(DEFAULT_SCHEME, key),
            Some(1800 + key.as_str().len() as u32)
        );
    }
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn enable_with_failing_activation_rolls_back_everything() {
    let dir = temp_dir("rollback-activate");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    fake.fail_activate.set(true);
    let error = enable(&fake, &dir, options(true, None), 1000).expect_err("must fail");
    assert!(
        error.contains("original power settings were restored"),
        "{error}"
    );
    for key in PowerKey::ALL {
        assert_eq!(
            fake.get(DEFAULT_SCHEME, key),
            Some(1800 + key.as_str().len() as u32)
        );
    }
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn damaged_state_file_is_removed_and_reported_then_awake_works_again() {
    let dir = temp_dir("damaged");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    let path = state_file::state_path(&dir);
    fs::write(&path, "{not json").expect("write garbage");

    // Reconcile recovers honestly: AutoOff with the damaged-file reason,
    // the file is gone, and NO power values were touched.
    let outcome = reconcile(&fake, &dir, 1000, true).expect("reconcile");
    match outcome {
        ReconcileOutcome::AutoOff {
            reason, message, ..
        } => {
            assert_eq!(reason, AutoOffReason::StateFileDamaged);
            assert!(message.contains("damaged"), "{message}");
        }
        other => panic!("expected AutoOff, got {other:?}"),
    }
    assert_eq!(state_file::load(&path), Ok(None));
    assert_eq!(fake.write_count(), 0);

    // Enabling afterwards just works.
    let status = enable(&fake, &dir, options(true, None), 2000).expect("enable after recovery");
    assert_eq!(status.state, "on");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn enable_with_failed_verification_rolls_back_everything() {
    let dir = temp_dir("rollback-verify");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    fake.corrupt_read.set(Some(PowerKey::HibernateDc));
    let error = enable(&fake, &dir, options(true, None), 1000).expect_err("must fail");
    assert!(error.contains("verification"), "{error}");
    for key in PowerKey::ALL {
        // The corrupted key's ORIGINAL was recorded as value+1 (the fake
        // corrupts every read of it, including the very first), and the
        // rollback faithfully wrote that recorded value back.
        let expected = if key == PowerKey::HibernateDc {
            original_of(key) + 1
        } else {
            original_of(key)
        };
        assert_eq!(fake.get(DEFAULT_SCHEME, key), Some(expected), "{key:?}");
    }
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn enable_twice_is_an_error() {
    let dir = temp_dir("twice");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("first enable");
    let error = enable(&fake, &dir, options(true, None), 2000).expect_err("second enable");
    assert!(error.contains("already on"), "{error}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn enable_screen_normal_leaves_display_keys_untouched() {
    let dir = temp_dir("screen-normal");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    let status = enable(&fake, &dir, options(false, None), 1000).expect("enable");
    assert!(!status.keep_screen_on);

    // Sleep/hibernate are Never; the display keys keep their originals.
    assert_eq!(fake.get(DEFAULT_SCHEME, PowerKey::SleepAc), Some(0));
    assert_eq!(fake.get(DEFAULT_SCHEME, PowerKey::HibernateDc), Some(0));
    assert_eq!(
        fake.get(DEFAULT_SCHEME, PowerKey::DisplayAc),
        Some(original_of(PowerKey::DisplayAc))
    );
    assert_eq!(
        fake.get(DEFAULT_SCHEME, PowerKey::DisplayDc),
        Some(original_of(PowerKey::DisplayDc))
    );

    let state = state_file::load(&state_file::state_path(&dir))
        .expect("load")
        .expect("some");
    assert!(state.applied.display_ac.is_none() && state.applied.display_dc.is_none());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn enable_never_touches_unreadable_keys() {
    let dir = temp_dir("unreadable");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    // Simulate an unreadable DC hibernate value.
    fake.make_unreadable(DEFAULT_SCHEME, PowerKey::HibernateDc);

    let status = enable(&fake, &dir, options(true, None), 1000).expect("enable");
    assert_eq!(status.state, "on");

    // The unreadable key was never written; the readable ones were.
    assert_eq!(fake.get(DEFAULT_SCHEME, PowerKey::HibernateDc), None);
    assert_eq!(fake.get(DEFAULT_SCHEME, PowerKey::HibernateAc), Some(0));
    let state = state_file::load(&state_file::state_path(&dir))
        .expect("load")
        .expect("some");
    assert!(state.originals.hibernate_dc.is_none() && state.applied.hibernate_dc.is_none());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn enable_with_nothing_readable_fails_cleanly() {
    let dir = temp_dir("nothing-readable");
    let fake = FakePower::new(DEFAULT_SCHEME); // no values at all: every read fails
    let error = enable(&fake, &dir, options(true, None), 1000).expect_err("must fail");
    assert!(error.contains("Nothing was changed"), "{error}");
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    assert_eq!(fake.write_count(), 0);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn disable_restores_everything_and_deletes_state() {
    let dir = temp_dir("disable");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");

    let (status, report) = engine::disable(&fake, &dir).expect("disable");
    assert_eq!(status.state, "off");
    assert_eq!(report.restored, 6);
    assert_eq!(report.kept, 0);
    assert!(report.failed.is_empty() && !report.scheme_gone);
    for key in PowerKey::ALL {
        assert_eq!(
            fake.get(DEFAULT_SCHEME, key),
            Some(1800 + key.as_str().len() as u32)
        );
    }
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn disable_keeps_manual_changes_and_deletes_state() {
    let dir = temp_dir("disable-manual");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");
    // The user changes sleep (plugged in) to 20 minutes by hand.
    fake.set_manual(DEFAULT_SCHEME, PowerKey::SleepAc, 1200);

    let (status, report) = engine::disable(&fake, &dir).expect("disable");
    assert_eq!(status.state, "off");
    assert_eq!(report.restored, 5);
    assert_eq!(report.kept, 1);
    assert_eq!(
        fake.get(DEFAULT_SCHEME, PowerKey::SleepAc),
        Some(1200),
        "manual change kept"
    );
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn disable_with_failed_restore_keeps_state_for_retry() {
    let dir = temp_dir("disable-retry");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");
    fake.fail_write.set(Some(PowerKey::HibernateDc));

    let (status, report) = engine::disable(&fake, &dir).expect("disable reports, not fails");
    assert_eq!(status.state, "on", "state file kept, so Awake is still on");
    assert_eq!(report.restored, 5);
    assert_eq!(report.failed.len(), 1);
    assert!(
        state_file::load(&state_file::state_path(&dir))
            .expect("load")
            .is_some()
    );

    // The failure clears: the next disable retries and finishes the job.
    fake.fail_write.set(None);
    let (status, report) = engine::disable(&fake, &dir).expect("retry disable");
    assert_eq!(status.state, "off");
    assert_eq!(report.restored, 1);
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn disable_without_state_file_is_a_noop() {
    let dir = temp_dir("disable-noop");
    let fake = FakePower::new(DEFAULT_SCHEME);
    let (status, report) = engine::disable(&fake, &dir).expect("disable");
    assert_eq!(status.state, "off");
    assert_eq!(
        report,
        super::super::awake::core::RestoreReport::default(),
        "nothing restored, kept or failed"
    );
    assert_eq!(fake.write_count(), 0);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn disable_reports_gone_scheme_and_deletes_state() {
    let dir = temp_dir("disable-gone");
    // Enable on DEFAULT, then the stored scheme disappears: the active
    // scheme is now OTHER and DEFAULT is no longer known.
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");
    let switched = FakePower::new(OTHER_SCHEME).with_originals();

    let (status, report) = engine::disable(&switched, &dir).expect("disable");
    assert_eq!(status.state, "off");
    assert!(report.scheme_gone);
    assert_eq!(report.restored, 0);
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reconcile_detects_manual_change_and_auto_offs() {
    let dir = temp_dir("reconcile-manual");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");
    fake.set_manual(DEFAULT_SCHEME, PowerKey::SleepAc, 1200);

    let outcome = reconcile(&fake, &dir, 2000, false).expect("reconcile");
    match outcome {
        ReconcileOutcome::AutoOff {
            reason,
            message,
            report,
        } => {
            assert_eq!(reason, AutoOffReason::ManualChange);
            assert!(message.contains("changed manually in Windows"), "{message}");
            assert_eq!(report.restored, 5);
            assert_eq!(report.kept, 1);
        }
        other => panic!("expected AutoOff, got {other:?}"),
    }
    // The manual change survives; everything else is back; state file gone.
    assert_eq!(fake.get(DEFAULT_SCHEME, PowerKey::SleepAc), Some(1200));
    assert_eq!(
        fake.get(DEFAULT_SCHEME, PowerKey::SleepDc),
        Some(original_of(PowerKey::SleepDc))
    );
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reconcile_still_on_reports_status_without_side_effects() {
    let dir = temp_dir("reconcile-still");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, Some(5000)), 1000).expect("enable");

    let outcome = reconcile(&fake, &dir, 2000, false).expect("reconcile");
    match outcome {
        ReconcileOutcome::StillOn { resumed, status } => {
            assert!(!resumed);
            assert_eq!(status.state, "on");
            assert_eq!(status.ends_at_unix, Some(5000));
        }
        other => panic!("expected StillOn, got {other:?}"),
    }
    assert!(
        state_file::load(&state_file::state_path(&dir))
            .expect("load")
            .is_some()
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reconcile_timer_expiry_auto_offs() {
    let dir = temp_dir("reconcile-timer");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, Some(5000)), 1000).expect("enable");

    let outcome = reconcile(&fake, &dir, 5000, false).expect("reconcile");
    match outcome {
        ReconcileOutcome::AutoOff {
            reason, message, ..
        } => {
            assert_eq!(reason, AutoOffReason::TimerExpired);
            assert!(message.contains("timer finished"), "{message}");
        }
        other => panic!("expected AutoOff, got {other:?}"),
    }
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reconcile_timer_expired_while_closed_uses_the_closed_wording() {
    let dir = temp_dir("reconcile-timer-closed");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, Some(5000)), 1000).expect("enable");

    let outcome = reconcile(&fake, &dir, 9000, true).expect("reconcile");
    match outcome {
        ReconcileOutcome::AutoOff {
            reason, message, ..
        } => {
            assert_eq!(reason, AutoOffReason::TimerExpiredWhileClosed);
            assert!(message.contains("while ERX Tools was closed"), "{message}");
        }
        other => panic!("expected AutoOff, got {other:?}"),
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reconcile_app_start_still_on_is_flagged_as_resumed() {
    let dir = temp_dir("reconcile-resumed");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");

    let outcome = reconcile(&fake, &dir, 2000, true).expect("reconcile");
    match outcome {
        ReconcileOutcome::StillOn { resumed, status } => {
            assert!(resumed);
            assert!(status.resumed_from_previous_session);
        }
        other => panic!("expected StillOn, got {other:?}"),
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reconcile_scheme_switch_restores_the_stored_scheme_only() {
    let dir = temp_dir("reconcile-switch");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");
    // The user switches to another scheme (DEFAULT still exists).
    fake.add_known_scheme(OTHER_SCHEME);
    fake.switch_active(OTHER_SCHEME);

    let outcome = reconcile(&fake, &dir, 2000, false).expect("reconcile");
    match outcome {
        ReconcileOutcome::AutoOff { reason, report, .. } => {
            assert_eq!(reason, AutoOffReason::ManualChange);
            assert_eq!(report.restored, 6, "restored into the stored scheme");
        }
        other => panic!("expected AutoOff, got {other:?}"),
    }
    // The stored scheme's values are back; the user's new scheme was never
    // written to (the restore only ever targets the stored scheme, R3).
    assert_eq!(
        fake.get(DEFAULT_SCHEME, PowerKey::SleepAc),
        Some(original_of(PowerKey::SleepAc))
    );
    assert!(
        fake.write_log()
            .iter()
            .all(|(scheme, _, _)| scheme == DEFAULT_SCHEME),
        "every write targeted the stored scheme only"
    );
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reconcile_scheme_gone_auto_offs_without_restore() {
    let dir = temp_dir("reconcile-gone");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");
    // The stored scheme disappears entirely.
    let switched = FakePower::new(OTHER_SCHEME).with_originals();

    let outcome = reconcile(&switched, &dir, 2000, false).expect("reconcile");
    match outcome {
        ReconcileOutcome::AutoOff { reason, report, .. } => {
            assert_eq!(reason, AutoOffReason::SchemeGone);
            assert!(report.scheme_gone);
        }
        other => panic!("expected AutoOff, got {other:?}"),
    }
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn set_options_timer_only_rewrites_state_without_writes() {
    let dir = temp_dir("options-timer");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");
    let writes_before = fake.write_count();

    let status = set_options(&fake, &dir, options(true, Some(9000))).expect("set options");
    assert_eq!(status.ends_at_unix, Some(9000));
    let state = state_file::load(&state_file::state_path(&dir))
        .expect("load")
        .expect("some");
    assert_eq!(state.ends_at_unix, Some(9000));
    assert_eq!(
        fake.write_count(),
        writes_before,
        "a timer change writes no power values"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn set_options_screen_normal_restores_display_keys() {
    let dir = temp_dir("options-screen-off");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");

    let status = set_options(&fake, &dir, options(false, None)).expect("set options");
    assert!(!status.keep_screen_on);
    assert_eq!(
        fake.get(DEFAULT_SCHEME, PowerKey::DisplayAc),
        Some(original_of(PowerKey::DisplayAc))
    );
    assert_eq!(
        fake.get(DEFAULT_SCHEME, PowerKey::DisplayDc),
        Some(original_of(PowerKey::DisplayDc))
    );
    let state = state_file::load(&state_file::state_path(&dir))
        .expect("load")
        .expect("some");
    assert!(state.applied.display_ac.is_none() && state.applied.display_dc.is_none());

    // Toggling back puts the display on Never again.
    let status = set_options(&fake, &dir, options(true, None)).expect("set options back");
    assert!(status.keep_screen_on);
    assert_eq!(fake.get(DEFAULT_SCHEME, PowerKey::DisplayAc), Some(0));
    assert_eq!(fake.get(DEFAULT_SCHEME, PowerKey::DisplayDc), Some(0));
    let state = state_file::load(&state_file::state_path(&dir))
        .expect("load")
        .expect("some");
    assert_eq!(state.applied.display_ac, Some(0));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn set_options_screen_normal_keeps_a_manual_display_change() {
    let dir = temp_dir("options-screen-manual");
    let fake = FakePower::new(DEFAULT_SCHEME).with_originals();
    enable(&fake, &dir, options(true, None), 1000).expect("enable");
    // The user sets display (plugged in) to 5 minutes by hand.
    fake.set_manual(DEFAULT_SCHEME, PowerKey::DisplayAc, 300);

    set_options(&fake, &dir, options(false, None)).expect("set options");
    // The manual 5-minute value is kept, not overwritten by the original.
    assert_eq!(fake.get(DEFAULT_SCHEME, PowerKey::DisplayAc), Some(300));
    // The untouched display-battery key goes back to its original.
    assert_eq!(
        fake.get(DEFAULT_SCHEME, PowerKey::DisplayDc),
        Some(original_of(PowerKey::DisplayDc))
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn set_options_while_off_is_an_error() {
    let dir = temp_dir("options-off");
    let fake = FakePower::new(DEFAULT_SCHEME);
    let error = set_options(&fake, &dir, options(true, None)).expect_err("must fail");
    assert!(error.contains("not on"), "{error}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn restore_plan_report_counts_are_honest() {
    // Sanity tie between core::restore_plan and the report counters.
    let originals = PowerValues {
        sleep_ac: Some(1800),
        ..PowerValues::default()
    };
    let applied = PowerValues {
        sleep_ac: Some(0),
        sleep_dc: Some(0),
        ..PowerValues::default()
    };
    let mut current = PowerValues::default();
    current.set(PowerKey::SleepAc, Some(0));
    current.set(PowerKey::SleepDc, Some(1500)); // manual change
    let plan = super::core::restore_plan(&originals, &applied, &current);
    let restored = plan
        .iter()
        .filter(|a| matches!(a, RestoreAction::Restore { .. }))
        .count();
    let kept = plan
        .iter()
        .filter(|a| matches!(a, RestoreAction::KeepManual { .. }))
        .count();
    assert_eq!((restored, kept), (1, 1));
}

#[test]
fn state_path_helpers_agree() {
    let dir = Path::new("C:\\nonexistent-test-dir");
    assert!(state_file::state_path(dir).ends_with("awake_state.json"));
}
