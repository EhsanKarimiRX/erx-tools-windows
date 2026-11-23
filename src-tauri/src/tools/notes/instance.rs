//! The one-instance guard of the Secure Notes window.
//!
//! A named mutex lives while any handle to it is open, and Windows closes all
//! handles of a process when it dies. A guard therefore needs no cleanup after
//! a crash: the mutex simply disappears with the process.

use std::ffi::OsStr;
use std::fmt;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

use super::error::NotesError;

/// The mutex name that marks the one Secure Notes window.
pub const NOTES_INSTANCE_NAME: &str = "Local\\ERXTools.SecureNotes.Instance.v1";

/// Keeps the named mutex alive for as long as it is held.
pub struct InstanceGuard {
    handle: HANDLE,
}

impl fmt::Debug for InstanceGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InstanceGuard { .. }")
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: `handle` came from `CreateMutexW`, is owned by this guard and
        // is closed exactly once here.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

/// Takes the one-instance mutex `name`. `Ok(None)` means another holder exists.
pub fn acquire_instance(name: &str) -> Result<Option<InstanceGuard>, NotesError> {
    if name.is_empty() || name.contains('\0') {
        return Err(NotesError::Internal);
    }
    let wide: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives this call,
    // and a null security-attributes pointer asks for the default descriptor.
    let handle = unsafe { CreateMutexW(ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        return Err(NotesError::Internal);
    }
    // SAFETY: this reads the calling thread's last-error code. It runs directly
    // after `CreateMutexW`, before any other Win32 call can overwrite it.
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already_exists {
        // SAFETY: `handle` is a valid handle owned by this call and is closed
        // exactly once here; the guard never stores it.
        unsafe {
            CloseHandle(handle);
        }
        return Ok(None);
    }
    Ok(Some(InstanceGuard { handle }))
}
