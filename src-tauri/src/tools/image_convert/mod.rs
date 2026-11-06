//! Image Converter backend.
//!
//! - `core`: pure inspection/validation logic, no Tauri types (unit-testable;
//!   grows the conversion engine in checkpoint 2B).
//! - `commands`: the thin Tauri command layer the webview calls.

pub mod commands;
pub mod convert;
pub mod core;
// TODO(6b): remove the temporary dead-code allowance once the converter
// calls into this module. Until then 6a's decoder exists but nothing in
// the app's behavior uses it.
#[allow(dead_code)]
pub mod heic;
pub mod naming;
pub mod pipeline;
