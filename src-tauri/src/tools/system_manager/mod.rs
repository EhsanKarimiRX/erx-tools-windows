//! System Manager backend.
//!
//! Sections (Awake, Quick Launch, the device card), each as pure,
//! unit-tested logic plus a thin Tauri command layer, mirroring the other
//! tools' split.

pub mod awake;
pub mod quick_launch;
pub mod system_info;
