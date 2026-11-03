//! Tauri command layer for the Folder Icon tool. Thin by design: shape the
//! DTOs, run the pure `core` pipeline. All validation happens in
//! `settings.rs`; the webview needs no permissions beyond `core:default`.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use super::core;
use crate::tools::ToolError;

/// Opens the native multi-select folder picker from the Rust side (no
/// webview dialog permission). Returns the absolute paths, or null on
/// cancel.
#[tauri::command]
pub fn folder_icon_pick_folders(app: AppHandle) -> Option<Vec<String>> {
    let picked = app.dialog().file().blocking_pick_folders();
    picked.map(|folders| {
        folders
            .into_iter()
            .filter_map(|folder| folder.into_path().ok())
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    })
}

/// The eligibility verdict in a flat UI-friendly shape.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EligibilityDto {
    /// "allowed" | "warning" | "blocked"
    pub status: &'static str,
    /// The secondary reason line when blocked.
    pub reason: Option<String>,
    /// The non-blocking warning line when warning.
    pub warning: Option<String>,
}

fn eligibility_dto(eligibility: &super::eligibility::Eligibility) -> EligibilityDto {
    use super::eligibility::Eligibility as E;
    match eligibility {
        E::Allowed => EligibilityDto {
            status: "allowed",
            reason: None,
            warning: None,
        },
        E::AllowedWithWarning(code) => EligibilityDto {
            status: "warning",
            reason: None,
            warning: Some(code.message().to_string()),
        },
        E::Blocked(reason) => EligibilityDto {
            status: "blocked",
            reason: Some(reason.message().to_string()),
            warning: None,
        },
    }
}

/// Raw settings as sent by the UI (unvalidated; Rust decides).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderIconSettingsDto {
    pub base_color: String,
    pub pattern: String,
    pub pattern_color: String,
    pub pattern_size: i32,
    pub pattern_opacity: i32,
}

/// The live preview: the SVG document plus non-blocking hints.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderIconPreview {
    pub svg: String,
    pub warnings: Vec<String>,
    /// Small previews rendered exactly like the .ico frames (PNG data
    /// URLs), so what the UI shows at 16/32/48 px IS the icon data.
    pub frames: Vec<FramePreview>,
}

/// One small preview frame.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FramePreview {
    pub size: u32,
    pub data_url: String,
}

/// Small preview frame sizes, matching the smallest .ico entries.
const SMALL_FRAME_SIZES: [u32; 3] = [16, 32, 48];

/// `data:image/png;base64,…` for a PNG buffer (the base64 crate is already
/// in the project via the QR tool's logo embedding).
fn png_data_url(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png)
    )
}

/// Builds the folder icon SVG for the live preview plus the small preview
/// frames rendered on the exact .ico path.
#[tauri::command]
pub fn folder_icon_preview_svg(
    settings: FolderIconSettingsDto,
) -> Result<FolderIconPreview, ToolError> {
    let settings = super::settings::IconSettings::from_parts(
        &settings.base_color,
        &settings.pattern,
        &settings.pattern_color,
        settings.pattern_size,
        settings.pattern_opacity,
    )
    .map_err(ToolError::Internal)?;
    let built = core::build(&settings);
    let frames = SMALL_FRAME_SIZES
        .iter()
        .map(|&size| {
            super::raster::render_frame_png(&built.params, size)
                .map(|png| FramePreview {
                    size,
                    data_url: png_data_url(&png),
                })
                .map_err(ToolError::Internal)
        })
        .collect::<Result<Vec<_>, ToolError>>()?;
    Ok(FolderIconPreview {
        svg: built.svg,
        warnings: built.warnings,
        frames,
    })
}

/// Inspect result: what the folder currently shows plus the
/// eligibility verdict for the status chip.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(windows)]
pub struct InspectResultDto {
    /// "default" | "custom_by_erx" | "custom_by_other"
    pub state: &'static str,
    pub eligibility: EligibilityDto,
}

/// Inspects a folder (read-only) for the UI: the current icon state plus
/// whether applying would be allowed/warned/blocked.
#[cfg(windows)]
#[tauri::command]
pub fn folder_icon_inspect(path: String) -> Result<InspectResultDto, ToolError> {
    let folder = std::path::PathBuf::from(&path);
    let resolved = super::eligibility::resolve(&folder);
    let env = super::platform::gather_environment();
    let facts = super::platform::path_facts(&resolved);
    let eligibility = super::eligibility::evaluate(&resolved, facts, &env);
    let state = super::apply::inspect_folder(&folder).map_err(ToolError::Internal)?;
    Ok(InspectResultDto {
        state: match state {
            super::apply::IconState::Default => "default",
            super::apply::IconState::CustomByErx => "custom_by_erx",
            super::apply::IconState::CustomByOther => "custom_by_other",
        },
        eligibility: eligibility_dto(&eligibility),
    })
}

