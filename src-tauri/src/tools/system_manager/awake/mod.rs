//! Awake section: keeps the PC awake by temporarily setting the active
//! power plan's sleep/hibernate/display timeouts to Never (owner decision —
//! safer failure mode than the execution-state API; see docs/PROMPT_5_DECISIONS.md).
//!
//! Layering, mirroring the other tools: `core` is pure logic, `state_file`
//! is disk I/O only, `power` is the Windows API behind a trait (with an
//! in-memory fake for tests), `engine` is the testable state machine, and
//! `commands` is the thin Tauri layer.

pub mod commands;
pub mod core;
pub mod engine;
#[cfg(test)]
pub mod engine_test;
pub mod power;
pub mod state_file;

#[cfg(windows)]
#[cfg(test)]
mod live_test;
