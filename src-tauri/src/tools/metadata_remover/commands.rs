//! Tauri command layer for the Metadata Remover.
//!
//! Thin by design: validate the input, call `dispatch` and `pipeline`, shape
//! the result. Native dialogs and "show in Explorer" run on the Rust side, so
//! the webview needs no permission beyond `core:default`.
//!
//! One batch runs at a time (a busy flag). Cancelling stops the files that
//! are being cleaned (they remove their temporary files) and skips the rest.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;

use super::dispatch::analyze_path;
use super::model::{CleanOptions, Finding};
use super::pipeline::{CleanedFile, FileResult, clean_one_file};
use crate::tools::ToolError;

/// Largest number of files in one list (the same as the Image Converter).
pub const MAX_FILES: usize = 200;
/// Two files at a time: the work is limited by the disk, and the pixel check
/// of a big photo needs a lot of memory.
const MAX_WORKERS: usize = 2;
/// At most one progress event per file and interval.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const NO_METADATA: &str = "This file has no metadata.";
const CANCELLED: &str = "cancelled";

/// Extensions offered in the native file dialog filter.
const DIALOG_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "heic", "heif", "hif", "mp4", "mov",
    "mkv", "webm",
];

static CANCEL: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);

/// Only one batch may run at a time. Releases the flag on drop.
struct BusyGuard {
    flag: &'static AtomicBool,
}

