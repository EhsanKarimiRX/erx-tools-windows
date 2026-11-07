//! The network layer of the update check: a minimal HTTPS GET client on
//! WinHTTP (Windows' own TLS, certificate store and proxy settings — no new
//! crates, nothing extra to ship). Never used anywhere else in the app; the
//! webview itself stays offline (R3).

use std::sync::mpsc;
use std::time::Duration;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Networking::WinHttp::{
    ERROR_WINHTTP_CANNOT_CONNECT, ERROR_WINHTTP_CLIENT_AUTH_CERT_NEEDED,
    ERROR_WINHTTP_CONNECTION_ERROR, ERROR_WINHTTP_NAME_NOT_RESOLVED,
    ERROR_WINHTTP_SECURE_CERT_CN_INVALID, ERROR_WINHTTP_SECURE_CERT_DATE_INVALID,
    ERROR_WINHTTP_SECURE_CERT_REV_FAILED, ERROR_WINHTTP_SECURE_CERT_REVOKED,
    ERROR_WINHTTP_SECURE_CERT_WRONG_USAGE, ERROR_WINHTTP_SECURE_CHANNEL_ERROR,
    ERROR_WINHTTP_SECURE_FAILURE, ERROR_WINHTTP_SECURE_FAILURE_PROXY,
    ERROR_WINHTTP_SECURE_INVALID_CA, ERROR_WINHTTP_SECURE_INVALID_CERT, ERROR_WINHTTP_TIMEOUT,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts,
};

use super::result::{HttpErrorKind, HttpOutcome};

/// One HTTPS GET against this host and path family is the whole protocol.
pub const API_HOST: &str = "api.github.com";
pub const LATEST_RELEASE_PATH: &str = "/repos/EhsanKarimiRX/erx-tools-windows/releases/latest";

/// The exact headers sent with every update check (GitHub API conventions).
const REQUEST_HEADERS: &str =
    "Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n";

/// Wall-clock budget for the whole request. WinHTTP's own timeouts (set on
/// the session) bound each step; this guards the total.
const OVERALL_TIMEOUT: Duration = Duration::from_secs(15);

/// Body reading: 8 KiB chunks, at most 256 KiB total.
const READ_CHUNK: usize = 8192;
const MAX_BODY: usize = 256 * 1024;

/// The abstraction the command logic tests against (a fake `HttpGet` with
/// canned outcomes), and what the real client implements.
pub trait HttpGet: Send + Sync {
    fn get(&self, host: &str, path: &str) -> HttpOutcome;
}

/// The real client: one WinHTTP session, connection, request per call.
pub struct WinHttpClient {
    pub user_agent: String,
}

/// Builds the honest user agent GitHub sees (name, version, project page).
pub fn user_agent_for(version: &str) -> String {
    format!("ERX-Tools/{version} (+https://github.com/EhsanKarimiRX/erx-tools-windows)")
}

/// Maps a WinHTTP `GetLastError` code to an honest error kind, using the
/// named constants of the vendored windows-sys 0.61.2 (values verified
/// there: 12002, 12007, 12029, 12030, 12175 and the ERROR_WINHTTP_SECURE_*
/// family).
pub fn classify_winhttp_error(code: u32) -> HttpErrorKind {
    match code {
        ERROR_WINHTTP_TIMEOUT => HttpErrorKind::TimedOut,
        ERROR_WINHTTP_NAME_NOT_RESOLVED
        | ERROR_WINHTTP_CANNOT_CONNECT
        | ERROR_WINHTTP_CONNECTION_ERROR => HttpErrorKind::CannotReach,
        ERROR_WINHTTP_SECURE_FAILURE
        | ERROR_WINHTTP_SECURE_CHANNEL_ERROR
        | ERROR_WINHTTP_SECURE_FAILURE_PROXY
        | ERROR_WINHTTP_SECURE_CERT_CN_INVALID
        | ERROR_WINHTTP_SECURE_CERT_DATE_INVALID
        | ERROR_WINHTTP_SECURE_CERT_REVOKED
        | ERROR_WINHTTP_SECURE_CERT_REV_FAILED
        | ERROR_WINHTTP_SECURE_CERT_WRONG_USAGE
        | ERROR_WINHTTP_SECURE_INVALID_CA
        | ERROR_WINHTTP_SECURE_INVALID_CERT
        // The server wants a client certificate: a TLS-layer problem.
        | ERROR_WINHTTP_CLIENT_AUTH_CERT_NEEDED => HttpErrorKind::Tls,
        _ => HttpErrorKind::Other,
    }
}

