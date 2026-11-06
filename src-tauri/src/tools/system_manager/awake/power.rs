//! The Windows power API behind a small trait, so every Awake decision can
//! be unit-tested against an in-memory fake. The real backend (`WindowsPower`)
//! is added in the Windows-only part of this file; the fake lives in test
//! builds only.

/// Lowercase hyphenated GUID strings ("381b4222-…") are the scheme currency
/// of the pure logic; the Windows backend converts them for the API.
pub type PowerResult<T> = Result<T, String>;

/// An in-memory power backend for engine tests: simulates values, failures
/// and manual changes. Test-builds only.
#[cfg(test)]
pub mod fake {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    use super::super::core::PowerKey;
    use super::PowerBackend;
    use super::PowerResult;

    pub const DEFAULT_SCHEME: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
    pub const OTHER_SCHEME: &str = "99999999-1111-2222-3333-444444444444";

    struct FakeState {
        /// Value per (scheme, key).
        values: HashMap<(String, PowerKey), u32>,
        active: String,
        /// Existing-but-inactive schemes (for scheme_exists checks).
        known_schemes: Vec<String>,
        /// Every successful write, in order (for order assertions).
        write_log: Vec<(String, PowerKey, u32)>,
    }

    pub struct FakePower {
        state: RefCell<FakeState>,
        /// Simulated write failure for one key (None = all writes succeed).
        pub fail_write: Cell<Option<PowerKey>>,
        /// Simulated verify mismatch: reads of this key return value + 1.
        pub corrupt_read: Cell<Option<PowerKey>>,
        pub fail_activate: Cell<bool>,
    }

    impl FakePower {
        pub fn new(active: &str) -> Self {
            FakePower {
                state: RefCell::new(FakeState {
                    values: HashMap::new(),
                    active: active.to_string(),
                    known_schemes: vec![active.to_string()],
                    write_log: Vec::new(),
                }),
                fail_write: Cell::new(None),
                corrupt_read: Cell::new(None),
                fail_activate: Cell::new(false),
            }
        }

        /// Gives every key of every known scheme a readable "original" value.
        pub fn with_originals(self) -> Self {
            let mut state = self.state.borrow_mut();
            for scheme in state.known_schemes.clone() {
                for key in PowerKey::ALL {
                    state
                        .values
                        .insert((scheme.clone(), key), 1800 + key.as_str().len() as u32);
                }
            }
            drop(state);
            self
        }

        /// Simulates the user changing a value by hand in Windows.
        pub fn set_manual(&self, scheme: &str, key: PowerKey, value: u32) {
            let mut state = self.state.borrow_mut();
            state.values.insert((scheme.to_string(), key), value);
            if !state.known_schemes.iter().any(|s| s == scheme) {
                state.known_schemes.push(scheme.to_string());
            }
        }

        pub fn get(&self, scheme: &str, key: PowerKey) -> Option<u32> {
            self.state
                .borrow()
                .values
                .get(&(scheme.to_string(), key))
                .copied()
        }

        pub fn write_log(&self) -> Vec<(String, PowerKey, u32)> {
            self.state.borrow().write_log.clone()
        }

        pub fn write_count(&self) -> usize {
            self.state.borrow().write_log.len()
        }

        /// Makes one value unreadable (simulates a failing read).
        pub fn make_unreadable(&self, scheme: &str, key: PowerKey) {
            self.state
                .borrow_mut()
                .values
                .remove(&(scheme.to_string(), key));
        }

        /// Simulates the user switching the active scheme.
        pub fn switch_active(&self, scheme: &str) {
            let mut state = self.state.borrow_mut();
            state.active = scheme.to_string();
            if !state.known_schemes.iter().any(|s| s == scheme) {
                state.known_schemes.push(scheme.to_string());
            }
        }

        /// Adds a known (existing) scheme without touching values.
        pub fn add_known_scheme(&self, scheme: &str) {
            let mut state = self.state.borrow_mut();
            if !state.known_schemes.iter().any(|s| s == scheme) {
                state.known_schemes.push(scheme.to_string());
            }
        }
    }

    impl PowerBackend for FakePower {
        fn active_scheme(&self) -> PowerResult<String> {
            Ok(self.state.borrow().active.clone())
        }

        fn read(&self, scheme: &str, key: PowerKey) -> PowerResult<u32> {
            let state = self.state.borrow();
            if scheme != state.active && !state.known_schemes.iter().any(|s| s == scheme) {
                return Err(format!("scheme {scheme} does not exist (fake)"));
            }
            match state.values.get(&(scheme.to_string(), key)) {
                Some(value) => {
                    if self.corrupt_read.get() == Some(key) {
                        Ok(value + 1)
                    } else {
                        Ok(*value)
                    }
                }
                None => Err(format!("unreadable key {} (fake)", key.as_str())),
            }
        }

