//! Live Desktop Icons verification on the real machine (run on purpose
//! ONLY, with the G9 snapshot protocol — see docs/PROMPT_5_PROGRESS.md
//! "Owner snapshots"). The five registry values (or "absent") are recorded
//! FIRST and restored EXACTLY at the end; a Drop guard restores even on a
//! failed assertion.
//!
//! Run:  cargo test -- --ignored

use std::sync::{Mutex, MutexGuard, OnceLock};

use super::core::DesktopIcon;
use super::registry::RegistryBackend;
use super::registry::windows::WindowsRegistry;

/// Serializes the live tests (they share the same real registry values).
fn live_lock() -> MutexGuard<'static, ()> {
    static LIVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let mutex = LIVE_LOCK.get_or_init(|| Mutex::new(()));
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// One icon's original state: a value, or absent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Original {
    Value(u32),
    Absent,
}

fn snapshot_all(backend: &WindowsRegistry) -> [(DesktopIcon, Original); 5] {
    DesktopIcon::ALL.map(|icon| {
        let original = match backend.read_dword(icon.clsid()) {
            Ok(Some(value)) => Original::Value(value),
            Ok(None) => Original::Absent,
            Err(error) => panic!("snapshot read failed for {icon:?}: {error}"),
        };
        (icon, original)
    })
}

/// Restores every original (value or absence) on drop.
struct RestoreGuard {
    backend: WindowsRegistry,
    snapshot: [(DesktopIcon, Original); 5],
}

impl Drop for RestoreGuard {
    fn drop(&mut self) {
        for (icon, original) in &self.snapshot {
            match original {
                Original::Value(value) => {
                    let _ = self.backend.write_dword(icon.clsid(), *value);
                }
                Original::Absent => delete_value(icon.clsid()),
            }
        }
    }
}

/// Deletes a value so an originally-absent icon becomes absent again. Only
/// ever used by this test's restore step, for values the test created.
fn delete_value(value_name: &str) {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let subkey = wide(super::registry::NEW_START_PANEL_SUBKEY);
    let name = wide(value_name);
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        let code = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        );
        if code != ERROR_SUCCESS {
            return; // key absent: nothing to restore
        }
        RegDeleteValueW(key, name.as_ptr());
        RegCloseKey(key);
    }
}

#[test]
#[ignore]
fn live_desktop_icons_round_trip_restores_the_snapshot() {
    let _live = live_lock();
    let backend = WindowsRegistry;
    let snapshot = snapshot_all(&backend);
    let _guard = RestoreGuard {
        backend: WindowsRegistry,
        snapshot,
    };

    // This PC: show it, verify the read-back, hide it, verify again.
    let this_pc_visible =
        super::commands::set_icon(&backend, DesktopIcon::ThisPc, true).expect("show This PC");
    assert!(
        this_pc_visible,
        "This PC must read back as shown after the write"
    );
    let this_pc_hidden =
        super::commands::set_icon(&backend, DesktopIcon::ThisPc, false).expect("hide This PC");
    assert!(!this_pc_hidden);

    // Recycle Bin (absent on this machine per the snapshot): one explicit
    // write, then the Drop guard restores its absence exactly.
    if snapshot
        .iter()
        .any(|(icon, o)| *icon == DesktopIcon::RecycleBin && *o == Original::Absent)
    {
        let bin_hidden = super::commands::set_icon(&backend, DesktopIcon::RecycleBin, false)
            .expect("write Recycle Bin");
        assert!(!bin_hidden);
    }

    // After the guard drops, the values equal the snapshot again.
    drop(_guard);
    for (icon, original) in &snapshot {
        let now = backend.read_dword(icon.clsid()).expect("read back");
        let now = match now {
            Some(value) => Original::Value(value),
            None => Original::Absent,
        };
        assert_eq!(now, *original, "{icon:?} must be restored exactly");
    }
}