/// One NUL-terminated UTF-16 string for the flat C API.
pub(super) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Closes one HINTERNET handle when it goes out of scope, so every early
/// return releases the handles.
struct HttpHandle(*mut core::ffi::c_void);

impl Drop for HttpHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle came from a WinHttp creation function and is
            // closed exactly once, here.
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn failed_at_step(step: &str) -> HttpOutcome {
    // SAFETY: GetLastError is valid immediately after the failing WinHTTP
    // call; no other Windows call happened in between.
    let code = unsafe { GetLastError() };
    if cfg!(debug_assertions) {
        eprintln!("update check: WinHTTP error code {code} at step: {step}");
    }
    HttpOutcome::Error(classify_winhttp_error(code))
}

impl WinHttpClient {
    /// The blocking request. Runs on a worker thread (see `HttpGet::get`);
    /// every WinHTTP handle is owned by an RAII guard in reverse-declaration
    /// order, so all early returns close them (request, connection, session).
    fn get_blocking(&self, host: &str, path: &str) -> HttpOutcome {
        let user_agent = wide(&self.user_agent);
        let host = wide(host);
        let path = wide(path);
        let verb = wide("GET");
        let headers = wide(REQUEST_HEADERS);

        // SAFETY: no handles yet; the strings outlive the call.
        let session = unsafe {
            WinHttpOpen(
                user_agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
        if session.is_null() {
            return failed_at_step("WinHttpOpen");
        }
        let session = HttpHandle(session);

        // SAFETY: session is a live handle from WinHttpOpen.
        let timeouts_ok = unsafe { WinHttpSetTimeouts(session.0, 5000, 5000, 10000, 10000) };
        if timeouts_ok == 0 {
            return failed_at_step("WinHttpSetTimeouts");
        }

        // SAFETY: session is live; host is a NUL-terminated buffer.
        let connection = unsafe { WinHttpConnect(session.0, host.as_ptr(), 443, 0) };
        if connection.is_null() {
            return failed_at_step("WinHttpConnect");
        }
        let connection = HttpHandle(connection);

        // SAFETY: connection is live; verb/path are NUL-terminated buffers.
        // No option ignores certificate errors; redirect policy untouched.
        let request = unsafe {
            WinHttpOpenRequest(
                connection.0,
                verb.as_ptr(),
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        };
        if request.is_null() {
            return failed_at_step("WinHttpOpenRequest");
        }
        let request = HttpHandle(request);

        // SAFETY: request is live; headers is NUL-terminated with its length
        // in characters; no optional body is sent.
        let sent = unsafe {
            WinHttpSendRequest(
                request.0,
                headers.as_ptr(),
                headers.len() as u32 - 1,
                std::ptr::null(),
                0,
                0,
                0,
            )
        };
        if sent == 0 {
            return failed_at_step("WinHttpSendRequest");
        }

        // SAFETY: request is live and the response has been sent.
        let received = unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) };
        if received == 0 {
            return failed_at_step("WinHttpReceiveResponse");
        }

        // SAFETY: request is live; the status is queried as a 4-byte number
        // into a buffer of exactly that size, as the API expects.
        let mut status: u32 = 0;
        let mut status_size: u32 = size_of::<u32>() as u32;
        let status_ok = unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                std::ptr::addr_of_mut!(status).cast(),
                &mut status_size,
                std::ptr::null_mut(),
            )
        };
        if status_ok == 0 {
            return failed_at_step("WinHttpQueryHeaders");
        }

