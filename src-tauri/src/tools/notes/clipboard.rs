//! Copying note text to the Windows clipboard from Rust.
//!
//! The notes page used to copy with `navigator.clipboard`, which Windows
//! clipboard history may keep. Here Rust writes the text itself together with
//! three marker formats: Windows clipboard history, the cloud clipboard and
//! clipboard monitors are asked to skip the text. The text is removed again
//! after 30 seconds, but only while the clipboard still holds our own write
//! (the clipboard sequence number has not changed). It is also removed when
//! the app exits.
//!
//! Honest limits: any program can read the clipboard while the text is on it.
//! The marker formats are a Windows convention, not a guarantee. Whether
//! history really skips the text is proven by hand (Win+V), not by unit
//! tests. The Win32 layer `WinClipboard` has no automatic test either, except
//! the ignored test c14, which uses the real clipboard once.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, State};
use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use zeroize::Zeroizing;

use super::commands::{NotesCommandError, NotesState, check_window_label, with_session};
use super::error::NotesError;

/// How long our text stays on the clipboard when nobody else copies.
pub const CLEAR_AFTER: Duration = Duration::from_secs(30);
/// How often opening the clipboard is tried before it counts as busy.
pub const OPEN_ATTEMPTS: u32 = 10;
/// The pause between two attempts to open the clipboard.
pub const OPEN_DELAY: Duration = Duration::from_millis(20);
/// How often the background thread looks at the clipboard.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// The largest text we accept, the same size as the largest note.
pub const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
/// Marker formats (Windows clipboard history / cloud / monitor exclusion); each is written as a 4-byte value 0.
pub const MARKER_FORMAT_NAMES: [&str; 3] = [
    "CanIncludeInClipboardHistory",
    "CanUploadToCloudClipboard",
    "ExcludeClipboardContentFromMonitorProcessing",
];

/// Why a clipboard operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardError {
    /// The text is empty, too large or contains a NUL.
    InvalidText,
    /// Another program holds the clipboard.
    Busy,
    /// Windows refused the operation.
    Failed,
}

/// What the background thread has to do with our text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearPlan {
    /// The clipboard sequence number right after our write.
    pub sequence: u32,
    /// The moment the text should be removed.
    pub deadline: Instant,
}

/// What `decide` says about a plan at a given moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearDecision {
    /// Not due yet.
    Wait,
    /// Due and still ours.
    Clear,
    /// The clipboard belongs to someone else now.
    Forget,
}

/// The result of one look at the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollOutcome {
    /// There is nothing to clear.
    Idle,
    /// The plan is not due yet.
    Waiting,
    /// Our text was removed.
    Cleared,
    /// The clipboard changed, the plan was dropped.
    Forgot,
    /// The removal failed; it is retried.
    ClearFailed,
}

/// Turns the text into UTF-16 with a closing 0.
pub fn encode_text(text: &str) -> Result<Zeroizing<Vec<u16>>, ClipboardError> {
    if text.is_empty() || text.len() > MAX_TEXT_BYTES || text.contains('\0') {
        return Err(ClipboardError::InvalidText);
    }
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    Ok(Zeroizing::new(units))
}

/// Says what to do with the plan now.
pub fn decide(plan: &ClearPlan, now: Instant, current_sequence: u32) -> ClearDecision {
    if current_sequence != plan.sequence {
        ClearDecision::Forget
    } else if now >= plan.deadline {
        ClearDecision::Clear
    } else {
        ClearDecision::Wait
    }
}

/// Maps a clipboard error to the error the frontend sees.
pub fn error_to_command_error(error: ClipboardError) -> NotesCommandError {
    let (code, message) = match error {
        ClipboardError::InvalidText => {
            ("clipboard_invalid_text", "There is no valid text to copy.")
        }
        ClipboardError::Busy => (
            "clipboard_busy",
            "The clipboard is in use by another program. Try again.",
        ),
        ClipboardError::Failed => ("clipboard_failed", "Could not copy to the clipboard."),
    };
    NotesCommandError {
        code: code.to_string(),
        message: message.to_string(),
    }
}

