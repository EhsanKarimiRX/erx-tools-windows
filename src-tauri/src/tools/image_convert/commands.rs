//! Tauri command layer for the Image Converter.
//!
//! Thin by design: validate input, call `core`/`pipeline`, shape the result.
//! Native dialogs, conversion and folder-reveal all run on the Rust side so
//! the webview never needs permissions beyond `core:default`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;

use super::core::{self, ConvertOptions};
use super::pipeline::convert_one_file;
use crate::tools::ToolError;

/// Extensions offered in the native file dialog filter.
const DIALOG_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff", "heic", "heif", "hif",
];

/// Upper bound for parallel conversion workers. A single image may decode
/// into ~320 MB of pixels, so the pool is capped regardless of core count.
const MAX_WORKERS: usize = 4;

// ---------------------------------------------------------------------------
// Inspection + dialogs
// ---------------------------------------------------------------------------

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
        core::Inspection::Image(info) => InspectedFile {
            path: path.to_string(),
            file_name,
            size_bytes: info.size_bytes,
            kind: "image",
            format: Some(info.format.label().to_string()),
            animated: info.animated,
            reason: None,
        },
        core::Inspection::NotSupported { detected } => {
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
        core::Inspection::Failed { reason } => InspectedFile {
            path: path.to_string(),
            file_name,
            size_bytes: 0,
            kind: "unsupported",
            format: None,
            animated: false,
            reason: Some(reason),
        },
        core::Inspection::Invalid { reason } => InspectedFile {
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

/// Shows the given output files selected in Windows Explorer.
#[tauri::command]
pub fn reveal_outputs(paths: Vec<String>) -> Result<(), ToolError> {
    for path in &paths {
        if tauri_plugin_opener::reveal_item_in_dir(path).is_err() {
            return Err(ToolError::Internal(
                "Windows refused to open the folder.".into(),
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Batch conversion
// ---------------------------------------------------------------------------

/// Shared state of the running batch (cancel flag).
#[derive(Default)]
pub struct BatchState {
    cancel: Arc<AtomicBool>,
}

/// Per-file progress pushed to the UI through a channel, in list order.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    index: usize,
    status: &'static str,
    output_path: Option<String>,
    output_size: Option<u64>,
    reason: Option<String>,
}

impl ProgressEvent {
    fn converting(index: usize) -> Self {
        ProgressEvent {
            index,
            status: "converting",
            output_path: None,
            output_size: None,
            reason: None,
        }
    }

    fn done(index: usize, output_path: String, output_size: u64) -> Self {
        ProgressEvent {
            index,
            status: "done",
            output_path: Some(output_path),
            output_size: Some(output_size),
            reason: None,
        }
    }

    fn failed(index: usize, reason: String) -> Self {
        ProgressEvent {
            index,
            status: "failed",
            output_path: None,
            output_size: None,
            reason: Some(reason),
        }
    }

    fn skipped(index: usize, reason: &str) -> Self {
        ProgressEvent {
            index,
            status: "skipped",
            output_path: None,
            output_size: None,
            reason: Some(reason.to_string()),
        }
    }
}

/// Final numbers of one batch, returned when every worker finished.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummary {
    converted: u32,
    failed: u32,
    skipped: u32,
    output_bytes: u64,
}

/// Running counters shared by the workers.
#[derive(Default)]
struct BatchCounters {
    converted: u32,
    failed: u32,
    skipped: u32,
    output_bytes: u64,
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

/// Converts all files, starting them in list order with a bounded worker
/// pool. Progress streams through `on_progress`; the command returns the
/// final summary. One failing file never stops the batch. Cancelling stops
/// the START of new files; running ones finish.
#[tauri::command]
pub fn convert_files(
    state: State<BatchState>,
    on_progress: Channel<ProgressEvent>,
    paths: Vec<String>,
    options: ConvertOptionsDto,
) -> Result<BatchSummary, ToolError> {
    if paths.is_empty() {
        return Err(ToolError::Internal("Add at least one file first.".into()));
    }
    let options = ConvertOptions::from_ui(
        &options.format,
        options.jpeg_quality,
        &options.background,
        options.output_folder.as_deref(),
    )
    .map_err(ToolError::Internal)?;

    state.cancel.store(false, Ordering::Relaxed);
    let items: Arc<Vec<PathBuf>> = Arc::new(paths.into_iter().map(PathBuf::from).collect());
    let total = items.len();
    let next_index = Arc::new(AtomicUsize::new(0));
    let counters = Arc::new(Mutex::new(BatchCounters::default()));

    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, MAX_WORKERS);
    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let items = Arc::clone(&items);
            let next_index = Arc::clone(&next_index);
            let counters = Arc::clone(&counters);
            let cancel = Arc::clone(&state.cancel);
            let channel = on_progress.clone();
            let options = options.clone();
            std::thread::spawn(move || {
                loop {
                    let index = next_index.fetch_add(1, Ordering::Relaxed);
                    if index >= total {
                        break;
                    }
                    if cancel.load(Ordering::Relaxed) {
                        let _ = channel.send(ProgressEvent::skipped(index, "cancelled"));
                        if let Ok(mut counters) = counters.lock() {
                            counters.skipped += 1;
                        }
                        continue;
                    }
                    let _ = channel.send(ProgressEvent::converting(index));
                    match convert_one_file(&items[index], &options) {
                        Ok(outcome) => {
                            let _ = channel.send(ProgressEvent::done(
                                index,
                                outcome.output_path.to_string_lossy().into_owned(),
                                outcome.output_size,
                            ));
                            if let Ok(mut counters) = counters.lock() {
                                counters.converted += 1;
                                counters.output_bytes += outcome.output_size;
                            }
                        }
                        Err(reason) => {
                            let _ = channel.send(ProgressEvent::failed(index, reason));
                            if let Ok(mut counters) = counters.lock() {
                                counters.failed += 1;
                            }
                        }
                    }
                }
            })
        })
        .collect();
    for handle in handles {
        let _ = handle.join();
    }

    let counters = counters.lock().expect("workers have exited");
    Ok(BatchSummary {
        converted: counters.converted,
        failed: counters.failed,
        skipped: counters.skipped,
        output_bytes: counters.output_bytes,
    })
}

/// Asks the running batch to stop starting new files. Safe to call when
/// nothing is running (it just sets a flag the next batch resets).
#[tauri::command]
pub fn cancel_conversion(state: State<BatchState>) {
    state.cancel.store(true, Ordering::Relaxed);
}
