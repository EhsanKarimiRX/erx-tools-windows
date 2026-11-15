//! Quantum Resistance Vault: quantum-resistant file encryption.
//!
//! Backend layout (filled in step by step):
//! - `wordlist`   : the embedded EFF large wordlist (7776 words).
//! - `passphrase` : passphrase generation, session state, normalization.
//! - `error`      : the vault error type (user-facing messages).
//! - `format`     : the .erx header (layout, parsing, validation).
//! - `kdf`        : Argon2id, HKDF subkeys, header commitment tag.
//! - `meta`       : the encrypted metadata block (original file name).
//! - `names`      : safe file names for restored files.
//! - `engine`     : streaming compress + encrypt / decrypt of one stream.
//! - `parts`      : split writer / joined reader for multi-part files.
//! - `space`      : free disk space check.
//! - `files`      : file-level encrypt / decrypt (temp files, rename).
//! - `archive`    : a folder tree as one byte stream (folder encryption).
//! - `folder`     : folder encrypt / decrypt (temp folder, rename).
//! - `verify`     : verify-after-writing (full decrypt into nothing).
//! - `runtime`    : tuning, progress, planning and jobs (no Tauri types).
//! - `commands`   : the Tauri commands (thin layer over `runtime`).
//!
//! Hard rules for this module: no `unwrap()` / `expect()` / panicking
//! indexing in non-test code (the release profile uses `panic = "abort"`),
//! and never log words, keys or file contents.

pub mod archive;
pub mod commands;
pub mod engine;
pub mod error;
pub mod files;
pub mod folder;
pub mod format;
pub mod kdf;
pub mod meta;
pub mod names;
pub mod parts;
pub mod passphrase;
pub mod runtime;
pub mod space;
pub mod verify;
pub mod wordlist;

#[cfg(test)]
mod engine_test;
#[cfg(test)]
mod files_test;
#[cfg(test)]
mod folder_test;
#[cfg(test)]
mod format_test;
#[cfg(test)]
mod large_test;
#[cfg(test)]
mod parts_test;
#[cfg(test)]
mod passphrase_test;
#[cfg(test)]
mod restore_test;
#[cfg(test)]
mod runtime_test;
#[cfg(test)]
mod wordlist_test;