/// The Win32 clipboard, behind a trait so the logic can be tested.
pub trait ClipboardApi: Send {
    /// Replaces the clipboard with the text (UTF-16 with a closing 0) plus the marker formats; returns the clipboard sequence number AFTER the write.
    fn write_text(&mut self, utf16: &[u16]) -> Result<u32, ClipboardError>;
    /// The current clipboard sequence number.
    fn current_sequence(&mut self) -> u32;
    /// Empties the clipboard.
    fn clear(&mut self) -> Result<(), ClipboardError>;
}

/// The state of our text on the clipboard.
struct Inner {
    api: Box<dyn ClipboardApi>,
    plan: Option<ClearPlan>,
}

/// The copy / clear logic. One lock for both, held for the whole operation, so
/// a copy and a poll never race.
pub struct ClipboardCore {
    inner: Mutex<Inner>,
}

impl ClipboardCore {
    /// Builds the logic around a clipboard.
    pub fn new(api: Box<dyn ClipboardApi>) -> ClipboardCore {
        ClipboardCore {
            inner: Mutex::new(Inner { api, plan: None }),
        }
    }

    /// Writes the text and remembers when to remove it.
    pub fn copy(&self, text: &str, now: Instant) -> Result<(), ClipboardError> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let utf16 = encode_text(text)?;
        let sequence = inner.api.write_text(&utf16)?;
        inner.plan = Some(ClearPlan {
            sequence,
            deadline: now + CLEAR_AFTER,
        });
        Ok(())
    }

    /// Looks at the clipboard once.
    pub fn poll(&self, now: Instant) -> PollOutcome {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(plan) = inner.plan else {
            return PollOutcome::Idle;
        };
        let current_sequence = inner.api.current_sequence();
        match decide(&plan, now, current_sequence) {
            ClearDecision::Wait => PollOutcome::Waiting,
            ClearDecision::Forget => {
                inner.plan = None;
                PollOutcome::Forgot
            }
            ClearDecision::Clear => {
                if inner.api.clear().is_ok() {
                    inner.plan = None;
                    PollOutcome::Cleared
                } else {
                    PollOutcome::ClearFailed
                }
            }
        }
    }

    /// Removes our text now. True only when it really cleared.
    pub fn clear_now(&self) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(plan) = inner.plan.take() else {
            return false;
        };
        inner.api.current_sequence() == plan.sequence && inner.api.clear().is_ok()
    }
}

impl Default for ClipboardCore {
    fn default() -> ClipboardCore {
        ClipboardCore::new(Box::new(WinClipboard))
    }
}

/// The real Windows clipboard.
pub struct WinClipboard;

impl WinClipboard {
    /// Opens the clipboard, retrying while another program holds it.
    fn open() -> Result<(), ClipboardError> {
        for attempt in 0..OPEN_ATTEMPTS {
            // SAFETY: a null owner window asks for the clipboard without an owner.
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return Ok(());
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                std::thread::sleep(OPEN_DELAY);
            }
        }
        Err(ClipboardError::Busy)
    }

    /// Best effort after a failure: leave nothing of ours on the clipboard.
    fn fail() -> Result<u32, ClipboardError> {
        // SAFETY: both calls only touch the clipboard of this thread; a failure
        // here is ignored because we are already reporting an error.
        unsafe {
            EmptyClipboard();
            CloseClipboard();
        }
        Err(ClipboardError::Failed)
    }

    /// Puts `bytes` on the clipboard under `format`.
    fn set_global(format: u32, bytes: &[u8]) -> bool {
        // SAFETY: the clipboard is open and owned by this thread; the memory is
        // handed to the clipboard on success and freed on every failure.
        unsafe {
            let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if memory.is_null() {
                return false;
            }
            let pointer = GlobalLock(memory);
            if pointer.is_null() {
                GlobalFree(memory);
                return false;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer as *mut u8, bytes.len());
            GlobalUnlock(memory);
            if SetClipboardData(format, memory).is_null() {
                GlobalFree(memory);
                return false;
            }
            true
        }
    }
}

