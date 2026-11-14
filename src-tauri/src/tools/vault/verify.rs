//! Verify-after-writing: decrypts a finished file set completely (which
//! checks every chunk, the STREAM end and the SHA-256 of the original) and
//! writes the result into nothing.

use std::io;
use std::path::Path;

use super::engine;
use super::error::VaultError;
use super::parts::SplitReader;

/// `progress(done, total)` counts encrypted bytes read; returning false
/// cancels with `Cancelled`. The typed words must be the ones used to
/// encrypt the file.
pub fn verify_file(
    first_part: &Path,
    typed_passphrase: &str,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<(), VaultError> {
    let mut reader = SplitReader::open(first_part)?;
    let total = reader.logical_total();
    engine::decrypt_stream(
        &mut reader,
        &mut io::sink(),
        typed_passphrase,
        &mut |done: u64| progress(done, total),
    )?;
    Ok(())
}
