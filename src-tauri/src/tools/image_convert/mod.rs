//! Image Converter backend.
//!
//! - `core`: pure inspection/validation logic, no Tauri types (unit-testable;
//!   grows the conversion engine in checkpoint 2B).
//! - `commands`: the thin Tauri command layer the webview calls.

pub mod commands;
pub mod convert;
pub mod core;
pub mod naming;
pub mod pipeline;
