//! System Manager backend.
//!
//! Sections arrive step by step (5b Awake, 5c Desktop Icons, 5d Organize,
//! 5e Quick Launch), each as pure, unit-tested logic plus a thin Tauri
//! command layer, mirroring the other tools' split.

pub mod awake;
pub mod desktop_icons;
pub mod organize;