        fn write(&self, scheme: &str, key: PowerKey, value: u32) -> PowerResult<()> {
            if self.fail_write.get() == Some(key) {
                return Err(format!("simulated write failure for {}", key.as_str()));
            }
            let mut state = self.state.borrow_mut();
            if scheme != state.active && !state.known_schemes.iter().any(|s| s == scheme) {
                return Err(format!("scheme {scheme} does not exist (fake)"));
            }
            state.values.insert((scheme.to_string(), key), value);
            state.write_log.push((scheme.to_string(), key, value));
            Ok(())
        }

        fn activate(&self, _scheme: &str) -> PowerResult<()> {
            if self.fail_activate.get() {
                return Err("simulated activation failure (fake)".into());
            }
            Ok(())
        }

        fn scheme_exists(&self, scheme: &str) -> PowerResult<bool> {
            let state = self.state.borrow();
            Ok(scheme == state.active || state.known_schemes.iter().any(|s| s == scheme))
        }
    }
}

pub trait PowerBackend {
    /// GUID of the currently active power scheme.
    fn active_scheme(&self) -> PowerResult<String>;

    /// Reads one value of one scheme. `Err` = unreadable (callers treat this
    /// as unknown and never touch that key).
    fn read(&self, scheme: &str, key: super::core::PowerKey) -> PowerResult<u32>;

    /// Writes one value of one scheme. Does not apply it; `activate` does.
    fn write(&self, scheme: &str, key: super::core::PowerKey, value: u32) -> PowerResult<()>;

    /// Re-activates a scheme so written values take effect.
    fn activate(&self, scheme: &str) -> PowerResult<()>;

    /// Whether a scheme still exists (needed by restore/reconcile R3/R4).
    fn scheme_exists(&self, scheme: &str) -> PowerResult<bool>;
}

#[cfg(windows)]
pub mod windows {
    //! Real backend over `windows-sys` power APIs. All functions return a
    //! plain Win32 error code (0 = success). Passes null for the user root
    //! power key (per-user power settings need no admin — verified live).
    //! `PowerGetActiveScheme` returns a GUID that must be freed with LocalFree.

