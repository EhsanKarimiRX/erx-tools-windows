//! Desktop Icons section: show/hide the five classic system desktop icons
//! via the one HKCU HideDesktopIcons\NewStartPanel key (no admin), with the
//! absent-value-means-Windows-default rule handled in pure logic and the
//! desktop refresh kept to the minimal safe layers (see `refresh.rs`).

pub mod commands;
pub mod core;
#[cfg(windows)]
pub mod refresh;
pub mod registry;

#[cfg(windows)]
#[cfg(test)]
mod live_test;
