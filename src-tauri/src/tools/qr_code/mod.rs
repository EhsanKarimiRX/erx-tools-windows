//! QR Code Generator backend.
//!
//! Layout follows the Image Converter pattern: pure, unit-testable logic
//! (`payload`, `colors`, `matrix`, `geometry`, `svg`, `core`) with a thin
//! Tauri layer in `commands`. Both export formats are drawn from the same
//! geometry so preview, SVG and PNG always match.

pub mod colors;
pub mod commands;
pub mod core;
pub mod geometry;
pub mod matrix;
pub mod payload;
pub mod svg;

#[cfg(test)]
mod readback_tests;
