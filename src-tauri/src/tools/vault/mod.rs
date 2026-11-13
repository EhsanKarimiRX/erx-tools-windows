//! Quantum Resistance Vault: quantum-resistant file encryption.
//!
//! Backend layout (filled in step by step):
//! - `wordlist`   : the embedded EFF large wordlist (7776 words).
//! - `passphrase` : passphrase generation, session state, normalization.
//!
//! Hard rules for this module: no `unwrap()` / `expect()` / panicking
//! indexing in non-test code (the release profile uses `panic = "abort"`),
//! and never log words, keys or file contents.

// TEMPORARY: nothing calls this module yet. Remove this line when the
// Tauri commands are wired up (plan step V5) and re-check with clippy.
#![allow(dead_code)]

pub mod passphrase;
pub mod wordlist;

#[cfg(test)]
mod passphrase_test;
#[cfg(test)]
mod wordlist_test;
