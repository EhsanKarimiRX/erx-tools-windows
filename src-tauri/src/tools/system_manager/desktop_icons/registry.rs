//! The desktop-icons registry behind a small trait: `RegistryBackend` for
//! the logic, `WindowsRegistry` for the real HKCU key (no admin needed),
//! `FakeRegistry` for tests. Only the one NewStartPanel key is ever
//! touched — never ClassicStartMenu or anything else — and values are only
//! ever written as explicit 0/1, never deleted.

/// The one key Desktop Icons uses (HKCU; created on write if missing).
pub const NEW_START_PANEL_SUBKEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel";

pub type RegistryResult<T> = Result<T, String>;

pub trait RegistryBackend {
    /// Reads one REG_DWORD of the NewStartPanel key; Ok(None) = absent.
    fn read_dword(&self, value_name: &str) -> RegistryResult<Option<u32>>;

    /// Writes one REG_DWORD into the NewStartPanel key (creates the key if
    /// missing). Never deletes.
    fn write_dword(&self, value_name: &str, value: u32) -> RegistryResult<()>;
}

#[cfg(windows)]
pub mod windows {
    use std::ptr;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, REG_VALUE_TYPE, RRF_RT_REG_DWORD,
        RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW,
    };

    use super::{NEW_START_PANEL_SUBKEY, RegistryBackend, RegistryResult};

    /// UTF-16 with a null terminator, for the wide registry APIs.
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn fail(what: &str, code: u32) -> String {
        format!("{what} (Windows error code {code}).")
    }

    pub struct WindowsRegistry;

    impl RegistryBackend for WindowsRegistry {
        fn read_dword(&self, value_name: &str) -> RegistryResult<Option<u32>> {
            let subkey_wide = wide(NEW_START_PANEL_SUBKEY);
            let name_wide = wide(value_name);
            let mut value: u32 = 0;
            let mut size: u32 = size_of::<u32>() as u32;
            let mut kind: REG_VALUE_TYPE = 0;
            let code = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    subkey_wide.as_ptr(),
                    name_wide.as_ptr(),
                    RRF_RT_REG_DWORD,
                    &mut kind,
                    ptr::addr_of_mut!(value).cast(),
                    &mut size,
                )
            };
            match code {
                ERROR_SUCCESS => Ok(Some(value)),
                ERROR_FILE_NOT_FOUND => Ok(None),
                _ => Err(fail("Couldn't read the desktop icon setting", code)),
            }
        }

        fn write_dword(&self, value_name: &str, value: u32) -> RegistryResult<()> {
            let subkey_wide = wide(NEW_START_PANEL_SUBKEY);
            let name_wide = wide(value_name);
            let mut key: HKEY = ptr::null_mut();
            // Creates the key (and parents) if missing; HKCU needs no admin.
            let code = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    subkey_wide.as_ptr(),
                    0,
                    ptr::null(),
                    0,
                    KEY_SET_VALUE,
                    ptr::null(),
                    &mut key,
                    ptr::null_mut(),
                )
            };
            if code != ERROR_SUCCESS {
                return Err(fail("Couldn't open the desktop icon settings key", code));
            }
            let bytes = value.to_ne_bytes();
            let code = unsafe {
                RegSetValueExW(
                    key,
                    name_wide.as_ptr(),
                    0,
                    REG_DWORD,
                    bytes.as_ptr(),
                    size_of::<u32>() as u32,
                )
            };
            unsafe { RegCloseKey(key) };
            if code != ERROR_SUCCESS {
                return Err(fail("Couldn't change the desktop icon setting", code));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
pub mod fake {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::RegistryBackend;
    use super::RegistryResult;

    struct FakeState {
        values: HashMap<String, u32>,
        write_log: Vec<(String, u32)>,
    }

    /// In-memory registry for tests: values per name; None = absent.
    pub struct FakeRegistry {
        state: RefCell<FakeState>,
        pub fail_writes_for: Vec<String>,
        pub fail_reads_for: Vec<String>,
    }

    impl FakeRegistry {
        pub fn new() -> Self {
            FakeRegistry {
                state: RefCell::new(FakeState {
                    values: HashMap::new(),
                    write_log: Vec::new(),
                }),
                fail_writes_for: Vec::new(),
                fail_reads_for: Vec::new(),
            }
        }

        pub fn get(&self, value_name: &str) -> Option<u32> {
            self.state.borrow().values.get(value_name).copied()
        }

        pub fn contains(&self, value_name: &str) -> bool {
            self.state.borrow().values.contains_key(value_name)
        }

        pub fn write_count(&self) -> usize {
            self.state.borrow().write_log.len()
        }
    }

    impl Default for FakeRegistry {
        fn default() -> Self {
            FakeRegistry::new()
        }
    }

    impl RegistryBackend for FakeRegistry {
        fn read_dword(&self, value_name: &str) -> RegistryResult<Option<u32>> {
            if self.fail_reads_for.iter().any(|name| name == value_name) {
                return Err(format!("simulated read failure for {value_name}"));
            }
            Ok(self.state.borrow().values.get(value_name).copied())
        }

        fn write_dword(&self, value_name: &str, value: u32) -> RegistryResult<()> {
            if self.fail_writes_for.iter().any(|name| name == value_name) {
                return Err(format!("simulated write failure for {value_name}"));
            }
            let mut state = self.state.borrow_mut();
            state.values.insert(value_name.to_string(), value);
            state.write_log.push((value_name.to_string(), value));
            Ok(())
        }
    }
}
