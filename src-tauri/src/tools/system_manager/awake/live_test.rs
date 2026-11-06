//! Live Awake verification on the real machine (run on purpose ONLY, with
//! the G9 snapshot protocol: docs/PROMPT_5_PROGRESS.md "Owner snapshots").
//!
//! Run:  cargo test -- --ignored
//!
//! Every test snapshots the six original power values first and restores
//! them in a Drop guard, so a failing assertion can never leave "Never"
//! behind. The tests share ONE mutex because they all change the same real
//! power scheme — they must never run in parallel. The state file is
//! written to a TEMP directory, never the real app data folder.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::core::{AutoOffReason, AwakeOptions, PowerKey, PowerValues};
use super::engine::{self, ReconcileOutcome};
use super::power::PowerBackend;
use super::power::windows::WindowsPower;
use super::state_file;

/// Serializes the live tests: they all touch the same real power scheme.
fn live_lock() -> MutexGuard<'static, ()> {
    static LIVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let mutex = LIVE_LOCK.get_or_init(|| Mutex::new(()));
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn temp_state_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "erx-awake-live-{}-{}-{}",
        std::process::id(),
        tag,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Restores the snapshot on drop, even when an assertion fails.
struct SnapshotGuard {
    backend: WindowsPower,
    scheme: String,
    originals: PowerValues,
}

impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        for key in PowerKey::ALL {
            if let Some(original) = self.originals.get(key) {
                let _ = self.backend.write(&self.scheme, key, original);
            }
        }
        let _ = self.backend.activate(&self.scheme);
    }
}

fn snapshot(backend: &WindowsPower) -> (String, PowerValues) {
    let scheme = backend.active_scheme().expect("read active scheme");
    let mut originals = PowerValues::default();
    for key in PowerKey::ALL {
        originals.set(key, backend.read(&scheme, key).ok());
    }
    (scheme, originals)
}

#[test]
#[ignore]
fn live_scheme_probe_and_active_scheme_round_trip() {
    let _live = live_lock();
    let backend = WindowsPower;
    let scheme = backend.active_scheme().expect("active scheme");
    assert_eq!(
        backend.scheme_exists(&scheme),
        Ok(true),
        "the active scheme must exist"
    );
    assert_eq!(
        backend.scheme_exists("00000000-0000-0000-0000-000000000000"),
        Ok(false),
        "the null GUID must not exist; if this fails with a different error, adjust scheme_exists"
    );
}

#[test]
#[ignore]
fn live_enable_then_disable_restores_the_snapshot() {
    let _live = live_lock();
    let backend = WindowsPower;
    let (scheme, originals) = snapshot(&backend);
    let _guard = SnapshotGuard {
        backend: WindowsPower,
        scheme: scheme.clone(),
        originals,
    };

    let dir = temp_state_dir("roundtrip");
    let status = engine::enable(
        &backend,
        &dir,
        AwakeOptions {
            keep_screen_on: true,
            ends_at_unix: None,
        },
        0,
    )
    .expect("enable must succeed without admin");
    assert_eq!(status.state, "on");

    // Every readable key now reads back as Never (0).
    for key in PowerKey::ALL {
        if originals.get(key).is_some() {
            assert_eq!(
                backend.read(&scheme, key).ok(),
                Some(0),
                "{:?} must be Never",
                key
            );
        }
    }

    let (status, report) = engine::disable(&backend, &dir).expect("disable");
    assert_eq!(status.state, "off");
    assert!(
        report.failed.is_empty(),
        "restore failures: {:?}",
        report.failed
    );
    assert!(!report.scheme_gone);

    // The machine equals the snapshot again.
    for key in PowerKey::ALL {
        assert_eq!(
            backend.read(&scheme, key).ok(),
            originals.get(key),
            "{:?} must be restored",
            key
        );
    }
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}

#[test]
#[ignore]
fn live_manual_change_switches_awake_off_and_is_kept() {
    let _live = live_lock();
    let backend = WindowsPower;
    let (scheme, originals) = snapshot(&backend);
    let _guard = SnapshotGuard {
        backend: WindowsPower,
        scheme: scheme.clone(),
        originals,
    };

    let dir = temp_state_dir("manual");
    engine::enable(
        &backend,
        &dir,
        AwakeOptions {
            keep_screen_on: true,
            ends_at_unix: None,
        },
        0,
    )
    .expect("enable");

    // Simulate the user changing sleep (plugged in) to 20 minutes, the way
    // `powercfg /change standby-timeout-ac 20` would (20 minutes = 1200 s).
    // We write through the backend instead of spawning powercfg.
    backend
        .write(&scheme, PowerKey::SleepAc, 1200)
        .expect("simulate manual change");
    backend.activate(&scheme).expect("apply the manual change");

    let outcome = engine::reconcile(&backend, &dir, 0, false).expect("reconcile");
    match outcome {
        ReconcileOutcome::AutoOff { reason, report, .. } => {
            assert_eq!(reason, AutoOffReason::ManualChange);
            assert_eq!(report.kept, 1, "the manual 1200 s value must be kept");
        }
        other => panic!("expected AutoOff, got {other:?}"),
    }
    assert_eq!(
        backend.read(&scheme, PowerKey::SleepAc).ok(),
        Some(1200),
        "manual value kept"
    );
    // Everything else is back to the snapshot.
    for key in PowerKey::ALL {
        if key != PowerKey::SleepAc {
            assert_eq!(
                backend.read(&scheme, key).ok(),
                originals.get(key),
                "{:?} restored",
                key
            );
        }
    }
    assert_eq!(state_file::load(&state_file::state_path(&dir)), Ok(None));
    fs::remove_dir_all(&dir).ok();
}
