//! Tauri commands for the update check: run the one HTTPS request when the
//! user clicks the update button, remember the release page of the last
//! result in memory only, and open that page in the user's browser on
//! request. Nothing runs automatically (R3).

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, State};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::http::{API_HOST, HttpGet, LATEST_RELEASE_PATH, WinHttpClient, user_agent_for, wide};
use super::release::{RELEASES_PAGE_URL, is_allowed_release_url};
use super::result::{UpdateCheckResult, decide};
use crate::tools::error::ToolError;

/// Shown when the check worker died without an answer (never expected).
const WORKER_FAILED_MESSAGE: &str = "GitHub's answer could not be understood.";

/// Shown when the browser could not be opened; always points the user at
/// the page directly.
const BROWSER_ERROR_MESSAGE: &str = "Couldn't open your browser. You can visit github.com/EhsanKarimiRX/erx-tools-windows/releases yourself.";

/// The page URL of the last `UpdateAvailable` result. Memory only: cleared
/// on every other result and never written to disk.
#[derive(Default)]
pub struct UpdateState {
    page_url: Mutex<Option<String>>,
}

/// The version the home screen shows — the same call `get_app_info` makes,
/// so the update check always compares against the displayed number.
pub fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Runs one check through the given client (the real WinHTTP client in the
/// app, a fake in tests) and decides the result.
fn run_check(current: &str, client: &dyn HttpGet) -> (UpdateCheckResult, Option<String>) {
    let outcome = client.get(API_HOST, LATEST_RELEASE_PATH);
    decide(current, &outcome)
}

fn set_page_url(state: &UpdateState, url: Option<String>) {
    let mut guard = match state.page_url.lock() {
        Ok(guard) => guard,
        // A panic elsewhere must not break the update button forever.
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = url;
}

fn remembered_page_url(state: &UpdateState) -> Option<String> {
    let guard = match state.page_url.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clone()
}

/// Which URL `open_update_page` may open: the remembered one, else the plain
/// releases page — and only if the allowlist accepts it.
fn page_to_open(remembered: Option<String>) -> Option<String> {
    let url = remembered.unwrap_or_else(|| RELEASES_PAGE_URL.to_string());
    if is_allowed_release_url(&url) {
        Some(url)
    } else {
        None
    }
}

/// Opens a URL in the default browser with `ShellExecuteW`, verb "open".
/// Quick Launch's helper is ShellExecuteExW-based and private to that
/// module, so this small dedicated opener matches the update page's needs.
fn open_url_in_browser(url: &str) -> bool {
    let operation = wide("open");
    let file = wide(url);
    // SAFETY: both string pointers point at NUL-terminated buffers that
    // outlive the call; ShellExecuteW only reads them and returns a value.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Windows convention: a value greater than 32 means success.
    (result as isize) > 32
}

#[tauri::command]
pub async fn check_for_updates(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<UpdateCheckResult, ToolError> {
    let current = current_version(&app);
    // The request is blocking WinHTTP, so it runs on a worker thread; the
    // async runtime thread is never stuck waiting for the network.
    let (result, page) = tauri::async_runtime::spawn_blocking(move || {
        #[cfg(debug_assertions)]
        if let Some(fake) = fake_result(&current) {
            std::thread::sleep(Duration::from_millis(700));
            let page = if matches!(fake, UpdateCheckResult::UpdateAvailable { .. }) {
                Some(RELEASES_PAGE_URL.to_string())
            } else {
                None
            };
            return (fake, page);
        }

        let client = WinHttpClient {
            user_agent: user_agent_for(&current),
        };
        run_check(&current, &client)
    })
    .await
    .unwrap_or_else(|_| {
        // A panicked or failed worker must never crash the app or lie; this
        // is the same honest message as an unreadable GitHub answer.
        (
            UpdateCheckResult::Failed {
                message: WORKER_FAILED_MESSAGE.to_string(),
            },
            None,
        )
    });

    set_page_url(state.inner(), page);
    Ok(result)
}

#[tauri::command]
pub fn open_update_page(state: State<'_, UpdateState>) -> Result<(), String> {
    match page_to_open(remembered_page_url(state.inner())) {
        Some(url) if open_url_in_browser(&url) => Ok(()),
        _ => Err(BROWSER_ERROR_MESSAGE.to_string()),
    }
}

/// Development-only switch so the owner can see every badge state before
/// any release exists. Set `ERX_FAKE_UPDATE` to `available`, `uptodate`,
/// `norelease` or `failed`; any other value (or unset) runs the real check.
/// Compiled out of release builds — it never ships.
#[cfg(debug_assertions)]
fn fake_result(current: &str) -> Option<UpdateCheckResult> {
    match std::env::var("ERX_FAKE_UPDATE").ok().as_deref() {
        Some("available") => Some(UpdateCheckResult::UpdateAvailable {
            current: current.to_string(),
            latest: "9.9.9".to_string(),
        }),
        Some("uptodate") => Some(UpdateCheckResult::UpToDate {
            current: current.to_string(),
            latest: current.to_string(),
        }),
        Some("norelease") => Some(UpdateCheckResult::NoRelease {
            current: current.to_string(),
        }),
        Some("failed") => Some(UpdateCheckResult::Failed {
            message: "Couldn't reach GitHub. Check your internet connection and try again."
                .to_string(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::result::{HttpErrorKind, HttpOutcome};
    use super::*;

    /// A canned `HttpGet` for command-level logic tests (no Windows calls).
    struct FakeClient {
        outcome: HttpOutcome,
    }

    impl FakeClient {
        fn response(status: u32, body: Vec<u8>) -> Self {
            Self {
                outcome: HttpOutcome::Response { status, body },
            }
        }

        fn error(kind: HttpErrorKind) -> Self {
            Self {
                outcome: HttpOutcome::Error(kind),
            }
        }
    }

    impl HttpGet for FakeClient {
        fn get(&self, _host: &str, _path: &str) -> HttpOutcome {
            self.outcome.clone()
        }
    }

    fn body(tag: &str, url: &str) -> Vec<u8> {
        serde_json::json!({
            "tag_name": tag,
            "html_url": url,
            "draft": false,
            "prerelease": false,
        })
        .to_string()
        .into_bytes()
    }

    const OWN_TAG_URL: &str =
        "https://github.com/EhsanKarimiRX/erx-tools-windows/releases/tag/v0.2.0";

    #[test]
    fn newer_tag_returns_update_available_and_remembers_the_page() {
        let (result, page) = run_check(
            "0.1.0",
            &FakeClient::response(200, body("v0.2.0", OWN_TAG_URL)),
        );
        assert!(matches!(
            result,
            UpdateCheckResult::UpdateAvailable { ref latest, .. } if latest == "0.2.0"
        ));
        assert_eq!(page, Some(OWN_TAG_URL.to_string()));
    }

    #[test]
    fn http_404_returns_no_release_and_no_page() {
        let (result, page) = run_check("0.1.0", &FakeClient::response(404, b"{}".to_vec()));
        assert!(matches!(result, UpdateCheckResult::NoRelease { .. }));
        assert_eq!(page, None);
    }

    #[test]
    fn error_kinds_return_failed_and_no_page() {
        for client in [
            FakeClient::error(HttpErrorKind::CannotReach),
            FakeClient::error(HttpErrorKind::TimedOut),
            FakeClient::error(HttpErrorKind::Tls),
            FakeClient::error(HttpErrorKind::Other),
        ] {
            let (result, page) = run_check("0.1.0", &client);
            assert!(matches!(result, UpdateCheckResult::Failed { .. }));
            assert_eq!(page, None);
        }
    }

    #[test]
    fn remembered_url_is_set_on_available_and_cleared_on_everything_else() {
        let state = UpdateState::default();

        let (_, page) = run_check(
            "0.1.0",
            &FakeClient::response(200, body("v0.2.0", OWN_TAG_URL)),
        );
        set_page_url(&state, page);
        assert_eq!(remembered_page_url(&state), Some(OWN_TAG_URL.to_string()));

        for client in [
            FakeClient::response(404, b"{}".to_vec()),
            FakeClient::response(200, body("0.1.0", OWN_TAG_URL)),
            FakeClient::error(HttpErrorKind::Tls),
        ] {
            let (_, page) = run_check("0.1.0", &client);
            set_page_url(&state, page);
            assert_eq!(remembered_page_url(&state), None);
        }
    }

    #[test]
    fn page_to_open_prefers_the_remembered_url_and_rechecks_the_allowlist() {
        assert_eq!(
            page_to_open(Some(OWN_TAG_URL.to_string())),
            Some(OWN_TAG_URL.to_string())
        );
        // Nothing remembered (or disallowed) falls back to the releases page.
        assert_eq!(page_to_open(None), Some(RELEASES_PAGE_URL.to_string()));
        assert_eq!(
            page_to_open(Some("https://github.com.evil.com/x".to_string())),
            None
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn fake_switch_covers_every_state() {
        // SAFETY: process-wide env mutation; the only test touching
        // ERX_FAKE_UPDATE, so no other test can observe it.
        unsafe { std::env::set_var("ERX_FAKE_UPDATE", "available") };
        let fake = fake_result("0.1.0").expect("available must produce a fake");
        assert!(matches!(
            fake,
            UpdateCheckResult::UpdateAvailable { ref latest, .. } if latest == "9.9.9"
        ));

        unsafe { std::env::set_var("ERX_FAKE_UPDATE", "uptodate") };
        assert!(matches!(
            fake_result("0.1.0"),
            Some(UpdateCheckResult::UpToDate { .. })
        ));
        unsafe { std::env::set_var("ERX_FAKE_UPDATE", "norelease") };
        assert!(matches!(
            fake_result("0.1.0"),
            Some(UpdateCheckResult::NoRelease { .. })
        ));
        unsafe { std::env::set_var("ERX_FAKE_UPDATE", "failed") };
        assert!(matches!(
            fake_result("0.1.0"),
            Some(UpdateCheckResult::Failed { .. })
        ));

        // Unknown values and an unset variable run the real check instead.
        unsafe { std::env::set_var("ERX_FAKE_UPDATE", "banana") };
        assert_eq!(fake_result("0.1.0"), None);
        unsafe { std::env::remove_var("ERX_FAKE_UPDATE") };
        assert_eq!(fake_result("0.1.0"), None);
    }

    /// Live tests that touch the real network (#[ignore]; run with
    /// `cargo test update -- --ignored --nocapture` from `src-tauri`).
    /// Never part of the normal test run (spec 9c-5).

    #[test]
    #[ignore]
    fn live_update_check_real_github() {
        let client = WinHttpClient {
            user_agent: user_agent_for(env!("CARGO_PKG_VERSION")),
        };
        let outcome = client.get(API_HOST, LATEST_RELEASE_PATH);
        match &outcome {
            HttpOutcome::Response { status, body } => {
                println!(
                    "Live answer: HTTP {status}, body: {}",
                    String::from_utf8_lossy(body)
                );
                // Expected today (no release published yet): HTTP 404.
            }
            HttpOutcome::Error(kind) => println!("Live answer: error {kind:?}"),
        }
    }

    #[test]
    #[ignore]
    fn live_update_check_unreachable_host() {
        let client = WinHttpClient {
            user_agent: user_agent_for(env!("CARGO_PKG_VERSION")),
        };
        // Nothing serves HTTPS on 127.0.0.1:443; WinHTTP's error code is
        // printed by the client (debug builds) and the answer must be an
        // honest reachability failure.
        let outcome = client.get("127.0.0.1", LATEST_RELEASE_PATH);
        println!("Outcome for unreachable host: {outcome:?}");
        assert!(matches!(
            outcome,
            HttpOutcome::Error(HttpErrorKind::CannotReach | HttpErrorKind::TimedOut)
        ));
    }
}
