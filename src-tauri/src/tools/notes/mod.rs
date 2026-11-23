//! Secure Notes: encrypted container (core), see docs/NOTES_FORMAT.md.

// A few items are used only by the tests today: `clock::days_from_civil`,
// `crypto::NotesKeys::header`, `store::NotesStore::with_ops` and
// `store::NotesStore::dir`. Remove this allow once the app itself uses them.
#![allow(dead_code)]

pub mod clock;
pub mod commands;
pub mod crypto;
pub mod error;
pub mod format;
pub mod hardening;
pub mod instance;
pub mod model;
pub mod search;
pub mod session;
pub mod store;
pub mod webprofile;

#[cfg(test)]
mod clock_test;
#[cfg(test)]
mod commands_test;
#[cfg(test)]
mod crypto_test;
#[cfg(test)]
mod format_test;
#[cfg(test)]
mod hardening_test;
#[cfg(test)]
mod instance_test;
#[cfg(test)]
mod model_test;
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
