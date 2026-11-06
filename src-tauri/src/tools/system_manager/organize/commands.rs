//! Tauri command layer for Organize: the native folder picker, the
//! read-only scan, the run with progress events, and the undo. The
//! frontend sends only a target id (plus, for Custom, the chosen path and
//! the scanned file list); the backend re-verifies everything before and
//! during the run — nothing is moved that was not previewed.

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;

use super::core::OrganizeTarget;
use super::engine;
use super::manifest;
use super::scan::{self, ScanPlan};
use crate::tools::ToolError;

/// Where the undo manifests live (app data dir), resolved like the other
/// sections resolve their folders.
fn state_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, ToolError> {
    use tauri::Manager;
    app.path().app_data_dir().map_err(|error| {
        ToolError::Internal(format!("Organize could not find its data folder: {error}"))
    })
}

fn target_of(id: &str) -> Result<OrganizeTarget, ToolError> {
    OrganizeTarget::from_id(id)
        .ok_or_else(|| ToolError::Internal("Unknown organize target.".into()))
}

// ---------------------------------------------------------------------------
// DTOs (camelCase on the wire)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFileDto {
    pub name: String,
    pub size: u64,
    pub modified_unix: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDto {
    pub name: &'static str,
    pub files: Vec<PlannedFileDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedDto {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanPlanDto {
    pub target: &'static str,
    pub root: String,
    pub on_onedrive: bool,
    pub categories: Vec<CategoryDto>,
    pub skipped: Vec<SkippedDto>,
    pub left_in_place: u32,
}

impl From<&ScanPlan> for ScanPlanDto {
    fn from(plan: &ScanPlan) -> Self {
        ScanPlanDto {
            target: plan.target.id(),
            root: plan.root.to_string_lossy().into_owned(),
            on_onedrive: plan.on_onedrive,
            categories: plan
                .categories
                .iter()
                .map(|category| CategoryDto {
                    name: category.name,
                    files: category
                        .files
                        .iter()
                        .map(|file| PlannedFileDto {
                            name: file.name.clone(),
                            size: file.size,
                            modified_unix: file.modified_unix,
                        })
                        .collect(),
                })
                .collect(),
            skipped: plan
                .skipped
                .iter()
                .map(|file| SkippedDto {
                    name: file.name.clone(),
                    reason: file.reason.clone(),
                })
                .collect(),
            left_in_place: plan.left_in_place,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFileInput {
    pub name: String,
    pub size: u64,
    pub modified_unix: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReportDto {
    pub moved: u32,
    pub skipped: Vec<SkippedDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoInfoDto {
    pub undoable: bool,
    /// The folder the last (custom) run organized, for the button label.
    pub root: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReportDto {
    pub summary: String,
    pub restored: u32,
    pub restored_renamed: u32,
    pub not_found: u32,
    pub failed: Vec<String>,
    pub notes: Vec<String>,
    pub folders_removed: u32,
    pub folders_kept: u32,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Opens the native folder picker for the Custom target (Rust side, so the
/// webview needs no dialog permission). Returns the absolute path or null.
#[tauri::command]
pub fn organize_pick_folder(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|folder| folder.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
}

/// Read-only scan for the preview. Nothing is written anywhere.
#[tauri::command]
pub fn organize_scan(
    target: String,
    custom_path: Option<String>,
) -> Result<ScanPlanDto, ToolError> {
    let target = target_of(&target)?;
    let root = scan::resolve_root(target, custom_path.as_deref()).map_err(ToolError::Internal)?;
    let plan = scan::scan_dir(&root, target).map_err(ToolError::Internal)?;
    Ok(ScanPlanDto::from(&plan))
}

/// Executes the scanned plan. Every file is re-checked (still exists, same
/// size and modified time); nothing outside the planned list is touched.
#[tauri::command]
pub fn organize_run(
    app: tauri::AppHandle,
    target: String,
    custom_path: Option<String>,
    files: Vec<PlannedFileInput>,
    on_progress: Channel<ProgressEvent>,
) -> Result<RunReportDto, ToolError> {
    let target = target_of(&target)?;
    let root = scan::resolve_root(target, custom_path.as_deref()).map_err(ToolError::Internal)?;
    let dir = state_dir(&app)?;
    let manifest_file = manifest::manifest_path(&dir, target);
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_secs()).ok())
        .unwrap_or(0);

    let planned: Vec<super::scan::PlannedFile> = files
        .into_iter()
        .map(|file| super::scan::PlannedFile {
            name: file.name,
            size: file.size,
            modified_unix: file.modified_unix,
        })
        .collect();

    let channel = on_progress;
    let report = engine::run(
        target,
        &root,
        &planned,
        &manifest_file,
        now_unix,
        &mut |done, total| {
            let _ = channel.send(ProgressEvent { done, total });
        },
    )
    .map_err(ToolError::Internal)?;

    Ok(RunReportDto {
        moved: report.moved,
        skipped: report
            .skipped
            .iter()
            .map(|file| SkippedDto {
                name: file.name.clone(),
                reason: file.reason.clone(),
            })
            .collect(),
    })
}

/// Whether this target has a manifest to undo (and which folder it was, so
/// the Custom button can name it).
#[tauri::command]
pub fn organize_undo_info(app: tauri::AppHandle, target: String) -> Result<UndoInfoDto, ToolError> {
    let target = target_of(&target)?;
    let dir = state_dir(&app)?;
    let manifest_file = manifest::manifest_path(&dir, target);
    if manifest_file.exists() {
        match manifest::read(&manifest_file) {
            Ok(entry) => Ok(UndoInfoDto {
                undoable: !entry.moves.is_empty() || !entry.mkdirs.is_empty(),
                root: Some(entry.root.to_string_lossy().into_owned()),
            }),
            Err(_) => Ok(UndoInfoDto {
                undoable: false,
                root: None,
            }),
        }
    } else {
        Ok(UndoInfoDto {
            undoable: false,
            root: None,
        })
    }
}

/// Undoes the most recent run for this target. The manifest stores the
/// root; the current custom picker value is irrelevant.
#[tauri::command]
pub fn organize_undo(app: tauri::AppHandle, target: String) -> Result<UndoReportDto, ToolError> {
    let target = target_of(&target)?;
    let dir = state_dir(&app)?;
    let manifest_file = manifest::manifest_path(&dir, target);
    let report = engine::undo(&manifest_file).map_err(ToolError::Internal)?;
    Ok(UndoReportDto {
        summary: report.summary(),
        restored: report.restored,
        restored_renamed: report.restored_renamed,
        not_found: report.not_found,
        failed: report.failed.clone(),
        notes: report.notes.clone(),
        folders_removed: report.folders_removed,
        folders_kept: report.folders_kept,
    })
}
