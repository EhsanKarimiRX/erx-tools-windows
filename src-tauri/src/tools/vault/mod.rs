//! Quantum Resistance Vault: quantum-resistant file encryption.
//!
//! Backend layout (filled in step by step):
//! - `wordlist`   : the embedded EFF large wordlist (7776 words).
//! - `passphrase` : passphrase generation, session state, normalization.
//! - `error`      : the vault error type (user-facing messages).
//! - `format`     : the .erx header (layout, parsing, validation).
//! - `kdf`        : Argon2id, HKDF subkeys, header commitment tag.
//! - `engine`     : streaming compress + encrypt / decrypt of one stream.
//! - `parts`      : split writer / joined reader for multi-part files.
//! - `space`      : free disk space check.
//! - `files`      : file-level encrypt / decrypt (temp files, rename).
//!
//! Hard rules for this module: no `unwrap()` / `expect()` / panicking
//! indexing in non-test code (the release profile uses `panic = "abort"`),
//! and never log words, keys or file contents.

// TEMPORARY: nothing calls this module yet. Remove this line when the
// Tauri commands are wired up (plan step V5) and re-check with clippy.
#![allow(dead_code)]

pub mod engine;
pub mod error;
pub mod files;
pub mod format;
pub mod kdf;
pub mod parts;
pub mod passphrase;
pub mod space;
pub mod wordlist;

#[cfg(test)]
mod engine_test;
#[cfg(test)]
mod files_test;
#[cfg(test)]
mod format_test;
#[cfg(test)]
mod parts_test;
#[cfg(test)]
mod passphrase_test;
#[cfg(test)]
mod wordlist_test;
