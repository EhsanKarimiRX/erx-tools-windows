//! Organize section: one engine, three targets (Desktop, Downloads, Custom
//! folder). Top level only, loose files only, never overwrite (" (2)"
//! names instead), never delete (Undo only removes EMPTY folders the tool
//! itself created), honest preview before anything moves and an honest
//! report afterwards. Layering mirrors the other sections: pure logic in
//! `core`/`protected`, file system in `scan`/`run`, persistence in
//! `manifest`, orchestration in `engine`, Tauri wiring in `commands`.

pub mod commands;
pub mod core;
pub mod engine;
#[cfg(test)]
pub mod engine_test;
#[cfg(windows)]
#[cfg(test)]
mod live_test;
pub mod manifest;
pub mod protected;
pub mod scan;