impl BusyGuard {
    /// None when a batch is already running.
    fn acquire(flag: &'static AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| BusyGuard { flag })
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Raw options from the UI (unvalidated).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionsDto {
    pub keep_orientation: bool,
    pub keep_color_profile: bool,
    pub output_folder: Option<String>,
}

#[derive(Debug)]
struct ValidOptions {
    clean: CleanOptions,
    out_folder: Option<PathBuf>,
}

fn validate(dto: &OptionsDto) -> Result<ValidOptions, String> {
    let out_folder = match dto.output_folder.as_deref() {
        Some(text) if !text.trim().is_empty() => {
            let path = PathBuf::from(text);
            if !path.is_absolute() {
                return Err("The output folder must be an absolute path.".to_string());
            }
            if !path.is_dir() {
                return Err("The chosen output folder does not exist.".to_string());
            }
            Some(path)
        }
        _ => None,
    };
    Ok(ValidOptions {
        clean: CleanOptions {
            keep_orientation: dto.keep_orientation,
            keep_color_profile: dto.keep_color_profile,
        },
        out_folder,
    })
}

fn check_count(count: usize) -> Result<(), String> {
    if count > MAX_FILES {
        return Err(format!("At most {MAX_FILES} files can be added at once."));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Dialogs and "show in Explorer"
// ---------------------------------------------------------------------------

/// Opens the native multi-select file dialog. Returns absolute paths, or
/// null on cancel.
#[tauri::command]
pub fn mr_pick_files(app: tauri::AppHandle) -> Option<Vec<String>> {
    let picked = app
        .dialog()
        .file()
        .add_filter("Photos and videos", DIALOG_EXTENSIONS)
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

/// Opens the native folder-picker dialog. Returns the absolute path, or null
/// on cancel.
#[tauri::command]
pub fn mr_pick_output_folder(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|folder| folder.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
}

/// Shows the given files selected in Windows Explorer.
#[tauri::command]
pub fn mr_reveal_files(paths: Vec<String>) -> Result<(), ToolError> {
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
// Scanning (the "found" list before cleaning)
// ---------------------------------------------------------------------------

/// One file as reported to the UI after a scan.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedFile {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    /// "found" (hidden data exists), "clean" (nothing to remove) or
    /// "unsupported" (cannot be handled; see `reason`).
    pub kind: &'static str,
    pub format: Option<&'static str>,
    pub findings: Vec<Finding>,
    pub kept: Vec<String>,
    pub reason: Option<String>,
}

fn file_name_of(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Scans one file: reads only headers, never changes anything.
pub fn scan_one(path: &str, options: &CleanOptions) -> ScannedFile {
    let file_name = file_name_of(path);
    let size_bytes = std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    match analyze_path(Path::new(path), options) {
        Ok(analysis) => {
            let found = analysis.has_changes();
            ScannedFile {
                path: path.to_string(),
                file_name,
                size_bytes,
                kind: if found { "found" } else { "clean" },
                format: Some(analysis.format.label()),
                findings: analysis.findings,
                kept: analysis.kept,
                reason: if found {
                    None
                } else {
                    Some(NO_METADATA.to_string())
                },
            }
        }
        Err(reason) => ScannedFile {
            path: path.to_string(),
            file_name,
            size_bytes,
            kind: "unsupported",
            format: None,
            findings: Vec::new(),
            kept: Vec::new(),
            reason: Some(reason),
        },
    }
}

/// Scans dropped or picked files. The command itself never fails because of
/// one bad file: every file gets its own result.
#[tauri::command(async)]
pub fn mr_scan_files(
    paths: Vec<String>,
    options: OptionsDto,
) -> Result<Vec<ScannedFile>, ToolError> {
    check_count(paths.len()).map_err(ToolError::Internal)?;
    let clean = CleanOptions {
        keep_orientation: options.keep_orientation,
        keep_color_profile: options.keep_color_profile,
    };
    Ok(paths.iter().map(|path| scan_one(path, &clean)).collect())
}

// ---------------------------------------------------------------------------
// Cleaning a batch
// ---------------------------------------------------------------------------

/// Per-file progress pushed to the UI through a channel.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub index: usize,
    /// "working", "done", "nothing" (no metadata), "failed" or "skipped".
    pub status: &'static str,
    pub done: u64,
    pub total: u64,
    pub output_path: Option<String>,
    pub output_size: Option<u64>,
    pub source_size: Option<u64>,
    pub removed: Vec<Finding>,
    pub kept: Vec<String>,
    pub pixels_verified: bool,
    pub reason: Option<String>,
}

impl ProgressEvent {
    fn base(index: usize, status: &'static str) -> Self {
        ProgressEvent {
            index,
            status,
            done: 0,
            total: 0,
            output_path: None,
            output_size: None,
            source_size: None,
            removed: Vec::new(),
            kept: Vec::new(),
            pixels_verified: false,
            reason: None,
        }
    }

    pub fn working(index: usize, done: u64, total: u64) -> Self {
        ProgressEvent {
            done,
            total,
            ..Self::base(index, "working")
        }
    }

    fn cleaned(index: usize, file: CleanedFile) -> Self {
        ProgressEvent {
            output_path: Some(file.output_path.to_string_lossy().into_owned()),
            output_size: Some(file.output_size),
            source_size: Some(file.source_size),
            removed: file.removed,
            kept: file.kept,
            pixels_verified: file.pixels_verified,
            ..Self::base(index, "done")
        }
    }

    fn nothing(index: usize, kept: Vec<String>) -> Self {
        ProgressEvent {
            kept,
            reason: Some(NO_METADATA.to_string()),
            ..Self::base(index, "nothing")
        }
    }

    fn failed(index: usize, reason: String) -> Self {
        ProgressEvent {
            reason: Some(reason),
            ..Self::base(index, "failed")
        }
    }

    fn skipped(index: usize, reason: &str) -> Self {
        ProgressEvent {
            reason: Some(reason.to_string()),
            ..Self::base(index, "skipped")
        }
    }
}

/// Final numbers of one batch.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummary {
    pub cleaned: u32,
    pub already_clean: u32,
    pub failed: u32,
    pub cancelled: u32,
}

enum Outcome {
    Cleaned,
    AlreadyClean,
    Failed,
    Cancelled,
}

fn process_one(
    index: usize,
    path: &Path,
    out_folder: Option<&Path>,
    options: &CleanOptions,
    cancel: &AtomicBool,
    emit: &(dyn Fn(ProgressEvent) + Sync),
) -> Outcome {
    if cancel.load(Ordering::Relaxed) {
        emit(ProgressEvent::skipped(index, CANCELLED));
        return Outcome::Cancelled;
    }
    emit(ProgressEvent::working(index, 0, 0));
    let mut last: Option<Instant> = None;
    let mut progress = |done: u64, total: u64| -> bool {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let due = done >= total || last.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL);
        if due {
            last = Some(Instant::now());
            emit(ProgressEvent::working(index, done, total));
        }
        true
    };
    match clean_one_file(path, out_folder, options, &mut progress) {
        Ok(FileResult::Cleaned(file)) => {
            emit(ProgressEvent::cleaned(index, file));
            Outcome::Cleaned
        }
        Ok(FileResult::NothingToRemove { kept, .. }) => {
            emit(ProgressEvent::nothing(index, kept));
            Outcome::AlreadyClean
        }
        Ok(FileResult::Cancelled) => {
            emit(ProgressEvent::skipped(index, CANCELLED));
            Outcome::Cancelled
        }
        Err(reason) => {
            emit(ProgressEvent::failed(index, reason));
            Outcome::Failed
        }
    }
}

/// Cleans all files, starting them in list order with a small worker pool.
/// One failing file never stops the batch. `cancel` stops running files and
/// skips the rest.
pub fn run_batch(
    paths: &[PathBuf],
    out_folder: Option<&Path>,
    options: &CleanOptions,
    cancel: &AtomicBool,
    emit: &(dyn Fn(ProgressEvent) + Sync),
) -> BatchSummary {
    let total = paths.len();
    let next = AtomicUsize::new(0);
    let summary = Mutex::new(BatchSummary::default());
    let workers = std::thread::available_parallelism()
        .map_or(1, |count| count.get())
        .clamp(1, MAX_WORKERS)
        .min(total.max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else {
                        break;
                    };
                    let outcome = process_one(index, path, out_folder, options, cancel, emit);
                    if let Ok(mut counts) = summary.lock() {
                        match outcome {
                            Outcome::Cleaned => counts.cleaned += 1,
                            Outcome::AlreadyClean => counts.already_clean += 1,
                            Outcome::Failed => counts.failed += 1,
                            Outcome::Cancelled => counts.cancelled += 1,
                        }
                    }
                }
            });
        }
    });
    summary
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Cleans the files; progress streams through `on_progress` and the command
/// returns the final summary.
#[tauri::command(async)]
pub fn mr_clean_files(
    on_progress: Channel<ProgressEvent>,
    paths: Vec<String>,
    options: OptionsDto,
) -> Result<BatchSummary, ToolError> {
    if paths.is_empty() {
        return Err(ToolError::Internal("Add at least one file first.".into()));
    }
    check_count(paths.len()).map_err(ToolError::Internal)?;
    let valid = validate(&options).map_err(ToolError::Internal)?;
    let Some(_guard) = BusyGuard::acquire(&BUSY) else {
        return Err(ToolError::Internal(
            "Another cleaning is already running.".into(),
        ));
    };
    CANCEL.store(false, Ordering::Relaxed);
    let items: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let channel = Mutex::new(on_progress);
    let emit = |event: ProgressEvent| {
        if let Ok(guard) = channel.lock() {
            let _ = guard.send(event);
        }
    };
    Ok(run_batch(
        &items,
        valid.out_folder.as_deref(),
        &valid.clean,
        &CANCEL,
        &emit,
    ))
}

