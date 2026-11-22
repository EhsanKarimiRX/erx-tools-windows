//! Secure Notes: encrypted container (core), see docs/NOTES_FORMAT.md.

// The tool is not wired up yet (a later step); remove this when it is.
#![allow(dead_code)]

pub mod crypto;
pub mod error;
pub mod format;

#[cfg(test)]
mod crypto_test;
#[cfg(test)]
mod format_test;