/// Applies the icon to ONE folder. Re-validates everything in Rust;
/// returns a short friendly success message.
#[cfg(windows)]
#[tauri::command]
pub fn folder_icon_apply(
    path: String,
    settings: FolderIconSettingsDto,
) -> Result<String, ToolError> {
    let settings = super::settings::IconSettings::from_parts(
        &settings.base_color,
        &settings.pattern,
        &settings.pattern_color,
        settings.pattern_size,
        settings.pattern_opacity,
    )
    .map_err(ToolError::Internal)?;
    let built = core::build(&settings);
    super::apply::apply_icon(std::path::Path::new(&path), &built.params)
        .map(|_| "Icon applied.".to_string())
        .map_err(ToolError::Internal)
}

/// Restores the default icon on ONE folder.
#[cfg(windows)]
#[tauri::command]
pub fn folder_icon_restore(path: String) -> Result<String, ToolError> {
    let outcome =
        super::apply::restore_icon(std::path::Path::new(&path)).map_err(ToolError::Internal)?;
    Ok(match outcome {
        super::apply::RestoreOutcome::Restored => "Default icon restored.".to_string(),
        super::apply::RestoreOutcome::AlreadyDefault => {
            "The folder already shows the default icon.".to_string()
        }
    })
}

// ----- batch processing (spec 4.4: sequential, capped, per-folder
// results streamed through a Channel, cancellable) -----

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::State;
use tauri::ipc::Channel;

/// Shared state of the running batch (cancel flag).
#[derive(Default)]
pub struct BatchState {
    pub cancel: Arc<AtomicBool>,
}

/// One per-folder result streamed while the batch runs.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BatchEvent {
    pub index: usize,
    pub path: String,
    /// "applied" | "restored" | "skipped" | "failed"
    pub status: &'static str,
    pub detail: Option<String>,
}

/// Final counts, returned when the batch command resolves.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummaryDto {
    pub done: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Default)]
struct BatchCounters {
    done: usize,
    skipped: usize,
    failed: usize,
}

