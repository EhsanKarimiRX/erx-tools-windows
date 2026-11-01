//! Tauri command layer for the Image Converter.
//!
//! Thin by design: validate input, call `core`, shape the result. Native
//! dialogs are opened here (Rust side) so the webview never needs dialog
//! permissions.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;

use super::core::{self, Inspection};
use crate::tools::ToolError;

/// Extensions offered in the native file dialog filter.
const DIALOG_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff"];

/// One candidate file as reported to the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectedFile {
    /// Absolute path (the key the UI sends back for conversion).
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    /// "image" (supported), "unsupported" (readable, not a supported image)
    /// or "invalid" (missing, folder, damaged, over a limit).
    pub kind: &'static str,
    /// Short display name of the detected format, e.g. "JPEG".
    pub format: Option<String>,
    /// Animated GIF/WebP or multi-page TIFF (first frame/page only).
    pub animated: bool,
    /// Short English reason, for unsupported/invalid files.
    pub reason: Option<String>,
}

/// Inspects dropped or picked files. Each file is classified from its
/// content; the command itself never fails, so one bad path cannot break a
/// whole drop.
#[tauri::command]
pub fn inspect_files(paths: Vec<String>) -> Vec<InspectedFile> {
    paths.iter().map(|path| inspect_one(path)).collect()
}

fn inspect_one(path: &str) -> InspectedFile {
    let file_name = Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().into_owned(),
    );

    match core::inspect_path(Path::new(path)) {
        Inspection::Image(info) => InspectedFile {
            path: path.to_string(),
            file_name,
            size_bytes: info.size_bytes,
            kind: "image",
            format: Some(info.format.label().to_string()),
            animated: info.animated,
            reason: None,
        },
        Inspection::NotSupported { detected } => {
            let reason = match detected {
                Some(name) => format!("{name} is not supported in v1"),
                None => "not a recognized image".to_string(),
            };
            InspectedFile {
                path: path.to_string(),
                file_name,
                size_bytes: 0,
                kind: "unsupported",
                format: None,
                animated: false,
                reason: Some(reason),
            }
        }
        Inspection::Invalid { reason } => InspectedFile {
            path: path.to_string(),
            file_name,
            size_bytes: 0,
            kind: "invalid",
            format: None,
            animated: false,
            reason: Some(reason),
        },
    }
}

/// Opens the native multi-select file dialog (Rust side, so the webview
/// needs no dialog permission). Returns absolute paths, or null on cancel.
#[tauri::command]
pub fn pick_image_files(app: tauri::AppHandle) -> Option<Vec<String>> {
    let picked = app
        .dialog()
        .file()
        .add_filter("Supported images", DIALOG_EXTENSIONS)
        .add_filter("All files", &["*"])
        .blocking_pick_files();
    picked.map(|files| {
        files
            .into_iter()
            .filter_map(|file| file.into_path().ok())
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    })
}

/// Opens the native folder-picker dialog. Returns the absolute path, or
/// null on cancel.
#[tauri::command]
pub fn pick_output_folder(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|folder| folder.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
}

/// Raw options payload from the UI (unvalidated; numbers arrive as JSON).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertOptionsDto {
    pub format: String,
    pub jpeg_quality: i32,
    pub background: String,
    pub output_folder: Option<String>,
}

/// Temporary checkpoint-2A command: proves the JS → Rust bridge with the
/// real options payload and validates it. Replaced by the actual conversion
/// pipeline in checkpoint 2C.
#[tauri::command]
pub fn probe_conversion(count: u32, options: ConvertOptionsDto) -> Result<String, ToolError> {
    let options = core::ConvertOptions::from_ui(
        &options.format,
        options.jpeg_quality,
        &options.background,
        options.output_folder.as_deref(),
    )
    .map_err(ToolError::Internal)?;
    if count == 0 {
        return Err(ToolError::Internal("Add at least one file first.".into()));
    }
    Ok(format!(
        "Bridge OK: would convert {count} file(s) to {}.",
        options.describe()
    ))
}
