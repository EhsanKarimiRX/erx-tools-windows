//! Tauri command layer for Quick Launch: the info facts (PowerShell
//! flavor, PC Manager state) and the launches. The frontend sends only an
//! id from the fixed allowlist; every path decision happens in Rust.

use serde::Serialize;

use super::core::launch_spec;
use super::launch;
use crate::tools::ToolError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PcManagerInfo {
    pub installed: bool,
    pub family: Option<String>,
    pub app_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickLaunchInfo {
    /// "PowerShell 7" or "Windows PowerShell" — shown under the button.
    pub powershell_label: String,
    pub pc_manager: PcManagerInfo,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    /// "opened" when the launch call succeeded, "cancelled" when the user
    /// dismissed a UAC prompt. The UI shows a neutral message for both and
    /// never invents more than this.
    pub outcome: String,
}

/// What the section shows before any click: which PowerShell will start
/// and whether Microsoft PC Manager is installed.
#[tauri::command]
pub fn quick_launch_info() -> Result<QuickLaunchInfo, ToolError> {
    #[cfg(windows)]
    {
        let facts = launch::quick_launch_facts().map_err(ToolError::Internal)?;
        Ok(QuickLaunchInfo {
            powershell_label: facts.powershell.label().to_string(),
            pc_manager: PcManagerInfo {
                installed: facts.pc_manager_installed,
                family: facts.pc_manager_family,
                app_id: facts.pc_manager_app_id,
            },
        })
    }
    #[cfg(not(windows))]
    {
        Err(ToolError::Internal(
            "ERX Tools System Manager needs Windows.".into(),
        ))
    }
}

/// Opens one allowlisted target by id. A UAC dismissal is a neutral
/// "cancelled", not an error.
#[tauri::command]
pub fn quick_launch_open(id: String) -> Result<LaunchResult, ToolError> {
    #[cfg(windows)]
    {
        let spec = launch_spec(&id)
            .ok_or_else(|| ToolError::Internal("Unknown Quick Launch button.".into()))?;
        let outcome = match spec.kind {
            super::core::TargetKind::PowerShell => {
                let powershell = launch::resolve_powershell().map_err(ToolError::Internal)?;
                launch::launch(&spec, &powershell, None)
            }
            super::core::TargetKind::PcManager => {
                let coordinates = launch::detect_pc_manager();
                launch::launch(
                    &spec,
                    &launch::resolve_powershell().map_err(ToolError::Internal)?,
                    coordinates.as_ref(),
                )
            }
            _ => {
                let powershell = launch::resolve_powershell().map_err(ToolError::Internal)?;
                launch::launch(&spec, &powershell, None)
            }
        };
        let outcome = outcome.map_err(ToolError::Internal)?;
        Ok(LaunchResult {
            outcome: match outcome {
                launch::LaunchOutcome::Opened => "opened".into(),
                launch::LaunchOutcome::Cancelled => "cancelled".into(),
            },
        })
    }
    #[cfg(not(windows))]
    {
        Err(ToolError::Internal(
            "ERX Tools System Manager needs Windows.".into(),
        ))
    }
}
