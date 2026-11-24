//! Secure Notes: encrypted container (core), see docs/NOTES_FORMAT.md.

// Items that are test-only carry their own `#[allow(dead_code)]`.

pub mod clipboard;
pub mod clock;
pub mod commands;
pub mod crypto;
pub mod error;
pub mod export;
pub mod format;
pub mod hardening;
pub mod instance;
pub mod model;
pub mod openflow;
pub mod search;
pub mod session;
pub mod store;
pub mod webprofile;
pub mod window;
pub mod windowgate;

#[cfg(test)]
mod clipboard_test;
#[cfg(test)]
mod clock_test;
#[cfg(test)]
mod commands_test;
#[cfg(test)]
mod crypto_test;
#[cfg(test)]
mod export_test;
#[cfg(test)]
mod format_test;
#[cfg(test)]
mod hardening_test;
#[cfg(test)]
mod instance_test;
#[cfg(test)]
mod model_test;
#[cfg(test)]
mod openflow_test;
#[cfg(test)]
mod search_test;
#[cfg(test)]
mod session_test;
#[cfg(test)]
mod store_test;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod webprofile_test;
#[cfg(test)]
mod window_test;
#[cfg(test)]
mod windowgate_test;
