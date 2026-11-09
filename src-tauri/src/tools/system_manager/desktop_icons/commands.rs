//! Tauri command layer for Desktop Icons: read the five visibilities, set
//! one, refresh the desktop. The frontend sends only a stable icon id —
//! never a registry name — and the switch always shows the value read back
//! from the registry after the write, never the write's intention.

use serde::Serialize;

use super::core::{DesktopIcon, is_visible};
use super::registry::{RegistryBackend, RegistryResult};
use crate::tools::ToolError;

#[cfg(windows)]
use super::refresh::notify_desktop_change;
#[cfg(windows)]
use super::registry::windows::WindowsRegistry;

/// One icon row as reported to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopIconState {
    pub id: &'static str,
    pub name: &'static str,
    pub visible: bool,
}

fn backend() -> Result<Box<dyn RegistryBackend>, String> {
    #[cfg(windows)]
    return Ok(Box::new(WindowsRegistry));
    #[cfg(not(windows))]
    return Err("ERX Tools System Manager needs Windows.".into());
}

/// Reads all five visibilities. Called when the section opens and whenever
/// the window regains focus, so changes made in Windows Settings show up.
#[tauri::command]
pub fn desktop_icons_get() -> Result<Vec<DesktopIconState>, ToolError> {
    let registry = backend().map_err(ToolError::Internal)?;
    read_all(registry.as_ref()).map_err(ToolError::Internal)
}

fn read_all(registry: &dyn RegistryBackend) -> Result<Vec<DesktopIconState>, String> {
    DesktopIcon::ALL
        .iter()
        .map(|&icon| {
            let value = registry.read_dword(icon.clsid())?;
            Ok(DesktopIconState {
                id: icon.id(),
                name: icon.label(),
                visible: is_visible(value, icon),
            })
        })
        .collect()
}

/// The write→read-back flow, testable against the fake registry. Returns
/// the visibility READ BACK after the write (the only thing the UI may
/// show). A write or read-back failure is an error; the UI then reverts
/// the switch and shows the message. After the primary write+read-back
/// succeeded, every legacy CLSID (see `DesktopIcon::legacy_clsids`) is
/// also written best effort — failures are ignored and never reach the
/// user, and no value is ever deleted.
pub fn set_icon(
    registry: &dyn RegistryBackend,
    icon: DesktopIcon,
    visible: bool,
) -> RegistryResult<bool> {
    // 0 = shown, 1 = hidden; always an explicit value, never a delete.
    registry.write_dword(icon.clsid(), u32::from(!visible))?;
    let value = registry
        .read_dword(icon.clsid())
        .map_err(|error| format!("The setting was written but could not be read back: {error}"))?
        .ok_or_else(|| "The setting was written but could not be read back.".to_string())?;
    let read_back = is_visible(Some(value), icon);
    if read_back != visible {
        return Err(format!(
            "The setting was written but reads back as {}. Please try again.",
            if read_back { "shown" } else { "hidden" }
        ));
    }
    for legacy in icon.legacy_clsids() {
        let _ = registry.write_dword(legacy, u32::from(!visible));
    }
    Ok(read_back)
}

/// Sets one icon and refreshes the desktop. Returns the read-back state.
#[tauri::command]
pub fn desktop_icons_set(id: String, visible: bool) -> Result<DesktopIconState, ToolError> {
    let icon = DesktopIcon::from_id(&id)
        .ok_or_else(|| ToolError::Internal("Unknown desktop icon id.".into()))?;
    let registry = backend().map_err(ToolError::Internal)?;
    let read_back = set_icon(registry.as_ref(), icon, visible).map_err(ToolError::Internal)?;
    // Only after a verified write: ask the desktop to repaint. Best effort.
    #[cfg(windows)]
    notify_desktop_change();
    Ok(DesktopIconState {
        id: icon.id(),
        name: icon.label(),
        visible: read_back,
    })
}

/// Opens Windows' own "Desktop Icon Settings" dialog (the one behind
/// Settings → Personalization → Themes → Desktop icon settings). Takes no
/// frontend arguments — everything below is a fixed literal.
#[tauri::command]
pub fn desktop_icons_open_windows_settings() -> Result<(), ToolError> {
    open_windows_desktop_icon_settings().map_err(ToolError::Internal)
}

/// Spawns `%SystemRoot%\System32\rundll32.exe` with the arguments
/// `shell32.dll,Control_RunDLL` and `desk.cpl,,0` and returns immediately
/// (the spawned dialog keeps running in its own process; the Child handle
/// is dropped without waiting). The exe path is built from the SystemRoot
/// environment variable — never from PATH — and the arguments are passed
/// as real arguments: no shell, no cmd.exe, no PowerShell, no user input.
#[cfg(windows)]
fn open_windows_desktop_icon_settings() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let rundll32 = std::path::Path::new(&system_root)
        .join("System32")
        .join("rundll32.exe");
    Command::new(&rundll32)
        .args(["shell32.dll,Control_RunDLL", "desk.cpl,,0"])
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .spawn()
        .map(|_child| ()) // intentionally not waited for
        .map_err(|error| format!("Couldn't open the Windows desktop icon settings. ({error})"))
}