impl ClipboardApi for WinClipboard {
    fn write_text(&mut self, utf16: &[u16]) -> Result<u32, ClipboardError> {
        Self::open()?;
        // SAFETY: the clipboard is open and owned by this thread.
        if unsafe { EmptyClipboard() } == 0 {
            // SAFETY: closing is allowed while we own the clipboard.
            unsafe { CloseClipboard() };
            return Err(ClipboardError::Failed);
        }
        for name in MARKER_FORMAT_NAMES {
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: `wide` is a valid null-terminated UTF-16 string for the call.
            let format = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
            if format == 0 {
                return Self::fail();
            }
            if !Self::set_global(format, &0u32.to_le_bytes()) {
                return Self::fail();
            }
        }
        let bytes: Vec<u8> = utf16.iter().flat_map(|unit| unit.to_le_bytes()).collect();
        if !Self::set_global(CF_UNICODETEXT as u32, &bytes) {
            return Self::fail();
        }
        // SAFETY: closing the clipboard we own.
        unsafe { CloseClipboard() };
        // SAFETY: no pointers; reads the global clipboard counter.
        Ok(unsafe { GetClipboardSequenceNumber() })
    }

    fn current_sequence(&mut self) -> u32 {
        // SAFETY: no pointers; reads the global clipboard counter.
        unsafe { GetClipboardSequenceNumber() }
    }

    fn clear(&mut self) -> Result<(), ClipboardError> {
        Self::open()?;
        // SAFETY: the clipboard is open and owned by this thread.
        if unsafe { EmptyClipboard() } == 0 {
            // SAFETY: closing is allowed while we own the clipboard.
            unsafe { CloseClipboard() };
            return Err(ClipboardError::Failed);
        }
        // SAFETY: closing the clipboard we own.
        unsafe { CloseClipboard() };
        Ok(())
    }
}

/// The managed state of the notes clipboard.
#[derive(Default)]
pub struct NotesClipboardState {
    core: Arc<ClipboardCore>,
}

/// Starts the one background thread that removes our text after 30 seconds.
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    // A failed start is ignored: without the timer the text stays until the
    // next copy or until the exit hook clears it.
    let _ = std::thread::Builder::new()
        .name("notes-clipboard".to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(POLL_INTERVAL);
                handle
                    .state::<NotesClipboardState>()
                    .core
                    .poll(Instant::now());
            }
        });
}

/// Removes our text from the clipboard when the app exits.
pub fn on_exit(app: &AppHandle) {
    let Some(state) = app.try_state::<NotesClipboardState>() else {
        return;
    };
    let _ = state.core.clear_now();
}

/// Copies the selected note text to the clipboard.
#[tauri::command]
pub async fn notes_copy_text(
    window: tauri::Window,
    clipboard: State<'_, NotesClipboardState>,
    notes: State<'_, NotesState>,
    text: String,
) -> Result<(), NotesCommandError> {
    if !check_window_label(window.label()) {
        return Err(NotesCommandError {
            code: "wrong_window".to_string(),
            message: "This window may not use Secure Notes.".to_string(),
        });
    }
    let text = Zeroizing::new(text);
    with_session(&notes.shared(), |session| {
        if session.status().unlocked {
            Ok(())
        } else {
            Err(NotesError::NotUnlocked)
        }
    })?;
    let core = Arc::clone(&clipboard.core);
    tauri::async_runtime::spawn_blocking(move || core.copy(text.as_str(), Instant::now()))
        .await
        .map_err(|_| NotesCommandError::internal())?
        .map_err(error_to_command_error)
}
