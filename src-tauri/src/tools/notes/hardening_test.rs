//! Hardening logic tests with a fake settings API. No window and no COM.

use super::hardening::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    ContextMenus,
    AcceleratorKeys,
    PasswordAutosave,
    ReadBack,
}

#[derive(Default)]
struct FakeApi {
    calls: Vec<Call>,
    fail_context_menus: bool,
    fail_accelerator_keys: bool,
    fail_password_autosave: bool,
    read_back: Option<Result<SettingsReadback, HardenError>>,
}

impl SettingsApi for FakeApi {
    fn disable_context_menus(&mut self) -> Result<(), HardenError> {
        self.calls.push(Call::ContextMenus);
        if self.fail_context_menus {
            Err(HardenError::Unavailable)
        } else {
            Ok(())
        }
    }

    fn disable_accelerator_keys(&mut self) -> Result<(), HardenError> {
        self.calls.push(Call::AcceleratorKeys);
        if self.fail_accelerator_keys {
            Err(HardenError::Unavailable)
        } else {
            Ok(())
        }
    }

    fn disable_password_autosave(&mut self) -> Result<(), HardenError> {
        self.calls.push(Call::PasswordAutosave);
        if self.fail_password_autosave {
            Err(HardenError::Unavailable)
        } else {
            Ok(())
        }
    }

    fn read_back(&mut self) -> Result<SettingsReadback, HardenError> {
        self.calls.push(Call::ReadBack);
        self.read_back.unwrap_or(Ok(EXPECTED_READBACK))
    }
}

#[test]
fn h1_ok_path() {
    let mut api = FakeApi::default();
    assert_eq!(harden_with(&mut api), Ok(EXPECTED_READBACK));
    assert_eq!(
        api.calls,
        vec![
            Call::ContextMenus,
            Call::AcceleratorKeys,
            Call::PasswordAutosave,
            Call::ReadBack
        ]
    );
}

#[test]
fn h2_context_menu_failure() {
    let mut api = FakeApi {
        fail_context_menus: true,
        ..FakeApi::default()
    };
    assert_eq!(harden_with(&mut api), Err(HardenError::Unavailable));
    assert_eq!(api.calls, vec![Call::ContextMenus]);
}

#[test]
fn h3_accelerator_failure() {
    let mut api = FakeApi {
        fail_accelerator_keys: true,
        ..FakeApi::default()
    };
    assert_eq!(harden_with(&mut api), Err(HardenError::Unavailable));
    assert_eq!(api.calls, vec![Call::ContextMenus, Call::AcceleratorKeys]);
}

#[test]
fn h4_password_failure() {
    let mut api = FakeApi {
        fail_password_autosave: true,
        ..FakeApi::default()
    };
    assert_eq!(harden_with(&mut api), Err(HardenError::Unavailable));
    assert_eq!(
        api.calls,
        vec![
            Call::ContextMenus,
            Call::AcceleratorKeys,
            Call::PasswordAutosave
        ]
    );
}

#[test]
fn h5_read_back_failure() {
    let mut api = FakeApi {
        read_back: Some(Err(HardenError::Unavailable)),
        ..FakeApi::default()
    };
    assert_eq!(harden_with(&mut api), Err(HardenError::Unavailable));
    assert_eq!(
        api.calls,
        vec![
            Call::ContextMenus,
            Call::AcceleratorKeys,
            Call::PasswordAutosave,
            Call::ReadBack
        ]
    );
}

#[test]
fn h6_each_wrong_value() {
    assert_eq!(verify_readback(&EXPECTED_READBACK), Ok(()));

    let mut cases: Vec<(&str, SettingsReadback)> = Vec::new();

    let mut wrong = EXPECTED_READBACK;
    wrong.general_autofill = !wrong.general_autofill;
    cases.push(("general_autofill", wrong));

    let mut wrong = EXPECTED_READBACK;
    wrong.password_autosave = !wrong.password_autosave;
    cases.push(("password_autosave", wrong));

    let mut wrong = EXPECTED_READBACK;
    wrong.default_context_menus = !wrong.default_context_menus;
    cases.push(("default_context_menus", wrong));

    let mut wrong = EXPECTED_READBACK;
    wrong.accelerator_keys = !wrong.accelerator_keys;
    cases.push(("accelerator_keys", wrong));

    let mut wrong = EXPECTED_READBACK;
    wrong.in_private = !wrong.in_private;
    cases.push(("in_private", wrong));

    for (field, wrong) in cases {
        assert_eq!(
            verify_readback(&wrong),
            Err(HardenError::NotApplied),
            "verify_readback must reject a wrong {field}"
        );

        let mut api = FakeApi {
            read_back: Some(Ok(wrong)),
            ..FakeApi::default()
        };
        assert_eq!(
            harden_with(&mut api),
            Err(HardenError::NotApplied),
            "harden_with must reject a wrong {field}"
        );
    }
}

#[test]
fn h7_all_wrong() {
    let wrong = SettingsReadback {
        general_autofill: true,
        password_autosave: true,
        default_context_menus: true,
        accelerator_keys: true,
        in_private: false,
    };
    let mut api = FakeApi {
        read_back: Some(Ok(wrong)),
        ..FakeApi::default()
    };
    assert_eq!(harden_with(&mut api), Err(HardenError::NotApplied));
}

#[test]
fn h8_constants() {
    assert_eq!(HARDEN_TIMEOUT, std::time::Duration::from_secs(5));
    assert_eq!(
        EXPECTED_READBACK,
        SettingsReadback {
            general_autofill: false,
            password_autosave: false,
            default_context_menus: false,
            accelerator_keys: false,
            in_private: true,
        }
    );
}

#[test]
fn h9_source_scan() {
    let source = include_str!("hardening.rs");
    assert_eq!(
        source
            .matches("SetAreDefaultContextMenusEnabled(false)")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("SetAreBrowserAcceleratorKeysEnabled(false)")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("SetIsPasswordAutosaveEnabled(false)")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("SetAreDefaultContextMenusEnabled(true)")
            .count(),
        0
    );
    assert_eq!(
        source
            .matches("SetAreBrowserAcceleratorKeysEnabled(true)")
            .count(),
        0
    );
    assert_eq!(
        source.matches("SetIsPasswordAutosaveEnabled(true)").count(),
        0
    );
}