/// Asks the running batch to stop. Safe to call when nothing is running (the
/// next batch resets the flag).
#[tauri::command]
pub fn mr_cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::metadata_remover::png::make_chunk;
    use image::{ExtendedColorType, ImageEncoder};
    use std::collections::BTreeSet;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("erx-mr-cmd-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn png_bytes() -> Vec<u8> {
        let image = image::RgbImage::from_fn(8, 8, |x, y| {
            image::Rgb([(x * 16) as u8, (y * 16) as u8, 128])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(image.as_raw(), 8, 8, ExtendedColorType::Rgb8)
            .unwrap();
        out.into_inner()
    }

    /// A real PNG with a text chunk after the header and extra bytes at the end.
    fn dirty_png() -> Vec<u8> {
        let base = png_bytes();
        let mut data = base[..33].to_vec();
        data.extend(make_chunk(b"tEXt", b"Comment\0hello"));
        data.extend_from_slice(&base[33..]);
        data.extend_from_slice(b"TRAILING-DATA");
        data
    }

    fn options_dto(folder: Option<String>) -> OptionsDto {
        OptionsDto {
            keep_orientation: true,
            keep_color_profile: false,
            output_folder: folder,
        }
    }

    #[test]
    fn options_are_validated() {
        let none = validate(&options_dto(None)).unwrap();
        assert_eq!(none.out_folder, None);
        assert!(none.clean.keep_orientation);
        assert!(!none.clean.keep_color_profile);

        let empty = validate(&options_dto(Some("  ".to_string()))).unwrap();
        assert_eq!(empty.out_folder, None);

        assert_eq!(
            validate(&options_dto(Some("relative\\path".to_string()))).unwrap_err(),
            "The output folder must be an absolute path."
        );
        assert_eq!(
            validate(&options_dto(Some("C:/definitely/not/here".to_string()))).unwrap_err(),
            "The chosen output folder does not exist."
        );

        let dir = temp_dir("options");
        let valid = validate(&options_dto(Some(dir.to_str().unwrap().to_string()))).unwrap();
        assert_eq!(valid.out_folder, Some(dir.clone()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn too_many_files_are_refused() {
        assert!(check_count(0).is_ok());
        assert!(check_count(200).is_ok());
        assert_eq!(
            check_count(201).unwrap_err(),
            "At most 200 files can be added at once."
        );
    }

    #[test]
    fn scan_reports_found_clean_and_unsupported() {
        let dir = temp_dir("scan");
        let dirty = dir.join("dirty.png");
        let clean = dir.join("clean.png");
        let text = dir.join("notes.txt");
        std::fs::write(&dirty, dirty_png()).unwrap();
        std::fs::write(&clean, png_bytes()).unwrap();
        std::fs::write(&text, b"just some text, not a photo").unwrap();
        let options = CleanOptions::default();

        let found = scan_one(dirty.to_str().unwrap(), &options);
        assert_eq!(found.kind, "found");
        assert_eq!(found.format, Some("PNG"));
        assert_eq!(found.file_name, "dirty.png");
        assert_eq!(found.size_bytes, dirty_png().len() as u64);
        assert!(!found.findings.is_empty());
        assert_eq!(found.reason, None);

        let nothing = scan_one(clean.to_str().unwrap(), &options);
        assert_eq!(nothing.kind, "clean");
        assert_eq!(nothing.format, Some("PNG"));
        assert!(nothing.findings.is_empty());
        assert_eq!(
            nothing.reason.as_deref(),
            Some("This file has no metadata.")
        );

        let bad = scan_one(text.to_str().unwrap(), &options);
        assert_eq!(bad.kind, "unsupported");
        assert_eq!(bad.format, None);
        assert_eq!(
            bad.reason.as_deref(),
            Some("not a supported photo or video file")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_batch_reports_every_file_and_a_summary() {
        let dir = temp_dir("batch");
        let paths = vec![
            dir.join("dirty.png"),
            dir.join("clean.png"),
            dir.join("notes.txt"),
            dir.join("missing.png"),
        ];
        std::fs::write(&paths[0], dirty_png()).unwrap();
        std::fs::write(&paths[1], png_bytes()).unwrap();
        std::fs::write(&paths[2], b"just some text, not a photo").unwrap();

        let events: Mutex<Vec<ProgressEvent>> = Mutex::new(Vec::new());
        let emit = |event: ProgressEvent| events.lock().unwrap().push(event);
        let cancel = AtomicBool::new(false);
        let summary = run_batch(&paths, None, &CleanOptions::default(), &cancel, &emit);
        assert_eq!(
            summary,
            BatchSummary {
                cleaned: 1,
                already_clean: 1,
                failed: 2,
                cancelled: 0
            }
        );

        let events = events.into_inner().unwrap();
        let finals = |index: usize| -> Vec<&ProgressEvent> {
            events
                .iter()
                .filter(|event| event.index == index && event.status != "working")
                .collect()
        };
        for index in 0..4 {
            assert_eq!(finals(index).len(), 1, "index {index}");
            assert!(
                events
                    .iter()
                    .any(|event| event.index == index && event.status == "working"),
                "index {index} has no working event"
            );
        }
        let done = finals(0)[0];
        assert_eq!(done.status, "done");
        assert!(
            done.output_path
                .as_deref()
                .unwrap()
                .ends_with("dirtyERX.png")
        );
        assert!(done.pixels_verified);
        assert!(!done.removed.is_empty());
        assert_eq!(finals(1)[0].status, "nothing");
        assert_eq!(
            finals(1)[0].reason.as_deref(),
            Some("This file has no metadata.")
        );
        assert_eq!(finals(2)[0].status, "failed");
        assert_eq!(
            finals(2)[0].reason.as_deref(),
            Some("not a supported photo or video file")
        );
        assert_eq!(finals(3)[0].status, "failed");
        assert_eq!(
            finals(3)[0].reason.as_deref(),
            Some("file not found or not accessible")
        );

        let names: BTreeSet<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let expected: BTreeSet<String> = ["dirty.png", "clean.png", "notes.txt", "dirtyERX.png"]
            .iter()
            .map(|name| name.to_string())
            .collect();
        assert_eq!(names, expected);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_cancelled_batch_skips_everything() {
        let dir = temp_dir("cancelled");
        let paths = vec![dir.join("a.png"), dir.join("b.png")];
        std::fs::write(&paths[0], dirty_png()).unwrap();
        std::fs::write(&paths[1], dirty_png()).unwrap();

        let events: Mutex<Vec<ProgressEvent>> = Mutex::new(Vec::new());
        let emit = |event: ProgressEvent| events.lock().unwrap().push(event);
        let cancel = AtomicBool::new(true);
        let summary = run_batch(&paths, None, &CleanOptions::default(), &cancel, &emit);
        assert_eq!(
            summary,
            BatchSummary {
                cleaned: 0,
                already_clean: 0,
                failed: 0,
                cancelled: 2
            }
        );
        let events = events.into_inner().unwrap();
        assert_eq!(events.len(), 2);
        for event in &events {
            assert_eq!(event.status, "skipped");
            assert_eq!(event.reason.as_deref(), Some("cancelled"));
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancelling_during_a_file_leaves_no_new_file() {
        let dir = temp_dir("cancel-running");
        let paths = vec![dir.join("a.png")];
        std::fs::write(&paths[0], dirty_png()).unwrap();

        let cancel = AtomicBool::new(false);
        let events: Mutex<Vec<ProgressEvent>> = Mutex::new(Vec::new());
        // The cancel request arrives while the file is being cleaned: the
        // first "working" event sets the flag.
        let emit = |event: ProgressEvent| {
            if event.status == "working" {
                cancel.store(true, Ordering::Relaxed);
            }
            events.lock().unwrap().push(event);
        };
        let summary = run_batch(&paths, None, &CleanOptions::default(), &cancel, &emit);
        assert_eq!(
            summary,
            BatchSummary {
                cleaned: 0,
                already_clean: 0,
                failed: 0,
                cancelled: 1
            }
        );
        let events = events.into_inner().unwrap();
        let statuses: Vec<&str> = events.iter().map(|event| event.status).collect();
        assert_eq!(statuses, vec!["working", "skipped"]);
        assert_eq!(events[1].reason.as_deref(), Some("cancelled"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert_eq!(std::fs::read(&paths[0]).unwrap(), dirty_png());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn json_shapes_are_stable() {
        let event = ProgressEvent::working(2, 5, 10);
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"index":2,"status":"working","done":5,"total":10,"outputPath":null,"outputSize":null,"sourceSize":null,"removed":[],"kept":[],"pixelsVerified":false,"reason":null}"#
        );
        let summary = BatchSummary {
            cleaned: 1,
            already_clean: 2,
            failed: 3,
            cancelled: 4,
        };
        assert_eq!(
            serde_json::to_string(&summary).unwrap(),
            r#"{"cleaned":1,"alreadyClean":2,"failed":3,"cancelled":4}"#
        );
    }

    #[test]
    fn the_busy_flag_allows_one_batch_at_a_time() {
        static FLAG: AtomicBool = AtomicBool::new(false);
        let first = BusyGuard::acquire(&FLAG);
        assert!(first.is_some());
        assert!(BusyGuard::acquire(&FLAG).is_none());
        drop(first);
        assert!(BusyGuard::acquire(&FLAG).is_some());
    }
}
