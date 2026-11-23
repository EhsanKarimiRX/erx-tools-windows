//! Turning browser features off in the Secure Notes WebView2 window.
//!
//! After the window exists, four settings are switched off through the
//! WebView2 COM interfaces and then read back. The window may only be shown
//! when every read-back value is the expected one; any error or any other
//! value keeps it hidden.
//!
//! The settings logic is kept behind [`SettingsApi`] so the tests can use a
//! fake. The COM layer itself has no automatic test; it is checked by hand in
//! the running app.

use std::time::Duration;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2_13, ICoreWebView2Settings, ICoreWebView2Settings3,
    ICoreWebView2Settings4,
};
use windows::core::{BOOL, Interface};

/// How long the caller waits for the main thread to answer.
pub const HARDEN_TIMEOUT: Duration = Duration::from_secs(5);

/// The four settings and the profile mode, as read back from WebView2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsReadback {
    /// General form autofill.
    pub general_autofill: bool,
    /// Saving passwords.
    pub password_autosave: bool,
    /// The browser's own context menus.
    pub default_context_menus: bool,
    /// Browser accelerator keys such as Ctrl+P.
    pub accelerator_keys: bool,
    /// InPrivate profile mode.
    pub in_private: bool,
}

/// The only accepted values.
pub const EXPECTED_READBACK: SettingsReadback = SettingsReadback {
    general_autofill: false,
    password_autosave: false,
    default_context_menus: false,
    accelerator_keys: false,
    in_private: true,
};

/// Why the window could not be hardened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardenError {
    /// The WebView2 interfaces could not be reached or a call failed.
    Unavailable,
    /// A setting read back with an unexpected value.
    NotApplied,
    /// The main thread did not answer in time.
    TimedOut,
}

/// What the logic needs from WebView2. The real implementation talks to COM;
/// tests use a fake.
pub trait SettingsApi {
    /// Turns the browser context menus off.
    fn disable_context_menus(&mut self) -> Result<(), HardenError>;
    /// Turns the browser accelerator keys off.
    fn disable_accelerator_keys(&mut self) -> Result<(), HardenError>;
    /// Turns password saving off.
    fn disable_password_autosave(&mut self) -> Result<(), HardenError>;
    /// Reads the settings back.
    fn read_back(&mut self) -> Result<SettingsReadback, HardenError>;
}

/// Ok only when `readback` equals [`EXPECTED_READBACK`].
pub fn verify_readback(readback: &SettingsReadback) -> Result<(), HardenError> {
    if *readback == EXPECTED_READBACK {
        Ok(())
    } else {
        Err(HardenError::NotApplied)
    }
}

/// Turns the three setters off, reads back, and verifies the result.
pub fn harden_with(api: &mut dyn SettingsApi) -> Result<SettingsReadback, HardenError> {
    api.disable_context_menus()?;
    api.disable_accelerator_keys()?;
    api.disable_password_autosave()?;
    let readback = api.read_back()?;
    verify_readback(&readback)?;
    Ok(readback)
}

/// The real WebView2 interfaces, gathered once on the main thread.
struct ComSettings {
    core: ICoreWebView2,
    settings: ICoreWebView2Settings,
    settings3: ICoreWebView2Settings3,
    settings4: ICoreWebView2Settings4,
}

impl SettingsApi for ComSettings {
    fn disable_context_menus(&mut self) -> Result<(), HardenError> {
        // SAFETY: the interface belongs to the live webview and is used on the
        // main thread, where WebView2 requires all calls to happen.
        unsafe { self.settings.SetAreDefaultContextMenusEnabled(false) }
            .map_err(|_| HardenError::Unavailable)
    }

    fn disable_accelerator_keys(&mut self) -> Result<(), HardenError> {
        // SAFETY: the interface belongs to the live webview and is used on the
        // main thread, where WebView2 requires all calls to happen.
        unsafe { self.settings3.SetAreBrowserAcceleratorKeysEnabled(false) }
            .map_err(|_| HardenError::Unavailable)
    }

    fn disable_password_autosave(&mut self) -> Result<(), HardenError> {
        // SAFETY: the interface belongs to the live webview and is used on the
        // main thread, where WebView2 requires all calls to happen.
        unsafe { self.settings4.SetIsPasswordAutosaveEnabled(false) }
            .map_err(|_| HardenError::Unavailable)
    }

    fn read_back(&mut self) -> Result<SettingsReadback, HardenError> {
        // SAFETY: all interfaces belong to the same live webview and are used
        // on the main thread, where WebView2 requires all calls to happen.
        unsafe {
            let mut general_autofill = BOOL::default();
            self.settings4
                .IsGeneralAutofillEnabled(&mut general_autofill)
                .map_err(|_| HardenError::Unavailable)?;

            let mut password_autosave = BOOL::default();
            self.settings4
                .IsPasswordAutosaveEnabled(&mut password_autosave)
                .map_err(|_| HardenError::Unavailable)?;

            let mut default_context_menus = BOOL::default();
            self.settings
                .AreDefaultContextMenusEnabled(&mut default_context_menus)
                .map_err(|_| HardenError::Unavailable)?;

            let mut accelerator_keys = BOOL::default();
            self.settings3
                .AreBrowserAcceleratorKeysEnabled(&mut accelerator_keys)
                .map_err(|_| HardenError::Unavailable)?;

            let core13 = self
                .core
                .cast::<ICoreWebView2_13>()
                .map_err(|_| HardenError::Unavailable)?;
            let profile = core13.Profile().map_err(|_| HardenError::Unavailable)?;
            let mut in_private = BOOL::default();
            profile
                .IsInPrivateModeEnabled(&mut in_private)
                .map_err(|_| HardenError::Unavailable)?;

            Ok(SettingsReadback {
                general_autofill: general_autofill.as_bool(),
                password_autosave: password_autosave.as_bool(),
                default_context_menus: default_context_menus.as_bool(),
                accelerator_keys: accelerator_keys.as_bool(),
                in_private: in_private.as_bool(),
            })
        }
    }
}

/// Runs on the main thread and hardens the given WebView2 webview.
pub fn harden_webview(
    webview: tauri::webview::PlatformWebview,
) -> Result<SettingsReadback, HardenError> {
    let mut api = {
        // SAFETY: `with_webview` calls this on the main thread while the
        // webview is alive, so the interfaces may be fetched and used.
        unsafe {
            let core = webview
                .controller()
                .CoreWebView2()
                .map_err(|_| HardenError::Unavailable)?;
            let settings = core.Settings().map_err(|_| HardenError::Unavailable)?;
            let settings3 = settings
                .cast::<ICoreWebView2Settings3>()
                .map_err(|_| HardenError::Unavailable)?;
            let settings4 = settings
                .cast::<ICoreWebView2Settings4>()
                .map_err(|_| HardenError::Unavailable)?;
            ComSettings {
                core,
                settings,
                settings3,
                settings4,
            }
        }
    };
    harden_with(&mut api)
}

/// Hardens a window from a blocking caller. Must never be called on the main
/// thread; the caller is an async command that runs it through
/// `spawn_blocking`.
pub fn harden_window(window: &tauri::WebviewWindow) -> Result<SettingsReadback, HardenError> {
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .with_webview(move |webview| {
            let _ = tx.send(harden_webview(webview));
        })
        .map_err(|_| HardenError::Unavailable)?;
    rx.recv_timeout(HARDEN_TIMEOUT)
        .map_err(|_| HardenError::TimedOut)?
}
