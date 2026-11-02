//! Backends for the app's tools, plus app-level commands.
//!
//! Every command lives in one submodule here and is registered in the single
//! `generate_handler!` list in `src/lib.rs` (the Tauri handler macro requires
//! one flat list). Adding a tool means: create a submodule, add one line in
//! `lib.rs`. Full recipe: docs/ADDING_A_TOOL.md.

pub mod app_info;
pub mod error;
pub mod image_convert;
pub mod qr_code;

pub use error::ToolError;
