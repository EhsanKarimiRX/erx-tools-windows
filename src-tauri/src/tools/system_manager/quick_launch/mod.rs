//! Quick Launch section: one-click buttons for common Windows tools. The
//! frontend sends only a fixed id; `core.rs` maps it to a hard-coded
//! allowlisted target and `launch.rs` starts it with ShellExecuteExW
//! (full System32 paths, no PATH dependency, no process spawning for
//! detection). Honest about cancellations and missing tools.

pub mod commands;
pub mod core;
#[cfg(windows)]
pub mod launch;
