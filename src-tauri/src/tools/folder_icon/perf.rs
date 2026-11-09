//! PERF-TRACE (temporary): runtime timeline logging for the Folder Icon
//! delay investigation. Enabled ONLY when the environment variable
//! `ERX_PERF=1` is set (read once, cached). Appends lines to
//! `%TEMP%\erx-folder-icon-perf.log`:
//!   `<unix_epoch_ms> | thread=<thread name or id> | <label>`
//! Wall-clock epoch milliseconds (SystemTime), so Rust marks and frontend
//! marks (Date.now()) are comparable. Works in debug AND release builds.
//! Never panics; all I/O errors are ignored; disabled = no-op except for
//! one env read.

use serde::Deserialize;
use std::io::Write;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// True when `ERX_PERF=1` (cached after the first read).
fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("ERX_PERF").as_deref() == Ok("1"))
}

/// The log file path: `%TEMP%\erx-folder-icon-perf.log`. `None` when TEMP
/// is not set (then nothing is written).
fn log_path() -> Option<std::path::PathBuf> {
    std::env::var_os("TEMP")
        .map(std::path::PathBuf::from)
        .map(|dir| dir.join("erx-folder-icon-perf.log"))
}

/// Current wall-clock epoch milliseconds (0 if the clock is before the
/// epoch, which never happens in practice — this only avoids a panic).
fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// The current thread's name, or its Debug id when unnamed.
fn thread_label() -> String {
    let thread = std::thread::current();
    match thread.name() {
        Some(name) => name.to_string(),
        None => format!("{:?}", thread.id()),
    }
}

/// Appends one mark line. No-op when disabled; never panics.
pub fn mark(label: &str) {
    if !enabled() {
        return;
    }
    let Some(path) = log_path() else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            file,
            "{} | thread={} | {}",
            epoch_ms(),
            thread_label(),
            label
        );
    }
}

/// One frontend timeline entry, buffered in the UI and flushed in batches
/// (so the frontend does NOT invoke once per mark — invoke calls queue on
/// the same main thread this instrumentation measures).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfEntry {
    pub ts: f64,
    pub label: String,
}

/// Writes the frontend's buffered entries with `thread=frontend` and
/// returns whether perf logging is enabled (so the UI can stop buffering
/// — and stop invoking — when `ERX_PERF` is off). Trivial by design.
#[tauri::command]
pub fn folder_icon_perf_flush(entries: Vec<PerfEntry>) -> bool {
    let on = enabled();
    if on {
        if let Some(path) = log_path() {
            if let Ok(mut file) =
                std::fs::OpenOptions::new().create(true).append(true).open(&path)
            {
                for entry in entries {
                    let _ = writeln!(
                        file,
                        "{:.0} | thread=frontend | {}",
                        entry.ts, entry.label
                    );
                }
            }
        }
    }
    on
}