#[cfg(not(windows))]
fn open_windows_desktop_icon_settings() -> Result<(), String> {
    Err("ERX Tools System Manager needs Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::super::core::DesktopIcon;
    use super::super::registry::RegistryBackend;
    use super::super::registry::fake::FakeRegistry;
    use super::{DesktopIconState, read_all, set_icon};

    #[test]
    fn read_all_reports_absent_values_as_windows_defaults() {
        let registry = FakeRegistry::new(); // nothing set: all absent
        let states = read_all(&registry).expect("read");
        assert_eq!(states.len(), 5);
        let visible: Vec<bool> = states.iter().map(|state| state.visible).collect();
        // Only the Recycle Bin shows by default; This PC etc. are hidden.
        assert_eq!(
            visible,
            vec![false, true, false, false, false],
            "order: This PC, Recycle Bin, Control Panel, Network, User's Files"
        );
        let names: Vec<&str> = states.iter().map(|state| state.name).collect();
        assert_eq!(names[0], "This PC");
    }

    #[test]
    fn set_icon_writes_explicit_values_and_reads_back() {
        let registry = FakeRegistry::new();
        // Show This PC (absent → default hidden): an explicit 0 is written.
        let shown = set_icon(&registry, DesktopIcon::ThisPc, true).expect("show");
        assert!(shown);
        assert_eq!(registry.get(DesktopIcon::ThisPc.clsid()), Some(0));
        // Hide it again: an explicit 1.
        let hidden = set_icon(&registry, DesktopIcon::ThisPc, false).expect("hide");
        assert!(!hidden);
        assert_eq!(registry.get(DesktopIcon::ThisPc.clsid()), Some(1));
        assert_eq!(registry.write_count(), 2, "no values were ever deleted");
    }

    #[test]
    fn set_icon_reports_write_failure_without_panicking() {
        let mut registry = FakeRegistry::new();
        registry
            .fail_writes_for
            .push(DesktopIcon::RecycleBin.clsid().to_string());
        let error = set_icon(&registry, DesktopIcon::RecycleBin, false).expect_err("must fail");
        assert!(error.contains("simulated write failure"), "{error}");
        assert!(
            !registry.contains(DesktopIcon::RecycleBin.clsid()),
            "a failed write leaves no value behind"
        );
    }

    #[test]
    fn set_icon_reports_read_back_mismatch() {
        // The fake writes and reads honestly, so this path needs a tampered
        // setup: fail the read for one icon so the write succeeds but the
        // read-back errors.
        let mut flaky = FakeRegistry::new();
        flaky
            .fail_reads_for
            .push(DesktopIcon::Network.clsid().to_string());
        let error = set_icon(&flaky, DesktopIcon::Network, false).expect_err("must fail");
        assert!(error.contains("could not be read back"), "{error}");
    }

    #[test]
    fn desktop_icon_state_shape_is_stable() {
        // The UI keys on `id`; keep the contract visible in one place.
        let state = DesktopIconState {
            id: "this_pc",
            name: "This PC",
            visible: false,
        };
        assert_eq!(state.id, DesktopIcon::ThisPc.id());
    }

    #[test]
    fn control_panel_write_reaches_primary_and_legacy() {
        let registry = FakeRegistry::new();
        let shown = set_icon(&registry, DesktopIcon::ControlPanel, true).expect("show");
        assert!(shown);
        assert_eq!(registry.get(DesktopIcon::ControlPanel.clsid()), Some(0));
        assert_eq!(
            registry.get(DesktopIcon::ControlPanel.legacy_clsids()[0]),
            Some(0)
        );
        let hidden = set_icon(&registry, DesktopIcon::ControlPanel, false).expect("hide");
        assert!(!hidden);
        assert_eq!(registry.get(DesktopIcon::ControlPanel.clsid()), Some(1));
        assert_eq!(
            registry.get(DesktopIcon::ControlPanel.legacy_clsids()[0]),
            Some(1)
        );
    }

    #[test]
    fn a_failing_legacy_write_never_fails_set_icon() {
        let mut registry = FakeRegistry::new();
        registry
            .fail_writes_for
            .push(DesktopIcon::ControlPanel.legacy_clsids()[0].to_string());
        let shown = set_icon(&registry, DesktopIcon::ControlPanel, true)
            .expect("the primary write must still succeed");
        assert!(shown);
        assert_eq!(registry.get(DesktopIcon::ControlPanel.clsid()), Some(0));
        assert!(
            registry
                .get(DesktopIcon::ControlPanel.legacy_clsids()[0])
                .is_none(),
            "the failed legacy write leaves no value behind"
        );
    }

    #[test]
    fn read_all_uses_only_the_primary_clsid() {
        let registry = FakeRegistry::new();
        // Only the LEGACY name is set to 0 (would mean "shown" if it were read).
        registry
            .write_dword(DesktopIcon::ControlPanel.legacy_clsids()[0], 0)
            .expect("seed legacy value");
        let states = read_all(&registry).expect("read");
        let control_panel = states
            .iter()
            .find(|state| state.id == "control_panel")
            .expect("control panel row");
        assert!(
            !control_panel.visible,
            "the legacy value must be ignored; an absent primary means the Windows default (hidden)"
        );
    }
}
