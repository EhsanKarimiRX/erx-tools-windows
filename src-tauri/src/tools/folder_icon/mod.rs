//! Folder Icon Changer backend.
//!
//! One geometry description of an original folder design produces both the
//! SVG preview and the PNG/ICO bitmaps, mirroring the QR tool's split:
//! pure, unit-tested logic files plus a thin Tauri command layer. The
//! Windows-specific apply/restore code (desktop.ini) arrives in checkpoint
//! 4B behind `#[cfg(windows)]`.

#[cfg(windows)]
pub mod apply;
pub mod colors;
pub mod commands;
pub mod core;
pub mod eligibility;
pub mod geometry;
pub mod ico;
pub mod ini;
#[cfg(windows)]
pub mod notification;
#[cfg(windows)]
pub mod platform;
pub mod probe;
pub mod raster;
pub mod settings;
pub mod svg;