    use std::ptr;

    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, LocalFree,
    };
    use windows_sys::Win32::System::Power::{
        PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex, PowerSetActiveScheme,
        PowerWriteACValueIndex, PowerWriteDCValueIndex,
    };
    use windows_sys::core::GUID;

    use super::super::core::PowerKey;
    use super::PowerBackend;
    use super::PowerResult;

    // windows-sys 0.61 spells the GUID fields lowercase (data1, data4, …).
    const SUB_SLEEP: GUID = GUID {
        data1: 0x238C_9FA8,
        data2: 0x0AAD,
        data3: 0x41ED,
        data4: [0x83, 0xF4, 0x97, 0xBE, 0x24, 0x2C, 0x8F, 0x20],
    };
    const SUB_VIDEO: GUID = GUID {
        data1: 0x7516_B95F,
        data2: 0xF776,
        data3: 0x4464,
        data4: [0x8C, 0x53, 0x06, 0x16, 0x7F, 0x40, 0xCC, 0x99],
    };
    const STANDBYIDLE: GUID = GUID {
        data1: 0x29F6_C1DB,
        data2: 0x86DA,
        data3: 0x48C5,
        data4: [0x9F, 0xDB, 0xF2, 0xB6, 0x7B, 0x1F, 0x44, 0xDA],
    };
    const HIBERNATEIDLE: GUID = GUID {
        data1: 0x9D78_15A6,
        data2: 0x7EE4,
        data3: 0x497E,
        data4: [0x88, 0x88, 0x51, 0x5A, 0x05, 0xF0, 0x23, 0x64],
    };
    const VIDEOIDLE: GUID = GUID {
        data1: 0x3C0B_C021,
        data2: 0xC8A8,
        data3: 0x4E07,
        data4: [0xA9, 0x73, 0x6B, 0x14, 0xCB, 0xCB, 0x2B, 0x7E],
    };

    /// Human hint for the Win32 codes we actually expect to see.
    fn error_hint(code: u32) -> &'static str {
        match code {
            ERROR_ACCESS_DENIED => "access denied",
            ERROR_FILE_NOT_FOUND => "not found",
            _ => "unexpected error",
        }
    }

    fn fail(what: &str, code: u32) -> String {
        format!("{what} (Windows error code {code}, {}).", error_hint(code))
    }

    fn parse_guid(text: &str) -> PowerResult<GUID> {
        let clean: String = text.chars().filter(|&c| c != '-').collect();
        if clean.len() != 32 || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("'{text}' is not a valid power scheme id."));
        }
        let data1 = u32::from_str_radix(&clean[0..8], 16).map_err(|_| bad_guid(text))?;
        let data2 = u16::from_str_radix(&clean[8..12], 16).map_err(|_| bad_guid(text))?;
        let data3 = u16::from_str_radix(&clean[12..16], 16).map_err(|_| bad_guid(text))?;
        let mut data4 = [0u8; 8];
        for (index, byte) in data4.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&clean[16 + index * 2..18 + index * 2], 16)
                .map_err(|_| bad_guid(text))?;
        }
        Ok(GUID {
            data1,
            data2,
            data3,
            data4,
        })
    }

    fn bad_guid(text: &str) -> String {
        format!("'{text}' is not a valid power scheme id.")
    }

    fn guid_to_string(guid: &GUID) -> String {
        format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            guid.data1,
            guid.data2,
            guid.data3,
            guid.data4[0],
            guid.data4[1],
            guid.data4[2],
            guid.data4[3],
            guid.data4[4],
            guid.data4[5],
            guid.data4[6],
            guid.data4[7]
        )
    }

    /// (subgroup GUID, setting GUID) for one key; AC/DC is chosen by caller.
    fn guids(key: PowerKey) -> (GUID, GUID) {
        match key {
            PowerKey::SleepAc | PowerKey::SleepDc => (SUB_SLEEP, STANDBYIDLE),
            PowerKey::HibernateAc | PowerKey::HibernateDc => (SUB_SLEEP, HIBERNATEIDLE),
            PowerKey::DisplayAc | PowerKey::DisplayDc => (SUB_VIDEO, VIDEOIDLE),
        }
    }

    pub struct WindowsPower;

    impl PowerBackend for WindowsPower {
        fn active_scheme(&self) -> PowerResult<String> {
            unsafe {
                let mut guid: *mut GUID = ptr::null_mut();
                let code = PowerGetActiveScheme(ptr::null_mut(), &mut guid);
                if code != ERROR_SUCCESS {
                    return Err(fail("Awake could not read the active power scheme", code));
                }
                if guid.is_null() {
                    return Err(
                        "Awake could not read the active power scheme: Windows returned none."
                            .into(),
                    );
                }
                let text = guid_to_string(&*guid);
                LocalFree(guid.cast());
                Ok(text)
            }
        }

        fn read(&self, scheme: &str, key: PowerKey) -> PowerResult<u32> {
            let scheme_guid = parse_guid(scheme)?;
            let (subgroup, setting) = guids(key);
            let mut value: u32 = 0;
            let code = unsafe {
                if key.as_str().ends_with("_dc") {
                    PowerReadDCValueIndex(
                        ptr::null_mut(),
                        &scheme_guid,
                        &subgroup,
                        &setting,
                        &mut value,
                    )
                } else {
                    PowerReadACValueIndex(
                        ptr::null_mut(),
                        &scheme_guid,
                        &subgroup,
                        &setting,
                        &mut value,
                    )
                }
            };
            if code != ERROR_SUCCESS {
                return Err(fail(&format!("Awake could not read {}", key.label()), code));
            }
            Ok(value)
        }

        fn write(&self, scheme: &str, key: PowerKey, value: u32) -> PowerResult<()> {
            let scheme_guid = parse_guid(scheme)?;
            let (subgroup, setting) = guids(key);
            let code = unsafe {
                if key.as_str().ends_with("_dc") {
                    PowerWriteDCValueIndex(
                        ptr::null_mut(),
                        &scheme_guid,
                        &subgroup,
                        &setting,
                        value,
                    )
                } else {
                    PowerWriteACValueIndex(
                        ptr::null_mut(),
                        &scheme_guid,
                        &subgroup,
                        &setting,
                        value,
                    )
                }
            };
            if code != ERROR_SUCCESS {
                return Err(fail(
                    &format!("Awake could not change {}", key.label()),
                    code,
                ));
            }
            Ok(())
        }

        fn activate(&self, scheme: &str) -> PowerResult<()> {
            let scheme_guid = parse_guid(scheme)?;
            let code = unsafe { PowerSetActiveScheme(ptr::null_mut(), &scheme_guid) };
            if code != ERROR_SUCCESS {
                return Err(fail("Awake could not apply the power scheme", code));
            }
            Ok(())
        }

        fn scheme_exists(&self, scheme: &str) -> PowerResult<bool> {
            // Probe with one read of the stored scheme. [VERIFY live] A missing
            // scheme is expected to surface as ERROR_FILE_NOT_FOUND; if the
            // real machine returns a different code, this match gets adjusted.
            let scheme_guid = parse_guid(scheme)?;
            let mut value: u32 = 0;
            let code = unsafe {
                PowerReadACValueIndex(
                    ptr::null_mut(),
                    &scheme_guid,
                    &SUB_SLEEP,
                    &STANDBYIDLE,
                    &mut value,
                )
            };
            match code {
                ERROR_SUCCESS => Ok(true),
                ERROR_FILE_NOT_FOUND => Ok(false),
                _ => Err(fail("Awake could not check the power scheme", code)),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn guid_strings_round_trip() {
            let text = "381b4222-f694-41f0-9685-ff5bb260df2e";
            let guid = parse_guid(text).expect("parse");
            assert_eq!(guid_to_string(&guid), text);
            assert!(parse_guid("nope").is_err());
            assert!(parse_guid("381b4222-f694-41f0-9685-ff5bb260df2zz").is_err());
        }

        #[test]
        fn error_hints_are_plain_english() {
            assert!(fail("test", ERROR_ACCESS_DENIED).contains("access denied"));
        }
    }
}
