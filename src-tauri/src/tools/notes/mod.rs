//! Secure Notes: encrypted container (core), see docs/NOTES_FORMAT.md.

// The tool is not wired up yet (a later step); remove this when it is.
#![allow(dead_code)]

pub mod clock;
pub mod crypto;
pub mod error;
pub mod format;
pub mod model;
pub mod search;
pub mod session;
pub mod store;

#[cfg(test)]
mod clock_test;
#[cfg(test)]
mod crypto_test;
#[cfg(test)]
mod format_test;
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
