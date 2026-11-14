//! Free disk space check before encrypting.

use std::path::Path;

use super::error::VaultError;
use super::format::{CHUNK_SIZE, HEADER_LEN};
use super::parts::PART_HEADER_LEN;

/// Extra room kept free on top of the estimate (4 MiB).
const SAFETY_MARGIN: u64 = 4 * 1024 * 1024;
/// Poly1305 tag per chunk.
const TAG_BYTES: u64 = 16;
/// Size of the integrity trailer.
const TRAILER_BYTES: u64 = 40;

/// Worst-case size of the output for `input_len` bytes of input: even
/// incompressible data grows slightly under zstd, every chunk gets a tag,
/// and every part gets a header.
pub fn required_for_encrypt(input_len: u64, limit: Option<u64>) -> u64 {
    let chunk = u64::from(CHUNK_SIZE);
    let payload = input_len
        .saturating_add(input_len / 128)
        .saturating_add(1024);
    let stream = payload.saturating_add(TRAILER_BYTES);
    let chunks = stream / chunk + 1;
    let sealed = stream
        .saturating_add(chunks.saturating_mul(TAG_BYTES))
        .saturating_add(HEADER_LEN as u64);
    let parts = match limit {
        Some(limit) => {
            let usable = limit.saturating_sub(PART_HEADER_LEN as u64).max(1);
            sealed / usable + 1
        }
        None => 1,
    };
    sealed
        .saturating_add(parts.saturating_mul(PART_HEADER_LEN as u64))
        .saturating_add(SAFETY_MARGIN)
}

/// Bytes available to the current user on the drive holding `dir`, or
/// `None` when Windows cannot tell.
#[cfg(windows)]
pub fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available: u64 = 0;
    // SAFETY: `wide` is a valid NUL-terminated UTF-16 string that outlives
    // the call, `available` is a valid out-pointer, and the other two
    // out-pointers are allowed to be null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 { None } else { Some(available) }
}

#[cfg(not(windows))]
pub fn free_bytes(_dir: &Path) -> Option<u64> {
    None
}

/// Refuses to start when the destination drive clearly lacks room. When
/// the free space is unknown the check passes; a full disk during writing
/// is still reported as `DiskFull`.
pub fn check_space_for_encrypt(
    dir: &Path,
    input_len: u64,
    limit: Option<u64>,
) -> Result<(), VaultError> {
    let needed = required_for_encrypt(input_len, limit);
    match free_bytes(dir) {
        Some(free) if free < needed => Err(VaultError::NotEnoughSpace {
            needed_mib: needed.div_ceil(1024 * 1024),
        }),
        _ => Ok(()),
    }
}
