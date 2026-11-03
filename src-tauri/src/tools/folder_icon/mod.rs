//! Folder Icon Changer backend.
//!
//! One geometry description of an original folder design produces both the
//! SVG preview and the PNG/ICO bitmaps, mirroring the QR tool's split:
//! pure, unit-tested logic files plus a thin Tauri command layer. The
//! Windows-specific apply/restore code (desktop.ini) arrives in checkpoint
//! 4B behind `#[cfg(windows)]`.

// Temporary: the types below are consumed by the preview command landing in
// checkpoint 4A-2 (same pattern as error.rs's allow(dead_code) until first
// use). Remove when the command exists.
#![allow(dead_code)]

pub mod colors;
pub mod settings;
