//! Tiny read-only registry helper (HKEY_LOCAL_MACHINE; reading needs no
//! administrator rights) and the UTF-16 helper for the wide Windows APIs.

use std::ptr;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};

/// UTF-16 text with a terminating NUL.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reads one REG_SZ value below HKEY_LOCAL_MACHINE. None = missing, empty or
/// unreadable.
pub fn read_hklm_string(subkey: &str, value: &str) -> Option<String> {
    let subkey_wide = wide(subkey);
    let value_wide = wide(value);
    let mut size: u32 = 0;
    // First call: only asks how many bytes the value needs.
    let code = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey_wide.as_ptr(),
            value_wide.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut size,
        )
    };
    if code != ERROR_SUCCESS || size == 0 || size > 8192 {
        return None;
    }
    let mut buffer: Vec<u16> = vec![0; (size as usize).div_ceil(2) + 1];
    let mut capacity = u32::try_from(buffer.len() * 2).ok()?;
    let code = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey_wide.as_ptr(),
            value_wide.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut capacity,
        )
    };
    if code != ERROR_SUCCESS {
        return None;
    }
    let characters = (capacity as usize / 2).min(buffer.len());
    let text = String::from_utf16_lossy(buffer.get(..characters)?);
    let text = text.trim_end_matches('\0').trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}
