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
/// the switch and shows the message.
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

#[cfg(test)]
mod tests {
    use super::super::core::DesktopIcon;
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
}