/// Asks the running batch to stop starting new folders (folders already
/// processed keep their result). Safe to call when nothing is running.
#[tauri::command]
pub fn folder_icon_cancel_batch(state: State<BatchState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

/// Runs one folder through the eligibility re-check (right before the
/// operation, spec 4.3) and the given operation, sending the per-folder
/// event and updating the counters.
#[cfg(windows)]
fn batch_step(
    folder: &std::path::Path,
    operation: impl Fn(&std::path::Path) -> Result<(), String>,
    ok_status: &'static str,
    index: usize,
    channel: &Channel<BatchEvent>,
    counters: &Mutex<BatchCounters>,
) {
    // Re-check eligibility right before touching the folder: blocked
    // folders are Skipped, not Failed (spec 4.4).
    let resolved = super::eligibility::resolve(folder);
    let env = super::platform::gather_environment();
    let facts = super::platform::path_facts(&resolved);
    let blocked = match super::eligibility::evaluate(&resolved, facts, &env) {
        super::eligibility::Eligibility::Allowed
        | super::eligibility::Eligibility::AllowedWithWarning(_) => None,
        super::eligibility::Eligibility::Blocked(reason) => Some(reason.message().to_string()),
    };
    let result = match blocked {
        Some(reason) => Err((ok_status, "skipped", Some(reason))),
        None => match operation(folder) {
            Ok(()) => Ok(()),
            Err(message) => Err((ok_status, "failed", Some(message))),
        },
    };
    let mut counters = counters.lock().expect("batch thread owns the counters");
    let (status, detail) = match &result {
        Ok(()) => {
            counters.done += 1;
            (ok_status, None)
        }
        Err((_, status, detail)) => {
            match *status {
                "skipped" => counters.skipped += 1,
                "failed" => counters.failed += 1,
                _ => {}
            }
            (*status, detail.clone())
        }
    };
    let _ = channel.send(BatchEvent {
        index,
        path: folder.to_string_lossy().into_owned(),
        status,
        detail,
    });
}

/// Applies the icon to every given folder, strictly sequentially (spec
/// 4.4: never parallel). Blocked folders are skipped, one failing folder
/// never stops the batch, a cancelled batch skips the remaining folders.
#[cfg(windows)]
#[tauri::command]
pub fn folder_icon_apply_batch(
    state: State<BatchState>,
    on_progress: Channel<BatchEvent>,
    paths: Vec<String>,
    settings: FolderIconSettingsDto,
) -> Result<BatchSummaryDto, ToolError> {
    super::apply::validate_batch_len(paths.len()).map_err(ToolError::Internal)?;
    let settings = super::settings::IconSettings::from_parts(
        &settings.base_color,
        &settings.pattern,
        &settings.pattern_color,
        settings.pattern_size,
        settings.pattern_opacity,
    )
    .map_err(ToolError::Internal)?;
    let built = core::build(&settings);

    state.cancel.store(false, Ordering::Relaxed);
    let counters = Mutex::new(BatchCounters::default());
    for (index, path) in paths.iter().enumerate() {
        if state.cancel.load(Ordering::Relaxed) {
            let _ = on_progress.send(BatchEvent {
                index,
                path: path.clone(),
                status: "skipped",
                detail: Some("Cancelled.".into()),
            });
            counters.lock().expect("single thread").skipped += 1;
            continue;
        }
        batch_step(
            std::path::Path::new(path),
            |folder| super::apply::apply_icon(folder, &built.params),
            "applied",
            index,
            &on_progress,
            &counters,
        );
    }
    let counters = counters.into_inner().expect("single thread");
    Ok(BatchSummaryDto {
        done: counters.done,
        skipped: counters.skipped,
        failed: counters.failed,
    })
}

/// Restores the default icon on every given folder, same rules as the
/// apply batch.
#[cfg(windows)]
#[tauri::command]
pub fn folder_icon_restore_batch(
    state: State<BatchState>,
    on_progress: Channel<BatchEvent>,
    paths: Vec<String>,
) -> Result<BatchSummaryDto, ToolError> {
    super::apply::validate_batch_len(paths.len()).map_err(ToolError::Internal)?;

    state.cancel.store(false, Ordering::Relaxed);
    let counters = Mutex::new(BatchCounters::default());
    for (index, path) in paths.iter().enumerate() {
        if state.cancel.load(Ordering::Relaxed) {
            let _ = on_progress.send(BatchEvent {
                index,
                path: path.clone(),
                status: "skipped",
                detail: Some("Cancelled.".into()),
            });
            counters.lock().expect("single thread").skipped += 1;
            continue;
        }
        batch_step(
            std::path::Path::new(path),
            super::apply::restore_icon_for_batch,
            "restored",
            index,
            &on_progress,
            &counters,
        );
    }
    let counters = counters.into_inner().expect("single thread");
    Ok(BatchSummaryDto {
        done: counters.done,
        skipped: counters.skipped,
        failed: counters.failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The batch event JSON the frontend consumes must keep its exact
    /// shape (tagged by "type", camelCase fields).
    #[test]
    fn batch_event_json_shape() {
        let event = BatchEvent {
            index: 3,
            path: r"C:\dev\one".into(),
            status: "applied",
            detail: None,
        };
        let json = serde_json::to_string(&event).expect("serializes");
        assert_eq!(
            json,
            r#"{"index":3,"path":"C:\\dev\\one","status":"applied","detail":null}"#
        );
        let skipped = BatchEvent {
            index: 0,
            path: r"C:\Windows".into(),
            status: "skipped",
            detail: Some("This is a protected Windows location.".into()),
        };
        let json = serde_json::to_string(&skipped).expect("serializes");
        assert!(json.contains(r#""status":"skipped""#));
        assert!(json.contains(r#""detail":"#));
    }

    /// The exact JSON shape the frontend sends must deserialize into our
    /// DTO (camelCase), so a rename on either side fails here instead of
    /// silently at runtime.
    #[test]
    fn frontend_json_shape_deserializes() {
        let dto: FolderIconSettingsDto = serde_json::from_str(
            r##"{
                "baseColor": "#0078D4",
                "pattern": "checker",
                "patternColor": "#FFFFFF",
                "patternSize": 2,
                "patternOpacity": 70
            }"##,
        )
        .expect("shape must parse");
        assert_eq!(dto.base_color, "#0078D4");
        assert_eq!(dto.pattern, "checker");
        assert_eq!(dto.pattern_size, 2);

        // The full pipeline runs through the same DTO path.
        let preview = folder_icon_preview_svg(dto).expect("builds");
        assert!(preview.svg.starts_with("<svg"));
        assert!(preview.warnings.is_empty());
        // The small previews arrive as PNG data URLs for every size.
        assert_eq!(
            preview
                .frames
                .iter()
                .map(|frame| frame.size)
                .collect::<Vec<_>>(),
            vec![16, 32, 48]
        );
        assert!(
            preview
                .frames
                .iter()
                .all(|frame| frame.data_url.starts_with("data:image/png;base64,"))
        );
    }

    #[test]
    fn invalid_input_becomes_a_friendly_error() {
        let dto: FolderIconSettingsDto = serde_json::from_str(
            r##"{
                "baseColor": "blue",
                "pattern": "none",
                "patternColor": "#FFFFFF",
                "patternSize": 3,
                "patternOpacity": 50
            }"##,
        )
        .unwrap();
        let error = folder_icon_preview_svg(dto).unwrap_err();
        assert!(error.to_string().contains("#RRGGBB"));
    }

    #[test]
    fn low_contrast_pattern_warns_through_the_dto_path() {
        let dto: FolderIconSettingsDto = serde_json::from_str(
            r##"{
                "baseColor": "#0067C0",
                "pattern": "horizontal",
                "patternColor": "#0055AA",
                "patternSize": 1,
                "patternOpacity": 100
            }"##,
        )
        .unwrap();
        let preview = folder_icon_preview_svg(dto).expect("builds");
        assert!(preview.warnings.iter().any(|w| w.contains("hard to see")));
    }
}