        // SAFETY: request is live; each read writes into a buffer sized
        // exactly by the bytes WinHTTP reported as available.
        let mut body: Vec<u8> = Vec::new();
        loop {
            let mut available: u32 = 0;
            let available_ok = unsafe { WinHttpQueryDataAvailable(request.0, &mut available) };
            if available_ok == 0 {
                return failed_at_step("WinHttpQueryDataAvailable");
            }
            if available == 0 {
                break;
            }
            let to_read = (available as usize).min(READ_CHUNK);
            let mut chunk = vec![0_u8; to_read];
            let mut read: u32 = 0;
            let read_ok = unsafe {
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    to_read as u32,
                    &mut read,
                )
            };
            if read_ok == 0 {
                return failed_at_step("WinHttpReadData");
            }
            body.extend_from_slice(&chunk[..(read as usize).min(to_read)]);
            if body.len() > MAX_BODY {
                return HttpOutcome::Error(HttpErrorKind::TooLarge);
            }
        }

        HttpOutcome::Response { status, body }
    }
}

impl HttpGet for WinHttpClient {
    /// Runs the blocking request on a worker thread and waits at most 15
    /// seconds. On timeout the answer is `TimedOut`; the abandoned thread
    /// ends by itself because WinHTTP has its own per-step timeouts.
    fn get(&self, host: &str, path: &str) -> HttpOutcome {
        let (sender, receiver) = mpsc::channel();
        let worker_client = WinHttpClient {
            user_agent: self.user_agent.clone(),
        };
        let worker_host = host.to_string();
        let worker_path = path.to_string();
        std::thread::spawn(move || {
            let outcome = worker_client.get_blocking(&worker_host, &worker_path);
            // The waiter may already be gone after a timeout; that is fine.
            let _ = sender.send(outcome);
        });
        match receiver.recv_timeout(OVERALL_TIMEOUT) {
            Ok(outcome) => outcome,
            Err(mpsc::RecvTimeoutError::Timeout) => HttpOutcome::Error(HttpErrorKind::TimedOut),
            // The worker died before answering; report it honestly here and
            // let `decide` turn it into the "could not be understood" message.
            Err(mpsc::RecvTimeoutError::Disconnected) => HttpOutcome::Error(HttpErrorKind::Other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_code_is_timed_out() {
        assert_eq!(
            classify_winhttp_error(ERROR_WINHTTP_TIMEOUT),
            HttpErrorKind::TimedOut
        );
    }

    #[test]
    fn resolution_and_connection_codes_cannot_reach() {
        for code in [
            ERROR_WINHTTP_NAME_NOT_RESOLVED,
            ERROR_WINHTTP_CANNOT_CONNECT,
            ERROR_WINHTTP_CONNECTION_ERROR,
        ] {
            assert_eq!(
                classify_winhttp_error(code),
                HttpErrorKind::CannotReach,
                "code {code}"
            );
        }
    }

    #[test]
    fn secure_failure_family_is_tls() {
        for code in [
            ERROR_WINHTTP_SECURE_FAILURE,
            ERROR_WINHTTP_SECURE_CHANNEL_ERROR,
            ERROR_WINHTTP_SECURE_FAILURE_PROXY,
            ERROR_WINHTTP_SECURE_CERT_CN_INVALID,
            ERROR_WINHTTP_SECURE_CERT_DATE_INVALID,
            ERROR_WINHTTP_SECURE_CERT_REVOKED,
            ERROR_WINHTTP_SECURE_CERT_REV_FAILED,
            ERROR_WINHTTP_SECURE_CERT_WRONG_USAGE,
            ERROR_WINHTTP_SECURE_INVALID_CA,
            ERROR_WINHTTP_SECURE_INVALID_CERT,
            ERROR_WINHTTP_CLIENT_AUTH_CERT_NEEDED,
        ] {
            assert_eq!(
                classify_winhttp_error(code),
                HttpErrorKind::Tls,
                "code {code}"
            );
        }
    }

    #[test]
    fn unknown_codes_are_other() {
        for code in [0_u32, 5, 1234, 12_345, u32::MAX] {
            assert_eq!(
                classify_winhttp_error(code),
                HttpErrorKind::Other,
                "code {code}"
            );
        }
    }

    #[test]
    fn user_agent_names_the_app_version_and_project() {
        assert_eq!(
            user_agent_for("0.1.0"),
            "ERX-Tools/0.1.0 (+https://github.com/EhsanKarimiRX/erx-tools-windows)"
        );
    }

    #[test]
    fn wide_strings_are_nul_terminated_utf16() {
        assert_eq!(wide("AB"), vec![65, 66, 0]);
        assert_eq!(wide(""), vec![0]);
    }
}
