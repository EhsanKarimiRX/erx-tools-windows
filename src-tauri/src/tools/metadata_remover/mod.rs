//! Metadata Remover backend: removes hidden data (EXIF, GPS, XMP, comments,
//! thumbnails, ...) from photos and videos WITHOUT re-encoding them.
//!
//! - `model`: data types shared by all handlers (findings, plan, options).
//! - `executor`: writes a plan as a stream into a file.
//! - `safe_write`: temporary names, no-overwrite move, output names.
//!
//! Every format handler reads a file with Read+Seek and returns an
//! `Analysis`. Pure logic only; the Tauri command layer comes later.

// Temporary: removed in the last step when everything is wired up.
#![allow(dead_code)]

pub mod bmp;
pub mod commands;
pub mod dispatch;
pub mod executor;
pub mod found;
pub mod gif;
pub mod heic;
pub mod jpeg;
pub mod mkv;
pub mod model;
pub mod mp4;
pub mod orientation;
pub mod patch;
pub mod pipeline;
pub mod png;
pub mod safe_write;
pub mod tiff;
pub mod webp;
